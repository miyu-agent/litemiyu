//! 落盘的几件小事（`docs/designs/07-存储.md` 第四节「各平台的坑」）：同步目录；建目录时，新建的
//! 每一层都同步它的上一层。新建文件、新建目录、改名以后，上一层不同步，断电以后这一项可能没了。
//! 先写临时文件再改名的，临时文件怎么建、叫什么、用不上了怎么删（blob、生成的文件、配置文件共用，施工 8-1 从 `blob.rs`
//! 挪来，名字施工 8-3 从 `generated.rs` 挪来）。
//!

use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// 临时文件的名字最多换几次：撞上的都是崩溃留下的，换几次总能换开。
const TEMP_TRIES: u32 = 64;

/// 建目录，连同缺的上层（同 [`create_dir_anywhere`]）。Miyu 这里还要往上找数据根的标记才建；LiteMiyu 的数据放在 XDG 的
/// 几处（`crate::store::paths`），没有标记，被删了的照样建回来。blob 不照这个建，照 [`create_below`]（数据目录不在了不建回来）。
///
/// # Errors
///
/// 同 [`create_dir_anywhere`]。
pub(crate) fn create_dir(dir: &Path) -> io::Result<()> {
    create_dir_anywhere(dir)
}

/// 建 `root` 下面缺的几层，一直到 `dir`；`root` 本身不建，不在了的报 `NotFound`（照 Miyu 施工 TMP-1 的 `create_below`）。blob 照它
/// 建自己的目录（[`crate::store::blob::Blobs`]，`root` 是数据目录）：数据目录被删了（人删了还开着的程序的数据、测试的夹具删了以后
/// 后台任务收尾还在写），不再被后台写的那一下连上级目录一起建回来。
///
/// # Errors
///
/// `root` 不在了（`NotFound`，说的是哪个目录）；别的同 [`create_dir_anywhere`]。
pub(crate) fn create_below(root: &Path, dir: &Path) -> io::Result<()> {
    if dir == root {
        if root.is_dir() {
            return Ok(());
        }
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!(
                "the LiteMiyu data directory {} is gone, not creating it again",
                root.display()
            ),
        ));
    }
    if dir.is_dir() {
        return Ok(());
    }
    let parent = dir.parent().unwrap_or(root);
    create_below(root, parent)?;
    make_one(dir, Some(parent))
}

/// 建目录，连同缺的上层，不管在不在数据根里（建数据根本身、写人指定位置的配置文件）。新建的每一层都同步它的上一层，建
/// 目录这件事本身才算落盘；已经有的不动。
///
/// Unix 上新建的权限 0700，只有本人能进；已经有的不改：数据根可能是人自己建、自己设的，权限
/// 不对由 `miyu doctor` 报告（`22-命令行.md` 第五节）。
///
/// # Errors
///
/// 建不了；该是目录的地方是个文件；同步不了。
pub(crate) fn create_dir_anywhere(dir: &Path) -> io::Result<()> {
    if dir.is_dir() {
        return Ok(());
    }
    let parent = dir.parent().filter(|parent| !parent.as_os_str().is_empty());
    if let Some(parent) = parent {
        create_dir_anywhere(parent)?;
    }
    make_one(dir, parent)
}

/// 上一层 `parent` 已经有了，建 `dir` 这一层（Unix 上 0700），同步上一层；同一时刻别处建好的也算。
fn make_one(dir: &Path, parent: Option<&Path>) -> io::Result<()> {
    let created = {
        use std::os::unix::fs::DirBuilderExt;
        fs::DirBuilder::new().mode(0o700).create(dir)
    };
    match created {
        Ok(()) => {}
        // 同一时刻别处建好的，也算。
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists && dir.is_dir() => {
            return Ok(());
        }
        Err(error) => return Err(error),
    }
    match parent {
        Some(parent) => sync_dir(parent),
        None => Ok(()),
    }
}

/// 同步目录：新建文件、新建目录、改名以后做，这一项本身才算落盘。
///
/// # Errors
///
/// 打不开这个目录，或者同步不了。
pub(crate) fn sync_dir(dir: &Path) -> io::Result<()> {
    fs::File::open(dir)?.sync_all()
}

/// 在 `dir` 里新建一个临时文件，名字照 `name` 起，只许新建。撞上崩溃留下的同名文件，换下一个名字。
///
/// # Errors
///
/// 建不了；一连 64 个名字都被占了。
pub(crate) fn create_temp(dir: &Path, name: impl FnMut() -> String) -> io::Result<(PathBuf, File)> {
    create_temp_with(dir, name, None)
}

/// 同 [`create_temp`]；给了 `mode` 的 Unix 上建的时候就是这几个权限位（再照 umask 收）：密钥文件的是 0600，只有本人能读写
/// （施工 8-5：不能有一瞬间是别人读得到的）；替换一份配置文件的照原来的（施工 SEC-3，外部审计 L-2：写完以前不比原来的宽）。
///
/// # Errors
///
/// 建不了；一连 64 个名字都被占了。
pub(crate) fn create_temp_with(
    dir: &Path,
    mut name: impl FnMut() -> String,
    mode: Option<u32>,
) -> io::Result<(PathBuf, File)> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    if let Some(mode) = mode {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(mode);
    }
    for _ in 0..TEMP_TRIES {
        let path = dir.join(name());
        match options.open(&path) {
            Ok(file) => return Ok((path, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        format!(
            "{TEMP_TRIES} temporary file names in a row are taken in {}",
            dir.display()
        ),
    ))
}

/// 替换一份文件时的临时文件的名字：点开头（大多数系统上不显示），带上原来的名字 `name`、进程号和一个计数
/// （`.<文件名>.<进程号>-<计数>.tmp`，生成的文件、配置文件共用，施工 8-3 从 `generated.rs` 挪来）。
pub(crate) fn temp_name(name: &str) -> String {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    format!(
        ".{name}.{}-{}.tmp",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    )
}

/// 名字是不是 [`temp_name`] 起的：`.<文件名>.<进程号>-<计数>.tmp`，进程号、计数都是数字，文件名不是空的。
pub(crate) fn is_temp_name(name: &str) -> bool {
    let Some(inner) = name
        .strip_prefix('.')
        .and_then(|rest| rest.strip_suffix(".tmp"))
    else {
        return false;
    };
    let Some((file, numbers)) = inner.rsplit_once('.') else {
        return false;
    };
    let digits = |part: &str| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit());
    !file.is_empty()
        && numbers
            .split_once('-')
            .is_some_and(|(pid, count)| digits(pid) && digits(count))
}

/// 删掉目录 `dir` 里崩了留下的临时文件（施工 AS-11）：名字是 [`temp_name`] 起的那种、是普通文件、最后改动在 `now` 之前
/// 满 `age` 的。刚写的不碰（说不定是别的程序正在写），认不出的名字一个都不碰。交回删掉了哪几个；删不掉的说在哪、为什么。
///
/// # Errors
///
/// 目录读不了（没有这个目录的当没有，交回空的）。
pub(crate) fn sweep_temps(
    dir: &Path,
    now: std::time::SystemTime,
    age: std::time::Duration,
) -> io::Result<Swept> {
    let mut swept = Swept::default();
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(swept),
        Err(error) => return Err(error),
    };
    for entry in entries {
        let entry = entry?;
        let Some(name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        if !is_temp_name(&name) {
            continue;
        }
        let Ok(metadata) = fs::symlink_metadata(entry.path()) else {
            continue;
        };
        let old_enough = metadata
            .modified()
            .ok()
            .and_then(|modified| now.duration_since(modified).ok())
            .is_some_and(|since| since >= age);
        if !metadata.is_file() || !old_enough {
            continue;
        }
        match fs::remove_file(entry.path()) {
            Ok(()) => swept.removed.push(entry.path()),
            Err(error) => swept.failed.push((entry.path(), error)),
        }
    }
    Ok(swept)
}

/// [`crate::store::config_file::sweep_temps`] 的结果。
#[derive(Debug, Default)]
pub struct Swept {
    /// 删掉了的。
    pub removed: Vec<PathBuf>,
    /// 删不掉的：在哪，为什么。
    pub failed: Vec<(PathBuf, io::Error)>,
}

/// 删掉用不上的临时文件。
#[expect(
    clippy::let_underscore_must_use,
    reason = "删不掉就留着，不耽误这一次：blob 的由回收清，生成的文件的留在旁边"
)]
pub(crate) fn discard(temp: &Path) {
    let _ = fs::remove_file(temp);
}

#[cfg(test)]
mod tests;
