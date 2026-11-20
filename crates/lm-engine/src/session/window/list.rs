//! 列磁盘上的主会话（设计第五节第 8 条、第 12 条「最近会话」，L7 一）：扫会话目录，每个会话读一份小概要（`summary.rs`），开没开、
//! 忙不忙照文件锁看（`lock.rs`）。不用先开会话：界面的「最近会话」、会话切换、`sessions` 工具都调 [`list`]。用到才扫，扫完不留。

use std::io;
use std::path::Path;

use lm_kernel::kernel::id::SessionId;
use lm_kernel::kernel::time::Timestamp;

use super::lock::{self, BUSY, LOCK};
use super::summary;
use crate::store::paths::Paths;

/// 列出来的一个主会话。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Listed {
    /// 会话编号。
    pub id: SessionId,
    /// 标题：没起的是空的。
    pub title: String,
    /// 工作目录（头报来的写法）。
    pub cwd: String,
    /// 最后一次动静的时刻：概要里记的；开着的会话照日志最后一段的修改时刻，晚的那个（概要只在第一轮结束、标题变了、会话
    /// 结束时写）。
    pub last_active: Timestamp,
    /// 开过几轮。
    pub turns: u64,
    /// 预览：第一句话的第一行，至多 50 个字（照 Miyu 核心会话表的 `preview`）；还没说过有字的话的、引擎 十以前写的概要里没有
    /// 这一格的是空的。界面里没起名的会话照它写。
    pub preview: String,
    /// 有窗口开着它。
    pub open: bool,
    /// 开着它的窗口这时有回合在进行。
    pub busy: bool,
}

/// 列数据根 `paths` 下的主会话，最近有动静的在前（一样的照编号倒着排）。这个进程自己开着的也列（界面的「最近会话」照
/// Miyu 连当前这个一起照先后排，界面线第二十一趟）：开着的照日志最后一段的修改时刻排，打开一个会话不写日志，不挪它的位置，
/// 在里面说了话才挪到前面。会话目录还没有的是空的；子会话、读不出概要也算不出
/// 来的（日志坏了、不是会话的目录）不列。碰磁盘，在阻塞线程里调。
///
/// # Errors
///
/// 放会话的目录在但读不了。
pub fn list(paths: &Paths) -> io::Result<Vec<Listed>> {
    under(&paths.sessions())
}

/// 同 [`list`]，照放会话的目录 `sessions` 扫。
pub(crate) fn under(sessions: &Path) -> io::Result<Vec<Listed>> {
    let entries = match std::fs::read_dir(sessions) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut listed = Vec::new();
    for entry in entries {
        let entry = entry?;
        let Some(id) = entry
            .file_name()
            .to_str()
            .and_then(|name| SessionId::parse(name).ok())
        else {
            continue;
        };
        let dir = entry.path();
        if let Some(one) = one(&dir, id) {
            listed.push(one);
        }
    }
    listed.sort_by(|a, b| (b.last_active, b.id.as_str()).cmp(&(a.last_active, a.id.as_str())));
    Ok(listed)
}

/// 会话目录 `dir` 里的那一个：有概要照概要，没有的照日志算（子会话算出来是没有）。
fn one(dir: &Path, id: SessionId) -> Option<Listed> {
    let summary = match summary::read(dir) {
        Some(summary) => summary,
        // 没有概要的先只读第一条：子会话不往下读。
        None => match crate::store::log::first_event(dir).ok()?.body {
            lm_kernel::kernel::event::Body::SessionCreated(created) if created.parent.is_none() => {
                summary::from_log(dir)?
            }
            _ => return None,
        },
    };
    let open = lock::held_by_other(dir, LOCK);
    let last_active = match open {
        true => newest_segment(dir).map_or(summary.last_active, |at| at.max(summary.last_active)),
        false => summary.last_active,
    };
    Some(Listed {
        id,
        title: summary.title,
        cwd: summary.cwd,
        last_active,
        turns: summary.turns,
        preview: summary.preview.unwrap_or_default(),
        open,
        busy: open && lock::held_by_other(dir, BUSY),
    })
}

/// 日志最后一段的修改时刻：段名是补零的序号，名字最大的就是最后一段。
fn newest_segment(dir: &Path) -> Option<Timestamp> {
    let newest = std::fs::read_dir(dir)
        .ok()?
        .flatten()
        .filter(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.ends_with(".jsonl"))
        })
        .max_by_key(std::fs::DirEntry::file_name)?;
    let modified = newest.metadata().ok()?.modified().ok()?;
    let ms = modified
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_millis();
    Timestamp::from_unix_millis(i64::try_from(ms).ok()?)
}
