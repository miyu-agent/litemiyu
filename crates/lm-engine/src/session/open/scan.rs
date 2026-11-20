//! 从日志里认出来的几样（施工 3-9 六补从 `load.rs` 挪来：那一份到了行数上限）：现在的快照、最近一次记下的工作目录；载入时
//! 边读边记的那几样（施工 V-15，[`Scan`]）。

use lm_kernel::kernel::event::{Body, Event};
use lm_kernel::kernel::facts::Environment;
use lm_kernel::kernel::id::ContentHash;

/// 现在的快照（施工 P-1 再补）：整份日志里最近一条带 `policy` 的 `session.policy_changed`，撤掉的回合里的也算（换快照不是
/// 对话的一部分）；没换过的没有，照 `session.created` 的。
pub(crate) fn current_policy(events: &[Event]) -> Option<&ContentHash> {
    events.iter().rev().find_map(|event| match &event.body {
        Body::PolicyChanged(changed) => changed.policy.as_ref(),
        _ => None,
    })
}

/// 日志里最后一次记下的工作目录、加进来的目录（施工 4-9 再补三上、5-10 上、9-7 上；V-2 三补从会话表挪来）：工作目录照列会话的
/// 同一个认法（`miyu_store::index::cwd`：最后一条带 `cwd` 的 `turn.started`、换工作区，没有就照 `session.created`）；加进来的
/// 目录照最后一条 `turn.started` 的，换工作区写了的照它，都没有的是空的。一路记的是 [`Place`]，这一份整份找的留给测试对照。
#[cfg(test)]
pub(crate) fn remembered(events: &[Event]) -> (Option<String>, Vec<String>) {
    let cwd = events
        .iter()
        .rev()
        .find_map(|event| cwd(event).map(str::to_string));
    let dirs = events
        .iter()
        .rev()
        .find_map(|event| match &event.body {
            Body::TurnStarted(started) => Some(started.dirs.clone()),
            Body::WorkspaceChanged(changed) => changed.dirs.clone(),
            _ => None,
        })
        .unwrap_or_default();
    (cwd, dirs)
}

/// 最后一次记下的工作目录、加进来的目录（同测试里整份找的 `remembered`），照先后一条条记（施工 V-39 中：分叉时边读边记，载入时 [`Scan`]
/// 也照它记）。
#[derive(Debug, Default)]
pub(crate) struct Place {
    /// 最后一次记下的工作目录。
    cwd: Option<String>,
    /// 最后一次记下的加进来的目录。
    dirs: Option<Vec<String>>,
}

impl Place {
    /// 记一条。
    pub(crate) fn note(&mut self, event: &Event) {
        if let Some(cwd) = cwd(event) {
            self.cwd = Some(cwd.to_string());
        }
        match &event.body {
            Body::TurnStarted(started) => self.dirs = Some(started.dirs.clone()),
            Body::WorkspaceChanged(changed) => {
                if let Some(dirs) = &changed.dirs {
                    self.dirs = Some(dirs.clone());
                }
            }
            _ => {}
        }
    }

    /// 最后一次记下的工作目录、加进来的目录。
    pub(crate) fn remembered(&self) -> (Option<String>, Vec<String>) {
        (self.cwd.clone(), self.dirs.clone().unwrap_or_default())
    }
}

/// 载入时边读边记的几样（施工 V-15：日志一块块读，读过的放掉，从整份里找的改成一路记）：每一样同上面整份找的那个。
#[derive(Debug, Default)]
pub(super) struct Scan {
    /// 最近一条带 `policy` 的 `session.policy_changed` 的快照（同 [`current_policy`]）。
    pub(super) policy: Option<ContentHash>,
    /// 最后一次记下的工作目录、加进来的目录（同测试里整份找的 `remembered`）。
    place: Place,
    /// 她看过的文件（`effects::seen_in`）。
    pub(super) seen: crate::session::effects::SeenScan,
    /// 派出去的任务（`Roster::from_events`）。
    pub(super) roster: crate::session::jobs::Roster,
    /// 累计的用量和计数（`Tally::of`）。
    pub(super) tally: crate::session::current::Tally,
    /// 上一次关掉时循环是不是还开着（L5 四，`looping::Trail`）。
    pub(super) looped: crate::session::looping::Trail,
    /// 最后一条的时刻。
    pub(super) last: Option<lm_kernel::kernel::time::Timestamp>,
    /// 读了几条。
    pub(super) count: usize,
}

impl Scan {
    /// 读完了，要照哪一份快照：`now` 是照着取了策略的那一份；最近一次换的（`policy`）和它不一样的交回换成的，一样的、没换过的
    /// （照 `created`）没有。
    pub(super) fn switched(&self, now: &ContentHash, created: &ContentHash) -> Option<ContentHash> {
        let last = self.policy.as_ref().unwrap_or(created);
        (last != now).then(|| last.clone())
    }

    /// 记下照先后的一块。
    pub(super) fn note(&mut self, events: &[Event]) {
        for event in events {
            self.place.note(event);
            if let Body::PolicyChanged(changed) = &event.body
                && let Some(policy) = &changed.policy
            {
                self.policy = Some(policy.clone());
            }
            self.seen.note(event);
            self.roster.note(event);
            self.looped.note(event);
        }
        self.tally.add(events);
        self.last = events.last().map(|event| event.at).or(self.last);
        self.count += events.len();
    }

    /// 最后一次记下的工作目录、加进来的目录（同测试里整份找的 `remembered`）。
    pub(super) fn remembered(&self) -> (Option<String>, Vec<String>) {
        self.place.remembered()
    }
}

/// 载入交回的（施工 V-2 三补）：会话这时在哪干活。
#[derive(Debug)]
pub struct Placed {
    /// 在哪干活：照日志里最后一次记下的挑的，会话表照它记，不用再问 actor（施工 V-2 三补）。
    pub environment: Environment,
}

/// 一条事件记下的工作目录（Miyu 的 `miyu_store::index::cwd`，L3 二挪来：LiteMiyu 没有会话索引）：回合开始的、造会话的、换了
/// 工作区的那一条。
pub(crate) fn cwd(event: &Event) -> Option<&str> {
    match &event.body {
        Body::TurnStarted(started) => started.cwd.as_deref(),
        Body::SessionCreated(created) => created.cwd.as_deref(),
        Body::WorkspaceChanged(changed) => Some(changed.cwd.as_str()),
        _ => None,
    }
}

#[cfg(test)]
mod tests;
