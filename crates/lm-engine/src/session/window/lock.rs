//! 主会话的两把文件锁（设计第五节第 2 条，L7 一）：开着它的窗口拿着会话目录里的 `lock`（[`LOCK`]），同一个会话不能被两个
//! 窗口同时开；有回合在进行时另拿着 `busy`（[`BUSY`]），别的窗口列会话时照它写忙不忙。都是 `flock`（标准库的 `File::try_lock`），
//! 进程没了（关掉、被杀）锁跟着没，不留陈旧的锁。
//!
//! 别的窗口怎么知道开没开着（[`held_by_other`]）：试着拿一下共享锁，拿不到就是有人拿着排他的那一把，拿到了当场放下。没用
//! `/proc/locks`：它一页一页读，两次读之间别的锁放下了会漏行，开着的会话会被看成没开。试的那一下要是正赶上另一个窗口在开这个
//! 会话，那个窗口的排他锁会拿不到：开会话的一方照这个让一让（[`Claim::take`]，几毫秒以内再试），试的一方拿着共享锁只有一次系统
//! 调用那么久。

use std::fs::{File, OpenOptions, TryLockError};
use std::io;
use std::path::Path;
use std::time::{Duration, Instant};

/// 开着这个会话的窗口拿着的锁文件。
pub(crate) const LOCK: &str = "lock";

/// 有回合在进行时拿着的锁文件。
pub(crate) const BUSY: &str = "busy";

/// 开会话时排他锁被占着，让多久再认定别的窗口开着：别的窗口试锁只占一次系统调用那么久。
const YIELD: Duration = Duration::from_millis(5);

/// 拿不到：别的窗口开着它，或者锁文件开不了。
#[derive(Debug)]
pub enum Claimed {
    /// 别的窗口（别的进程，或者这个进程里另一个把手）开着它。
    Elsewhere,
    /// 锁文件建不了、开不了。
    Disk(io::Error),
}

impl std::fmt::Display for Claimed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Claimed::Elsewhere => f.write_str("the session is open in another window"),
            Claimed::Disk(error) => write!(f, "the session lock not taken: {error}"),
        }
    }
}

impl std::error::Error for Claimed {}

/// 一个窗口对一个主会话的占有：拿着 `lock`，回合在进行时拿着 `busy`。放下（`drop`）就都放了。
#[derive(Debug)]
pub(crate) struct Claim {
    lock: File,
    busy: File,
    busy_held: bool,
}

impl Claim {
    /// 在会话目录 `dir` 里拿锁：已经有人拿着的交回 [`Claimed::Elsewhere`]。碰磁盘，在阻塞线程里调。
    pub(crate) fn take(dir: &Path) -> Result<Claim, Claimed> {
        let lock = open(&dir.join(LOCK)).map_err(Claimed::Disk)?;
        // 别的窗口列会话时试的那一下只拿着共享锁一瞬：拿不到的让一让再试，过了 [`YIELD`] 还拿不到才是真有窗口开着。
        let began = Instant::now();
        loop {
            match lock.try_lock() {
                Ok(()) => break,
                Err(TryLockError::WouldBlock) if began.elapsed() < YIELD => {
                    std::thread::yield_now()
                }
                Err(TryLockError::WouldBlock) => return Err(Claimed::Elsewhere),
                Err(TryLockError::Error(error)) => return Err(Claimed::Disk(error)),
            }
        }
        let busy = open(&dir.join(BUSY)).map_err(Claimed::Disk)?;
        Ok(Claim {
            lock,
            busy,
            busy_held: false,
        })
    }

    /// 回合在不在进行：变了才换 `busy` 那把锁（一次系统调用，不开文件）。拿不到、放不掉的记一行，照旧往下走：只影响别的窗口
    /// 列会话时写的忙不忙。
    pub(crate) fn set_busy(&mut self, busy: bool) {
        if busy == self.busy_held {
            return;
        }
        let done = match busy {
            true => self.busy.try_lock().map_err(|error| match error {
                // 别的窗口正在试：下一批再拿。
                TryLockError::WouldBlock => None,
                TryLockError::Error(error) => Some(error.to_string()),
            }),
            false => self.busy.unlock().map_err(|error| Some(error.to_string())),
        };
        match done {
            Ok(()) => self.busy_held = busy,
            Err(None) => {}
            Err(Some(_)) => {}
        }
    }
}

/// 放下：先明说放开 `lock`（关掉文件本来也会放），`busy` 跟着关掉放开。
impl Drop for Claim {
    fn drop(&mut self) {
        drop(self.lock.unlock());
    }
}

/// 开锁文件：没有就建。
fn open(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(path)
}

/// 会话目录 `dir` 里叫 `name` 的锁文件这一刻有没有人拿着排他的那一把：试着拿共享锁，拿到了当场放下。没有这个文件的、开不了的
/// 算没有。碰磁盘，在阻塞线程里调。
pub(crate) fn held_by_other(dir: &Path, name: &str) -> bool {
    File::open(dir.join(name))
        .is_ok_and(|file| matches!(file.try_lock_shared(), Err(TryLockError::WouldBlock)))
}
