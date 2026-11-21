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

/// 照编号在会话目录里找到的一个会话（`-s <编号>`，LM-25）：主会话、子会话都找得到，不经列会话的那一份。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    /// 会话编号。
    pub id: SessionId,
    /// 父会话：主会话没有。
    pub parent: Option<SessionId>,
    /// 这一刻打不开它的原因：拿着锁的那个会话。它自己有窗口开着的是它自己；它上面哪一层（父会话、父会话的父会话……）有窗口
    /// 开着的是那一层（子会话跟着主会话在那个窗口的进程里，那边随时可能叫起它、往它的日志里写）。都没有的是 `None`。
    pub held: Option<SessionId>,
}

/// 从子会话往上找父会话最多找这么多层：派子代理的深度上限远比它小，只为日志被改坏成一圈时停得下来。
const ANCESTORS: usize = 64;

/// 数据根 `paths` 下编号是 `id` 的会话（LM-25）：只读它日志的第一条认是不是会话、父会话是谁，再照文件锁看它自己、它上面各层
/// 有没有窗口开着。编号写得不对、没有这个目录、第一条不是造会话那一条的是 `None`。碰磁盘，在阻塞线程里调。
#[must_use]
pub fn find(paths: &Paths, id: &str) -> Option<Found> {
    let id = SessionId::parse(id).ok()?;
    let dir = paths.session(&id);
    let parent = parent_of(&dir)?;
    let held = match lock::held_by_other(&dir, LOCK) {
        true => Some(id.clone()),
        false => held_above(paths, parent.clone()),
    };
    Some(Found { id, parent, held })
}

/// 会话目录 `dir` 里那个会话的父会话：主会话是 `Some(None)`；不是会话的目录（第一条读不出、不是造会话那一条）是 `None`。
fn parent_of(dir: &Path) -> Option<Option<SessionId>> {
    match crate::store::log::first_event(dir).ok()?.body {
        lm_kernel::kernel::event::Body::SessionCreated(created) => Some(created.parent),
        _ => None,
    }
}

/// 从父会话 `parent` 往上各层里，头一个有窗口开着的（[`Found::held`]）。哪一层的目录不在了、读不出来的到那里为止。
fn held_above(paths: &Paths, parent: Option<SessionId>) -> Option<SessionId> {
    let mut next = parent;
    for _ in 0..ANCESTORS {
        let session = next?;
        let dir = paths.session(&session);
        if lock::held_by_other(&dir, LOCK) {
            return Some(session);
        }
        next = parent_of(&dir).flatten();
    }
    None
}

/// 会话目录 `dir` 里那个会话上面各层有没有窗口开着（界面直接载入子会话时查，`open/load.rs`）：主会话、不是会话的目录没有上面。
pub(crate) fn parent_open(paths: &Paths, dir: &Path) -> bool {
    parent_of(dir).is_some_and(|parent| held_above(paths, parent).is_some())
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
