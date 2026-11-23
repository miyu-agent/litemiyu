//! 窗口之间的门铃（LM-4，设计第五节第 8 条）：开着的窗口绑一个 Unix 数据报套接字，发的一方把信改名进它的收件目录以后往这里
//! 敲一个字节，叫醒它去扫收件目录。信照旧只走文件（`letter.rs`），门铃只是叫醒：敲不响的（对方没开着、套接字被删了、队列满了）
//! 信也落了盘，收的一方开着的定时兜底扫（`listen.rs`），没开着的下次打开先读。只用标准库的 `UnixDatagram`，Linux、macOS、BSD
//! 一样，不用 inotify 这类哪一家独有的。
//!
//! 放在哪（[`Bell`]，2026-11-18 定）：
//!
//! 1. `$XDG_RUNTIME_DIR/litemiyu-<名字>.sock`：这个目录要在、是目录、属主是自己、别人没有任何权限（不比 0700 宽），照
//!    `stat` 查（`$XDG_RUNTIME_DIR` 本身是个链接的照它指的查）。
//! 2. 上一处不合用、太长的：`<tmp>/litemiyu-<uid>/<名字>.sock`。`<tmp>` 平常是 `/tmp`。目录没有的照 0700 建；建了的、本来有的
//!    都照 `lstat` 核对：是目录（不是链接）、属主是自己、别人没有任何权限。别人抢先建了同名目录、放了个链接的，这一处不用。
//!    `/tmp` 有粘滞位，属主是自己的目录别人删不了、换不了。
//! 3. 两处都不合用的不绑：这个窗口只靠扫（[`Bell::bare`]，扫得勤些）。
//!
//! 名字：会话目录先 `canonicalize`（收发两边都这样，链接、`..`、多余的斜杠算出一样的），取整条路径 SHA-256 的前 8 个字节写成
//! 16 位十六进制。不同的数据根、同一个数据根里不同的会话不撞。整条地址最多 [`MAX_PATH`] 字节（macOS、BSD 的 `sun_path` 104
//! 字节、要留结尾的 0；Linux 108）：超了的那一处当不合用。
//!
//! 绑（[`bind`]，`Window::claim` 拿到会话锁以后）：同一个会话只有拿着锁的窗口绑，地址上已经有文件的是上次崩溃留下的——先试着
//! 连一下，连得上的是有人在听（照理不会有：锁在这里），不拆它、这一处不用；连不上的才删掉重绑。绑好改成 0600。
//! 放（[`Placed::release`]，窗口放下锁以前）时删掉套接字文件：核对还是自己绑的那一个（设备号、inode），不是的不动。删会话
//! （[`forget`]，拿着会话锁、删目录以前）时删掉崩溃留下的：是套接字、又连不上的才删。
//!
//! 敲（[`ring`]）：非阻塞地往两处都敲一下（收的一方和发的一方环境不一样时也敲得到），出什么错都不管：没开着是常态。队列满了
//! 说明里面有没读的字节，对方一定会醒。
//!
//! 别的会话崩溃留下的（LM-14，[`sweep`]）：程序被杀掉以后它的套接字文件没人删，那个会话再打开、被删掉以前一直留着。绑门铃
//! 的时候（建会话、打开会话，本来就在阻塞线程里碰这两个目录）顺手看两处目录里别的门铃：名字严格照门铃的写法、是套接字、属主
//! 是自己、连上去被拒（没人在听）的删掉。一次最多看 [`SWEEP`] 个，看不完的下次绑门铃时再看。起来到出现输入框那一段不做。

use std::fs;
use std::os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::net::UnixDatagram;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::store::paths::Bell;

/// 绑门铃时顺手清旧门铃，一次最多看几个（[`sweep`]）：每看一个要 `lstat` 一次、连一次，没人听的再 `unlink` 一次。目录里
/// 攒了上千个的时候不在这一下卡住，剩下的留给以后每一次绑门铃，每次再清这么多。
pub(crate) const SWEEP: usize = 64;

/// 套接字地址最长多少字节：macOS、BSD 的 `sun_path` 是 104 字节、要留结尾的 0（Linux 是 108，照小的算，两边一样）。
pub(crate) const MAX_PATH: usize = 103;

/// 绑上了的门铃：套接字（交给等收件目录的任务）、绑在哪（窗口放下时删）。
#[derive(Debug)]
pub(crate) struct Bound {
    pub(crate) socket: UnixDatagram,
    pub(crate) placed: Placed,
}

/// 绑在哪：地址，和绑好那一刻那个文件的设备号、inode（删之前核对还是不是它）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Placed {
    pub(crate) path: PathBuf,
    dev: u64,
    ino: u64,
}

impl Placed {
    /// 删掉套接字文件：还是自己绑的那一个才删。碰磁盘，一次 `lstat` 一次 `unlink`。
    pub(crate) fn release(&self) {
        if fs::symlink_metadata(&self.path)
            .is_ok_and(|meta| meta.dev() == self.dev && meta.ino() == self.ino)
        {
            drop(fs::remove_file(&self.path));
        }
    }
}

/// 给会话目录 `dir` 绑门铃：照 [`Bell`] 的先后，第一处绑得上的。都不合用、都绑不上的交回 `None`。碰磁盘，在阻塞线程里调。
pub(crate) fn bind(bell: &Bell, dir: &Path) -> Option<Bound> {
    let dir = fs::canonicalize(dir).ok()?;
    let (name, uid) = (name(&dir), euid());
    // 第一处绑上了的，第二处的目录不建。
    let bound = runtime_spot(bell, &name, uid)
        .and_then(|path| take(&path))
        .or_else(|| own_spot(bell, &name, uid, true).and_then(|path| take(&path)));
    sweep(
        bell,
        uid,
        bound.as_ref().map(|bound| bound.placed.path.as_path()),
    );
    bound
}

/// 清两处目录里别的会话崩溃留下的门铃（`bind` 绑完调；`mine` 是自己刚绑的，不看）：运行时目录（合用的才看）里的
/// `litemiyu-<16 位十六进制>.sock`、`tmp` 下自己的目录（已经有的才看，不建）里的 `<16 位十六进制>.sock`。两处合起来最多看
/// [`SWEEP`] 个。碰磁盘，在阻塞线程里调。
fn sweep(bell: &Bell, uid: u32, mine: Option<&Path>) {
    let mut budget = SWEEP;
    if let Some(runtime) = bell.runtime.as_deref()
        && fs::metadata(runtime).is_ok_and(|meta| private(&meta, uid))
    {
        clear(runtime, "litemiyu-", uid, mine, &mut budget);
    }
    if let Some(own) = own_folder(&bell.tmp, uid, false) {
        clear(&own, "", uid, mine, &mut budget);
    }
}

/// 在目录 `dir` 里看名字是 `<prefix><16 位小写十六进制>.sock` 的，每看一个 `budget` 减一，到 0 停：是套接字（`lstat`：链接
/// 不算）、属主是 `uid`、连上去被拒（`ECONNREFUSED`：没人在听）的删掉。连得上的（有人在听）、连不上又不是被拒的（没有写
/// 权限这类，判不了）、名字不合的（别的进程换门铃时旁边那个 `<地址>.<进程号>`）都不动。删之前再 `lstat` 一次，还是看过的那
/// 个文件（设备号、inode）才删：看和删之间那个会话重开、换上了新的门铃，不删新的。删不掉的（别的进程先删了）不管。
pub(crate) fn clear(dir: &Path, prefix: &str, uid: u32, mine: Option<&Path>, budget: &mut usize) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if *budget == 0 {
            return;
        }
        let file = entry.file_name();
        let Some(hex) = file
            .to_str()
            .and_then(|file| file.strip_prefix(prefix))
            .and_then(|rest| rest.strip_suffix(".sock"))
        else {
            continue;
        };
        if hex.len() != 16 || !hex.bytes().all(|byte| HEX.contains(&byte)) {
            continue;
        }
        let path = entry.path();
        if mine == Some(path.as_path()) {
            continue;
        }
        *budget -= 1;
        let Ok(meta) = fs::symlink_metadata(&path) else {
            continue;
        };
        if !meta.file_type().is_socket() || meta.uid() != uid {
            continue;
        }
        // 每个用一个新的、没绑的套接字去连：连上了的不再拿去连别处（各家对连过的数据报套接字再连的规矩不全一样）。
        let refused = UnixDatagram::unbound().is_ok_and(|probe| {
            probe
                .connect(&path)
                .is_err_and(|error| error.kind() == std::io::ErrorKind::ConnectionRefused)
        });
        if refused
            && fs::symlink_metadata(&path)
                .is_ok_and(|now| now.dev() == meta.dev() && now.ino() == meta.ino())
        {
            drop(fs::remove_file(&path));
        }
    }
}

/// 敲会话目录 `dir` 的门铃：往收的一方可能绑的每一处敲一个字节，非阻塞，出什么错都不管。碰磁盘，在阻塞线程里调。
pub(crate) fn ring(bell: &Bell, dir: &Path) {
    let Ok(dir) = fs::canonicalize(dir) else {
        return;
    };
    let spots = spots(bell, &dir, euid());
    if spots.is_empty() {
        return;
    }
    let Ok(socket) = UnixDatagram::unbound() else {
        return;
    };
    if socket.set_nonblocking(true).is_err() {
        return;
    }
    for path in spots {
        drop(socket.send_to(b"!", path));
    }
}

/// 删会话目录 `dir`（还在的时候调，要照它算名字）的门铃：每一处是套接字、又没人在听的删掉（崩溃留下的；开着的窗口拿着锁，
/// 删会话的拿得到锁就没有）。碰磁盘，在阻塞线程里调。
pub(crate) fn forget(bell: &Bell, dir: &Path) {
    let Ok(dir) = fs::canonicalize(dir) else {
        return;
    };
    for path in spots(bell, &dir, euid()) {
        if stale(&path) {
            drop(fs::remove_file(&path));
        }
    }
}

/// 会话目录 `dir`（规整过的）的门铃可能在的几处，照先后：发的一方往这几处敲。第二处的目录还没有的不算（只看不建）。
pub(crate) fn spots(bell: &Bell, dir: &Path, uid: u32) -> Vec<PathBuf> {
    let name = name(dir);
    [
        runtime_spot(bell, &name, uid),
        own_spot(bell, &name, uid, false),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// 别的本机套接字放在哪（shell 集成里留下来的进程等同一个终端的下一句话）：照门铃那两处的规矩，第一处合用的——
/// `$XDG_RUNTIME_DIR/litemiyu-<name>.sock`（那个目录属主是自己、别人进不去），不合用的是 `<tmp>/litemiyu-<uid>/<name>.sock`
/// （`create` 的没有那个目录就照 0700 建，建了的、本来有的都核对）；两处都不合用、地址太长的是 `None`。能连这个套接字的
/// 只有进得了那个目录的人，也就是自己。
///
/// `name` 由调的一方给，不许是 16 位十六进制（那是门铃的名字，清旧门铃的会把没人听的删掉）。
pub fn private_spot(bell: &Bell, name: &str, create: bool) -> Option<PathBuf> {
    let uid = euid();
    runtime_spot(bell, name, uid).or_else(|| own_spot(bell, name, uid, create))
}

/// 第一处：`$XDG_RUNTIME_DIR` 是属主为 `uid`、别人进不去的目录（照它指的查），地址不太长。
fn runtime_spot(bell: &Bell, name: &str, uid: u32) -> Option<PathBuf> {
    let runtime = bell.runtime.as_deref()?;
    let meta = fs::metadata(runtime).ok()?;
    let path = runtime.join(format!("litemiyu-{name}.sock"));
    (private(&meta, uid) && path.as_os_str().len() <= MAX_PATH).then_some(path)
}

/// 第二处：`tmp` 下自己的目录（`create` 的没有就建），地址不太长。
fn own_spot(bell: &Bell, name: &str, uid: u32, create: bool) -> Option<PathBuf> {
    let path = own_folder(&bell.tmp, uid, create)?.join(format!("{name}.sock"));
    (path.as_os_str().len() <= MAX_PATH).then_some(path)
}

/// `tmp` 下自己的目录 `litemiyu-<uid>`：`create` 的没有就照 0700 建；照 `lstat` 核对是目录、属主是 `uid`、别人没有权限，
/// 不是的交回 `None`。
pub(crate) fn own_folder(tmp: &Path, uid: u32, create: bool) -> Option<PathBuf> {
    let folder = tmp.join(format!("litemiyu-{uid}"));
    if create {
        match fs::DirBuilder::new().mode(0o700).create(&folder) {
            Err(error) if error.kind() != std::io::ErrorKind::AlreadyExists => return None,
            _ => {}
        }
    }
    let meta = fs::symlink_metadata(&folder).ok()?;
    private(&meta, uid).then_some(folder)
}

/// 是目录（`lstat` 来的：链接不算）、属主是 `uid`、组和别人没有任何权限。
pub(crate) fn private(meta: &fs::Metadata, uid: u32) -> bool {
    meta.is_dir() && meta.uid() == uid && meta.permissions().mode() & 0o077 == 0
}

/// 会话目录 `dir`（规整过的）的门铃名字：整条路径 SHA-256 的前 8 个字节，16 位十六进制。
pub(crate) fn name(dir: &Path) -> String {
    let digest = Sha256::digest(dir.as_os_str().as_encoded_bytes());
    digest
        .iter()
        .take(8)
        .fold(String::with_capacity(16), |mut name, byte| {
            name.push(char::from(HEX[usize::from(byte >> 4)]));
            name.push(char::from(HEX[usize::from(byte & 0xf)]));
            name
        })
}

const HEX: &[u8; 16] = b"0123456789abcdef";

/// 这个进程的有效用户。
pub(crate) fn euid() -> u32 {
    rustix::process::geteuid().as_raw()
}

/// 换掉 `path` 上没人在听的旧套接字：先在旁边另绑一个，再改名盖过去。不先删再绑：旧文件一删，它的 inode 号
/// 文件系统可以马上发给新文件，[`Placed::release`] 照设备号、inode 认「还是不是自己绑的那个」就认错了
/// （2026-11-19 CI 上红在这）。改名盖过去的时候旧的还在，新的 inode 号一定和它不一样。旁边那个名字放不进
/// 地址（太长）的、改名不成的，退回先删再绑。
fn replace(path: &Path) -> Option<UnixDatagram> {
    let mut beside = path.as_os_str().to_owned();
    beside.push(format!(".{}", std::process::id()));
    let beside = PathBuf::from(beside);
    drop(fs::remove_file(&beside));
    if let Ok(socket) = UnixDatagram::bind(&beside) {
        if fs::rename(&beside, path).is_ok() {
            return Some(socket);
        }
        drop(socket);
        drop(fs::remove_file(&beside));
    }
    fs::remove_file(path).ok()?;
    UnixDatagram::bind(path).ok()
}

/// 在 `path` 上绑：已经有文件的，是没人在听的套接字才换掉重绑。绑好改成 0600、设成非阻塞，记下设备号、inode。
fn take(path: &Path) -> Option<Bound> {
    let socket = match UnixDatagram::bind(path) {
        Ok(socket) => socket,
        Err(error) if error.kind() == std::io::ErrorKind::AddrInUse => {
            if !stale(path) {
                return None;
            }
            replace(path)?
        }
        Err(_) => return None,
    };
    let placed = fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .and_then(|()| socket.set_nonblocking(true))
        .and_then(|()| fs::symlink_metadata(path))
        .map(|meta| Placed {
            path: path.to_path_buf(),
            dev: meta.dev(),
            ino: meta.ino(),
        });
    match placed {
        Ok(placed) => Some(Bound { socket, placed }),
        Err(_) => {
            drop(fs::remove_file(path));
            None
        }
    }
}

/// `path` 是套接字、又连不上（没人在听：崩溃留下的）。
fn stale(path: &Path) -> bool {
    fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_socket())
        && UnixDatagram::unbound().is_ok_and(|probe| probe.connect(path).is_err())
}

#[cfg(test)]
mod tests;
