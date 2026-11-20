//! 造不成、载入不了的几种（施工 3-8 七补从 `open.rs` 挪出来，那边放不下了）。

use std::fmt;
use std::io;
use std::path::PathBuf;

use crate::store::blob::BlobError;
use crate::store::log::{BrokenAt, OpenError, abandon};
use lm_kernel::kernel::session::LoadError as Broken;
use lm_kernel::policy::{BuildError, SnapshotError};

/// 造不成。
#[derive(Debug)]
pub enum CreateError {
    /// 随核心附带的字造不出策略：安装坏了。
    Policy(BuildError),
    /// 存不下快照、建不了会话目录和日志。
    Disk(io::Error),
    /// 造会话那一条没落盘，会话就停了。
    Stopped,
}

/// 载入不了。
#[derive(Debug)]
pub enum LoadError {
    /// 日志打不开：没有这个会话，或者日志坏了。
    Log(OpenError),
    /// 日志里没有造会话那一条。
    NotCreated,
    /// 策略快照取不出来。
    Blob(BlobError),
    /// 策略快照读不懂。
    Snapshot(SnapshotError),
    /// 快照造不出策略。
    Policy(BuildError),
    /// 内核载入不了：日志过不了账本。
    Kernel(Broken),
    /// 别的窗口开着它（L7 一，`crate::session::window`）。
    Taken,
}

impl fmt::Display for CreateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CreateError::Policy(error) => write!(f, "policy not built: {error}"),
            CreateError::Disk(error) => write!(f, "session not created on disk: {error}"),
            CreateError::Stopped => write!(f, "session.created not stored; the session stopped"),
        }
    }
}

impl std::error::Error for CreateError {}

impl LoadError {
    /// 这一刻读不了（Miyu 施工 YC-2）：读日志、策略快照时读写出错（打开文件数用光、权限、磁盘出错），日志本身没坏，过一会儿可以
    /// 再试。界面照它说「会话暂时无法读取，请稍后重试」，不说成损坏。
    #[must_use]
    pub fn passing(&self) -> bool {
        matches!(
            self,
            LoadError::Log(OpenError::Io(_)) | LoadError::Blob(BlobError::Io(_))
        )
    }

    /// 日志中间坏了一行、坏行前面还有的（Miyu 施工 YC-6）：坏在哪。会话载入不了、不能接着聊，要继续新开一个会话；日志一个字节
    /// 都不动（2026-11-10 Miyu 项目主人定：只读打开，不给修）。坏行以前的照样读得到（[`crate::store::log::read_range`] 这些读到
    /// `seq` 以前）。坏在第一条的没有：没有能看的，照损坏说。
    #[must_use]
    pub fn broken_at(&self) -> Option<BrokenAt> {
        match self {
            LoadError::Log(error) => error.broken_at().filter(|at| at.seq.get() > 1),
            _ => None,
        }
    }
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // 一时读不了的、坏了一行的照 Miyu 给人看的说法（`refusal/message.rs` 的 `session_unavailable`、`session_read_only`），
        // 后面带上原因。
        if self.passing() {
            let cause = match self {
                LoadError::Log(error) => error.to_string(),
                LoadError::Blob(error) => error.to_string(),
                _ => String::new(),
            };
            return write!(
                f,
                "Session can't be read right now. Try again later. ({cause})"
            );
        }
        if let Some(at) = self.broken_at() {
            let line = at.line;
            return write!(
                f,
                "The session log is broken at line {line}; only the part before it can be viewed and the session can't continue. Start a new session to go on. ({})",
                at.segment
            );
        }
        match self {
            LoadError::Log(error) => write!(f, "session log not opened: {error}"),
            LoadError::NotCreated => write!(f, "the session log has no session.created"),
            LoadError::Blob(error) => write!(f, "policy snapshot not fetched: {error}"),
            LoadError::Snapshot(error) => write!(f, "policy snapshot not understood: {error}"),
            LoadError::Policy(error) => write!(f, "policy not built from the snapshot: {error}"),
            LoadError::Kernel(error) => write!(f, "not loaded: {error}"),
            LoadError::Taken => f.write_str("the session is open in another window"),
        }
    }
}

impl std::error::Error for LoadError {}

/// 造会话那一条没落盘：只剩空的第一段的会话目录 `dir` 删掉；快照的 blob 留着，按内容存，别的会话可能也在用（施工 4-9 再补四
/// 下：原来都留在磁盘上）。删不掉的记一行。
pub(super) async fn abandoned(dir: PathBuf) {
    drop(crate::session::blocking::blocking(move || abandon(&dir)).await);
}

#[cfg(test)]
mod tests;
