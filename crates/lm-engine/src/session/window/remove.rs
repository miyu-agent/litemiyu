//! 删一个主会话（`/delete`、`/sessions` 里 `Ctrl+D`）：拿着会话的锁直接删掉整个会话目录（LM-2 砍了
//! 回收站，删了找不回）。别的窗口开着的不删。删目录以前连它的门铃（LM-4）一起删：开着时关窗会删，这里删的是崩溃留下的。
//!
//! 它派出去的子会话一起删（LM-18，照 Miyu `session.delete`「它派出去的子会话一起删」）：照每个会话日志的第一条
//! （`session.created` 的 `parent`）一层层往下认，从最深的删起，它自己最后：半路出错的，它还在原处、列得出来，再删一次接着删完。
//! fork 子代理的底子在父会话的日志里，父会话没了它也读不回来，所以不能留。原来只删主会话自己的目录，子会话的目录留在数据目录里。
//!
//! 它派出去的子会话有窗口直接开着的（`-s <子会话编号>`，LM-25）不删，照「别的窗口开着」报：那个窗口还在用子会话的目录。

use lm_kernel::kernel::id::SessionId;

use super::lock::{self, Claim, Claimed};
use crate::store::paths::Paths;

/// 删不成。
#[derive(Debug)]
pub enum NotRemoved {
    /// 没有这个会话（编号写法不对、目录没有）。
    Missing,
    /// 别的窗口开着它，或者开着它派出去的某个子会话。
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
    let family = family(paths, &id);
    // 它派出去的哪个子会话正被窗口直接开着（`-s <子会话编号>`，LM-25）：那个窗口还在往子会话的目录里写，一个都不删。
    if family
        .iter()
        .any(|child| lock::held_by_other(&paths.session(child), lock::LOCK))
    {
        return Err(NotRemoved::Open);
    }
    let removed = family
        .iter()
        .rev()
        .try_for_each(
            |child| match std::fs::remove_dir_all(paths.session(child)) {
                Err(why) if why.kind() != std::io::ErrorKind::NotFound => {
                    Err(NotRemoved::Failed(why.to_string()))
                }
                _ => Ok(()),
            },
        )
        .and_then(|()| {
            // 门铃的名字照会话目录算：目录还在的时候删。
            crate::session::peers::forget(&paths.bell, &dir);
            std::fs::remove_dir_all(&dir).map_err(|why| NotRemoved::Failed(why.to_string()))
        });
    drop(claim);
    removed
}

/// 会话 `id` 派出去的子会话，连子会话再派的，照层数从浅到深：数据目录里每个会话日志的第一条只读一次，认 `parent`。读不了的
/// 目录、第一条读不懂的不算。
fn family(paths: &Paths, id: &SessionId) -> Vec<SessionId> {
    let Ok(entries) = std::fs::read_dir(paths.sessions()) else {
        return Vec::new();
    };
    let parents: Vec<(SessionId, SessionId)> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let child = SessionId::parse(entry.file_name().to_str()?).ok()?;
            let first = crate::store::log::first_event(&entry.path()).ok()?;
            match first.body {
                lm_kernel::kernel::event::Body::SessionCreated(created) => {
                    Some((child, created.parent?))
                }
                _ => None,
            }
        })
        .collect();
    let mut family: Vec<SessionId> = Vec::new();
    let mut layer = vec![id.clone()];
    while !layer.is_empty() {
        let next: Vec<SessionId> = parents
            .iter()
            .filter(|(child, parent)| layer.contains(parent) && !family.contains(child))
            .map(|(child, _)| child.clone())
            .collect();
        family.extend(next.iter().cloned());
        layer = next;
    }
    family
}
