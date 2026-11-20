//! blob（`docs/designs/07-存储.md` 第五节）：图片、文件、超长的工具输出、策略快照这些大内容，按
//! 内容哈希存成单独的文件，事件里只放引用（`03-事件模型.md` E4）。数据根里一份：
//! `blobs/<前两位>/<64 位十六进制>`。
//!
//! 存：先写进 `tmp/` 里的临时文件、同步，再改名成它的哈希、同步目录。返回时它已经落了盘，这才能
//! 写引用它的事件（07 第四节「先落 blob，再写引用它的事件」）。改名是原子的，所以磁盘上不会有
//! 写了一半的 blob；崩溃留在 `tmp/` 里的，回收的时候再清。
//!
//! 分块读（`web-module.md`「七、分块读」，施工 W-6）：[`Blobs::read_range`] 一块一块读出来，不用整份先取
//! 进内存。
//!
//! 取的时候核对哈希（[`Blobs::get`]），核对过的记下文件的长短和修改时刻（施工 V-24）：同一个 `Blobs`（和从它拷出来的）再取，
//! 文件没变过的不再算。会话的每一次请求都把历史里的图片、文件取一遍，以前每一次都整份重算。
//!
//! 只核对、只量大小（[`Blobs::check`]），和打开了从某个位置读（[`Blobs::open_at`]），施工 V-38：发请求时照原样发的图片、
//! PDF 不整份取进内存，发的时候才一小块一小块地读出来编成 base64。

use std::collections::HashMap;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

use lm_kernel::kernel::id::{ContentHash, Hasher};

use crate::store::durable::{
    create_below, create_dir, create_dir_anywhere, create_temp, discard, sync_dir,
};

/// 放临时文件的目录。和前两位的目录（两位十六进制）撞不上。
const TMP: &str = "tmp";

/// 数据根里的 blob。
#[derive(Debug, Clone)]
pub struct Blobs {
    /// `blobs/`。
    dir: PathBuf,
    /// 核对过的：哪一个、核对时文件多长、修改时刻（施工 V-24）。拷出来的共用一份。
    checked: Arc<Mutex<HashMap<ContentHash, Stamp>>>,
    /// 会话的工具存改前改后用的（速度线三，[`Blobs::deferring`]）：[`Blobs::keep`] 存了不同步，登记在这里。没有的照
    /// [`Blobs::put`] 当场同步。
    unsynced: Option<Arc<Mutex<Unsynced>>>,
    /// 只在内存里的（应急模式，[`Blobs::in_memory`]）：存、取都照这张表，目录用不上。
    memory: Option<memory::Held>,
}

/// 一份文件的长短和修改时刻：变了就要重新核对。
type Stamp = (u64, SystemTime);

impl Blobs {
    /// 数据目录还在、blob 目录还没有的，建 blob 目录（造会话时先调，[`crate::session`] 的 `create`）：数据目录本身（`dir` 的上一层）
    /// 也照需要建，第一次用的时候它还没有。只在内存里的不用。
    ///
    /// # Errors
    ///
    /// 建不了。
    pub(crate) fn prepare(&self) -> io::Result<()> {
        match self.memory {
            Some(_) => Ok(()),
            None => create_dir_anywhere(&self.dir),
        }
    }

    /// 建 blob 目录里的一层 `dir`（临时文件那一层、分到的那一层）：只建数据目录（`self.dir` 的上一层）下面的，数据目录不在了的不
    /// 建回来（照 Miyu 施工 TMP-1，[`create_below`]）。数据目录说不出的（`self.dir` 没有上一层）照旧连上层建。
    fn make(&self, dir: &Path) -> io::Result<()> {
        match self
            .dir
            .parent()
            .filter(|root| !root.as_os_str().is_empty())
        {
            Some(root) => create_below(root, dir),
            None => create_dir(dir),
        }
    }

    /// blob 目录，平时是 [`Paths::blobs`](crate::store::paths::Paths::blobs)。用到时才建。
    pub fn new(dir: PathBuf) -> Blobs {
        Blobs {
            dir,
            checked: Arc::default(),
            unsynced: None,
            memory: None,
        }
    }

    /// 同一个目录，[`Blobs::keep`] 存了不同步、登记进交回的那一份（速度线三）：一个会话的工具用它，后台同步照它在引用它们的
    /// 事件写进日志以前同步。
    pub(crate) fn deferring(mut self) -> (Blobs, Arc<Mutex<Unsynced>>) {
        let unsynced = Arc::new(Mutex::new(Unsynced::default()));
        self.unsynced = Some(Arc::clone(&unsynced));
        (self, unsynced)
    }

    /// 存改前改后的内容（`session/effects.rs`）：[`Blobs::deferring`] 来的存了不同步、登记下（[`Blobs::put_unsynced`]），
    /// 别的照 [`Blobs::put`]。
    ///
    /// # Errors
    ///
    /// 同 [`Blobs::put_unsynced`]、[`Blobs::put`]。
    pub(crate) fn keep(&self, content: &[u8]) -> io::Result<ContentHash> {
        if let Some(held) = &self.memory {
            return Ok(held.put(content));
        }
        let Some(unsynced) = &self.unsynced else {
            return self.put(content);
        };
        let (hash, path) = self.put_unsynced(content)?;
        unsynced
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .add(path);
        Ok(hash)
    }

    /// 这个哈希的 blob 放在哪：`<前两位>/<64 位>`。
    pub fn path(&self, hash: &ContentHash) -> PathBuf {
        self.fan(hash).join(hash.hex())
    }

    /// 存一份内容，返回它的哈希。返回时它已经落了盘。已经有了的不重写，只把修改时间刷成现在：
    /// 回收的宽限期按修改时间算，刚又传了一遍的不能当成旧的删掉（07 第五节）。
    ///
    /// # Errors
    ///
    /// 建不了目录、写不进、同步不了、改不了名。
    pub fn put(&self, content: &[u8]) -> io::Result<ContentHash> {
        if let Some(held) = &self.memory {
            return Ok(held.put(content));
        }
        let hash = ContentHash::of(content);
        let fan = self.fan(&hash);
        let path = fan.join(hash.hex());
        if path.is_file() {
            freshen(&path)?;
            // 它可能是同一时刻别处刚改好名、还没同步目录的。
            sync_dir(&fan)?;
            return Ok(hash);
        }
        let tmp = self.dir.join(TMP);
        self.make(&tmp)?;
        let (temp, file) = create_temp(&tmp, temp_name)?;
        let stored = self
            .make(&fan)
            .and_then(|()| store(file, content, &temp, &fan, &path));
        if stored.is_err() {
            discard(&temp);
        }
        stored?;
        Ok(hash)
    }

    /// 同 [`Blobs::put`]，只是不同步（速度线三）：写进临时文件、改名成它的哈希，交回哈希和该同步的那个文件；数据、目录的两次
    /// 同步交给调的一方，在引用它的事件写进日志以前做（[`Unsynced`]）。已经有了的照旧刷修改时间；长短和内容对不上的（断电
    /// 时改了名、数据没落盘留下的短文件）照新的重写。
    ///
    /// # Errors
    ///
    /// 建不了目录、写不进、改不了名。
    pub(crate) fn put_unsynced(&self, content: &[u8]) -> io::Result<(ContentHash, PathBuf)> {
        let hash = ContentHash::of(content);
        let fan = self.fan(&hash);
        let path = fan.join(hash.hex());
        let whole = fs::metadata(&path)
            .is_ok_and(|meta| meta.is_file() && u64::try_from(content.len()) == Ok(meta.len()));
        if whole {
            freshen(&path)?;
            return Ok((hash, path));
        }
        let tmp = self.dir.join(TMP);
        self.make(&tmp)?;
        let (temp, mut file) = create_temp(&tmp, temp_name)?;
        let stored = file.write_all(content).and_then(|()| {
            drop(file);
            self.make(&fan)?;
            fs::rename(&temp, &path)
        });
        if stored.is_err() {
            discard(&temp);
        }
        stored?;
        Ok((hash, path))
    }

    /// 照文件 `source` 存一份（施工 7-12 下，后台命令的输出最多 5 GB）：边读边算哈希、边写进临时文件，不把整份放进内存；
    /// 读到的每一段也交给 `each`（数字数之类）。别的照 [`Blobs::put`]。
    ///
    /// # Errors
    ///
    /// 读不了 `source`；建不了目录、写不进、同步不了、改不了名。
    pub fn put_file(&self, source: &Path, each: impl FnMut(&[u8])) -> io::Result<ContentHash> {
        if let Some(held) = &self.memory {
            return held.put_file(source, each);
        }
        let tmp = self.dir.join(TMP);
        self.make(&tmp)?;
        let (temp, mut file) = create_temp(&tmp, temp_name)?;
        let copied = copy(source, &mut file, each).and_then(|hash| {
            file.sync_data()?;
            Ok(hash)
        });
        drop(file);
        let hash = match copied {
            Ok(hash) => hash,
            Err(error) => {
                discard(&temp);
                return Err(error);
            }
        };
        let fan = self.fan(&hash);
        let path = fan.join(hash.hex());
        if path.is_file() {
            discard(&temp);
            freshen(&path)?;
            sync_dir(&fan)?;
            return Ok(hash);
        }
        let stored = self
            .make(&fan)
            .and_then(|()| settle(&temp, &path))
            .and_then(|()| sync_dir(&fan));
        if stored.is_err() {
            discard(&temp);
        }
        stored?;
        Ok(hash)
    }

    /// 读一段：从 `offset` 起读最多 `length` 个字节，读到结尾就停；`offset` 过了结尾的是空的；`length` 写 0
    /// 只问大小。`size`（第二个值）是打开那一刻的大小；不重新核对整份内容的哈希，核对在核心自己用整份内容的
    /// 时候（施工 W-6，`web-module.md`「七、分块读」第 1、3、4 条）。安全地打开照 [`crate::fs::read_range`]，
    /// 和 `fs.read` 共用一份，不另写一套跟链接的判断。
    ///
    /// # Errors
    ///
    /// 没有这个 blob；读不了。
    pub fn read_range(
        &self,
        hash: &ContentHash,
        offset: u64,
        length: u64,
    ) -> Result<(Vec<u8>, u64), BlobError> {
        if let Some(held) = &self.memory {
            return held.read_range(hash, offset, length);
        }
        // 先把 blob 目录换成真实的位置：数据根本身可能经过一层链接（macOS 的临时目录在 `/var` 下，`/var` 是链接），安全地
        // 打开路上一层链接都不跟，不换就一个都打不开。blob 在核心自己的数据根里，不是人给的路径（2026-10-02 定）。只换
        // 目录：整条路径都换的话，blob 目录里被换成链接的那一份也顺着解开了，指到哪就读到哪（施工 SEC-3，外部审计 L-3）。
        let dir = match std::fs::canonicalize(&self.dir) {
            Ok(dir) => dir,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(BlobError::Missing(hash.clone()));
            }
            Err(error) => return Err(BlobError::Io(error)),
        };
        let real = Blobs::new(dir).path(hash);
        let segment =
            crate::fs::read_range(&real, offset, length).map_err(|error| match error {
                crate::fs::OpenError::NotFound => BlobError::Missing(hash.clone()),
                other => BlobError::Io(io::Error::other(other)),
            })?;
        Ok((segment.data, segment.size))
    }

    /// 取一份内容，核对它的哈希：读出来和名字对不上，报错，不悄悄用，也不删（07 第五节）。这个 `Blobs` 核对过、文件的长短
    /// 和修改时刻都没变的不再算（施工 V-24）：防的是文件被换、被改；一样长、修改时刻也一样的悄悄坏了查不出来。
    ///
    /// # Errors
    ///
    /// 没有这个 blob；读出来和名字对不上；读不了。
    pub fn get(&self, hash: &ContentHash) -> Result<Vec<u8>, BlobError> {
        if let Some(held) = &self.memory {
            return held.get(hash).map(|content| content.to_vec());
        }
        let mut file = match File::open(self.path(hash)) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(BlobError::Missing(hash.clone()));
            }
            Err(error) => return Err(BlobError::Io(error)),
        };
        // 长短、修改时刻照开着的这一份读，和读出来的内容是同一个文件。
        let stamp: Option<Stamp> = file
            .metadata()
            .ok()
            .and_then(|meta| Some((meta.len(), meta.modified().ok()?)));
        let mut content =
            Vec::with_capacity(stamp.map_or(0, |(len, _)| usize::try_from(len).unwrap_or(0)));
        file.read_to_end(&mut content).map_err(BlobError::Io)?;
        if stamp.is_some() && stamp == self.checked(hash) {
            return Ok(content);
        }
        if ContentHash::of(&content) != *hash {
            return Err(BlobError::Corrupt(hash.clone()));
        }
        if let (Some(stamp), Ok(mut checked)) = (stamp, self.checked.lock()) {
            checked.insert(hash.clone(), stamp);
        }
        Ok(content)
    }

    /// 核对一个 blob，交回它有多少个字节（施工 V-38）：照 [`Blobs::get`] 的规矩，这个 `Blobs` 核对过、文件的长短和修改时刻都
    /// 没变的不再算；要算的一块一块地读着算，不把整份放进内存。发请求时照原样发的图片、PDF 编码前先这样核对、量大小，
    /// 发的时候再照 [`Blobs::open_at`] 一小块一小块地读。
    ///
    /// # Errors
    ///
    /// 没有这个 blob；读出来和名字对不上；读不了。
    pub fn check(&self, hash: &ContentHash) -> Result<u64, BlobError> {
        if let Some(held) = &self.memory {
            return held
                .get(hash)
                .map(|content| u64::try_from(content.len()).unwrap_or(u64::MAX));
        }
        let (mut file, stamp) = self.open_stamped(hash)?;
        match stamp {
            Some((len, _)) if stamp == self.checked(hash) => Ok(len),
            _ => self.verify(hash, &mut file, stamp),
        }
    }

    /// 打开一个 blob、定位到第 `offset` 个字节，交回开着的文件（施工 V-38：发请求体时从这里一小块一小块地读）。核对过、文件的
    /// 长短和修改时刻都没变的直接交回；变过的、没核对过的先照 [`Blobs::check`] 一块一块地整份核对一遍，对得上才交回。
    ///
    /// # Errors
    ///
    /// 没有这个 blob；读出来和名字对不上；读不了、定位不了。
    pub fn open_at(&self, hash: &ContentHash, offset: u64) -> Result<File, BlobError> {
        let (mut file, stamp) = self.open_stamped(hash)?;
        if stamp.is_none() || stamp != self.checked(hash) {
            self.verify(hash, &mut file, stamp)?;
        }
        file.seek(SeekFrom::Start(offset)).map_err(BlobError::Io)?;
        Ok(file)
    }

    /// 打开一个 blob，连同打开的这一份的长短和修改时刻（读不出来的没有）。
    fn open_stamped(&self, hash: &ContentHash) -> Result<(File, Option<Stamp>), BlobError> {
        let file = match File::open(self.path(hash)) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Err(BlobError::Missing(hash.clone()));
            }
            Err(error) => return Err(BlobError::Io(error)),
        };
        let stamp = file
            .metadata()
            .ok()
            .and_then(|meta| Some((meta.len(), meta.modified().ok()?)));
        Ok((file, stamp))
    }

    /// 从头一块一块地读 `file`、算哈希，和名字对上了记下核对过（`stamp` 是打开时的长短和修改时刻），交回读了多少个字节。
    fn verify(
        &self,
        hash: &ContentHash,
        file: &mut File,
        stamp: Option<Stamp>,
    ) -> Result<u64, BlobError> {
        file.seek(SeekFrom::Start(0)).map_err(BlobError::Io)?;
        let mut hasher = Hasher::default();
        let mut buffer = vec![0u8; 64 * 1024];
        let mut total: u64 = 0;
        loop {
            let count = match file.read(&mut buffer) {
                Ok(0) => break,
                Ok(count) => count,
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(BlobError::Io(error)),
            };
            hasher.update(&buffer[..count]);
            total = total.saturating_add(u64::try_from(count).unwrap_or(u64::MAX));
        }
        if hasher.finish() != *hash {
            return Err(BlobError::Corrupt(hash.clone()));
        }
        if let (Some(stamp), Ok(mut checked)) = (stamp, self.checked.lock()) {
            checked.insert(hash.clone(), stamp);
        }
        Ok(total)
    }

    /// 核对 `hash` 那一次文件的长短和修改时刻；没核对过的没有。
    fn checked(&self, hash: &ContentHash) -> Option<Stamp> {
        self.checked
            .lock()
            .ok()
            .and_then(|checked| checked.get(hash).copied())
    }

    /// 前两位的目录。
    fn fan(&self, hash: &ContentHash) -> PathBuf {
        let hex = hash.hex();
        self.dir.join(hex.get(..2).unwrap_or(hex))
    }
}

/// 取不出来。
#[derive(Debug)]
pub enum BlobError {
    /// 没有这个 blob。
    Missing(ContentHash),
    /// 读出来的内容和它的名字对不上：磁盘坏了，或者被别的程序改过。不自动修，也不删。
    Corrupt(ContentHash),
    /// 读不了。
    Io(io::Error),
}

impl fmt::Display for BlobError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BlobError::Missing(hash) => write!(f, "no blob {hash}"),
            BlobError::Corrupt(hash) => {
                write!(f, "blob {hash} does not match its name; left as it is")
            }
            BlobError::Io(error) => error.fmt(f),
        }
    }
}

impl std::error::Error for BlobError {}

/// 写进临时文件、同步、关上（Windows 上开着的文件改不了名），改名成它的哈希，再同步目录。分到的那一层 `fan` 先建好了。
fn store(mut file: File, content: &[u8], temp: &Path, fan: &Path, path: &Path) -> io::Result<()> {
    file.write_all(content)?;
    file.sync_data()?;
    drop(file);
    settle(temp, path)?;
    sync_dir(fan)
}

/// 把 `source` 一段段抄进 `file`，边抄边算哈希，每一段也交给 `each`。
fn copy(source: &Path, file: &mut File, mut each: impl FnMut(&[u8])) -> io::Result<ContentHash> {
    let mut source = File::open(source)?;
    let mut hasher = Hasher::default();
    let mut buffer = vec![0u8; 64 * 1024];
    loop {
        let count = match source.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        };
        let chunk = &buffer[..count];
        hasher.update(chunk);
        each(chunk);
        file.write_all(chunk)?;
    }
    Ok(hasher.finish())
}

/// 改名成它的哈希。改名失败、可目标已经有了，算成功：两个会话同时存同一份，Windows 上后改名的
/// 那个可能失败，内容是一样的，删掉自己的临时文件就行。
fn settle(temp: &Path, path: &Path) -> io::Result<()> {
    match fs::rename(temp, path) {
        Ok(()) => Ok(()),
        Err(_) if path.is_file() => {
            discard(temp);
            Ok(())
        }
        Err(error) => Err(error),
    }
}

/// 临时文件的名字：进程号加一个计数。
fn temp_name() -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    format!(
        "{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}

/// 把修改时间刷成现在。
fn freshen(path: &Path) -> io::Result<()> {
    OpenOptions::new()
        .write(true)
        .open(path)?
        .set_modified(SystemTime::now())
}

mod memory;
mod unsynced;
pub(crate) use unsynced::{Unsynced, sync_files};

#[cfg(test)]
mod tests;
