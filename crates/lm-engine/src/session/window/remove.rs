//! 删一个主会话（`/delete`、`/sessions` 里 `Ctrl+D`）：拿着会话的锁直接删掉整个会话目录（LM-2 砍了
//! 回收站，删了找不回）。别的窗口开着的不删。删目录以前连它的门铃（LM-4）一起删：开着时关窗会删，这里删的是崩溃留下的。

use lm_kernel::kernel::id::SessionId;

use super::lock::{Claim, Claimed};
use crate::store::paths::Paths;

/// 删不成。
#[derive(Debug)]
pub enum NotRemoved {
    /// 没有这个会话（编号写法不对、目录没有）。
    Missing,
    /// 别的窗口开着它。
    Open,
    /// 锁文件开不了、目录删不掉：原话。
    Failed(String),
}

impl std::fmt::Display for NotRemoved {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NotRemoved::Missing => f.write_str("no such session"),
            NotRemoved::Open => f.write_str("the session is open in another window"),
            NotRemoved::Failed(why) => f.write_str(why),
        }
    }
}

/// 删掉数据根 `paths` 下的主会话 `id`：拿着它的锁删整个目录，删完放下（锁文件在目录里，跟着删了；放下的是已经打开的那一份）。
/// 开着它的窗口拿着锁，这里拿不到就不删；删的这一瞬也没有窗口开得了它。碰磁盘，在阻塞线程里调。
///
/// # Errors
///
/// 见 [`NotRemoved`]。删到一半出错的（权限）照原话报，删掉的那几个文件回不来。
pub fn remove(paths: &Paths, id: &str) -> Result<(), NotRemoved> {
    let id = SessionId::parse(id).map_err(|_| NotRemoved::Missing)?;
    let dir = paths.sessions().join(id.as_str());
    if !dir.is_dir() {
        return Err(NotRemoved::Missing);
    }
    let claim = Claim::take(&dir).map_err(|why| match why {
        Claimed::Elsewhere => NotRemoved::Open,
        Claimed::Disk(error) => NotRemoved::Failed(error.to_string()),
    })?;
    // 门铃的名字照会话目录算：目录还在的时候删。
    crate::session::peers::forget(&paths.bell, &dir);
    let removed = std::fs::remove_dir_all(&dir).map_err(|why| NotRemoved::Failed(why.to_string()));
    drop(claim);
    removed
}
