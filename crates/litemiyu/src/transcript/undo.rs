//! 撤销、恢复（Miyu 终端 `transcript/undo.rs`、`pages.rs` 的 `decided`，蓝图 `tui.md`「正文」第 5、6 条）：撤掉的几轮藏起来、
//! 恢复的显示回来；撤销画一行说明，恢复去掉它。看着撤的和补发的都照事件来（Miyu 看着撤的照回应画，LiteMiyu 没有协议的回应，
//! 同 Miyu 补发的样子）。改过的文件内核不改回（Miyu 核心 U-1），说明里不列文件。
//!
//! 只留几屏（`pieces.rs`）：放掉的段读回来时是在一份草稿里照那一截日志办的，后来撤掉、恢复的那几轮、去掉的说明在草稿里不知道；
//! 这里记着每一轮最后是藏着还是露着、去掉了哪几行说明，放回去时照它改（同 Miyu 读更早一页时的 `older.undone`）。
//!
//! 撤销的回应带着撤掉了几次压缩、清空，停了几个任务（Miyu 回应的 `Report`）：回应和推送谁先到不一定，回应先到的记着，那一行
//! 说明画出来时写上；照说明那条 `turn.reverted` 的序号记着，读回来的段里照旧写（日志里没有这几个数）。

use std::collections::{HashMap, HashSet};

use lm_kernel::kernel::session::Undid;

use super::{Entry, Kind, Transcript};

/// 记着的撤销、恢复。
#[derive(Debug, Default)]
pub(super) struct Undos {
    /// 撤掉、恢复过的每一轮最后是藏着（真）还是露着。
    decided: HashMap<u64, bool>,
    /// 去掉了的说明（它那条 `turn.reverted` 的序号）。
    gone: HashSet<u64>,
    /// 每一行说明写的那一句（照它那条 `turn.reverted` 的序号）：读回来的一段从中间开始、撤的头一轮不在那一段里时，照这里的写
    /// （同全留着时画的一样）。
    said: HashMap<u64, String>,
    /// 画过几行说明（`app/undo.rs` 比它认这一次的那一行画了没有）。
    lines: u64,
    /// 最近一次撤销是哪个命令撤的：重做重发的话带着同一个（[`Transcript::resent`]）。
    cause: Option<String>,
    /// 撤销的回应说撤掉了什么（照说明那条 `turn.reverted` 的序号）：只记一样都不是没有的。
    undid: HashMap<u64, Undid>,
    /// 回应先到、那一行说明还没画：画的时候写上。
    pending: Option<Undid>,
}

impl Transcript {
    /// 撤掉了这几轮（`turn.reverted`，序号 `seq`）：藏起来，画一行说明。
    pub(super) fn reverted(&mut self, turns: &[u64], seq: u64, cause: Option<String>) {
        self.undos.cause = cause;
        self.hide(turns, true);
        self.undos.lines += 1;
        let said = self
            .entries
            .iter()
            .find(|e| e.kind == Kind::User && e.turn.is_some_and(|t| turns.contains(&t)))
            .map(|e| e.text.clone())
            .unwrap_or_default();
        let said = self.undos.said.entry(seq).or_insert(said).clone();
        self.note(Kind::Undo, said);
        if let Some(undid) = self.undos.pending.take().filter(|u| !u.none()) {
            self.undos.undid.insert(seq, undid);
        }
        let undid = self.undos.undid.get(&seq).copied();
        if let Some(last) = self.entries.last_mut() {
            last.seq = Some(seq);
            last.undo = undid.map(Box::new);
        }
    }

    /// 撤销的回应到了（`app/undo.rs`）：撤掉了几次压缩、清空，停了几个任务。这一次的那行说明画了（`drawn`）的写进最近那一行，
    /// 还没画的记着，推送来了画的时候写（[`Transcript::reverted`]）。
    pub fn undid(&mut self, undid: Undid, drawn: bool) {
        if !drawn {
            self.undos.pending = Some(undid);
            return;
        }
        if undid.none() {
            return;
        }
        let line = self.entries.iter_mut().rev().find(|e| e.kind == Kind::Undo);
        if let Some(line) = line {
            line.undo = Some(Box::new(undid));
            if let Some(seq) = line.seq {
                self.undos.undid.insert(seq, undid);
            }
        }
    }

    /// 恢复了这几轮（`turn.unreverted`）：显示回来，藏起最近的那一行撤销说明（Miyu 收恢复回应时的 `undo_gone`；LiteMiyu
    /// 照事件办，看着恢复的和补发的一个样），记下去掉的是哪一行，读回来的段里也不放回去。
    pub(super) fn unreverted(&mut self, turns: &[u64]) {
        self.hide(turns, false);
        self.undos.cause = None;
        self.drop_undo_line();
    }

    /// 一句你说的话是命令 `cause` 记的：和最近一次撤销是同一个命令的，是重做重发的那句，撤销说明去掉（重做不留撤销说明，
    /// Miyu 终端 `core/replay.rs`）；撤掉的那一轮照旧藏着。看着重做的、补发的、只留几屏读回来的一个样。
    pub(super) fn resent(&mut self, cause: &str) {
        if self.undos.cause.as_deref() == Some(cause) {
            self.undos.cause = None;
            self.drop_undo_line();
        }
    }

    /// 藏起最近的那一行撤销说明，记下去掉的是哪一行，读回来的段里也不放回去。
    fn drop_undo_line(&mut self) {
        // 藏起来、不挪掉：挪掉一条会动只留几屏记着的下标（`pieces.rs`），读回来的段对不上。
        let line = self
            .entries
            .iter_mut()
            .rev()
            .find(|e| e.kind == Kind::Undo && !e.hidden);
        if let Some(line) = line {
            line.hidden = true;
            if let Some(seq) = line.seq {
                self.undos.gone.insert(seq);
            }
        }
    }

    /// 撤掉、恢复的那几轮的条目藏起来、显示回来，记下最后是哪样。别处来的话撤销不带走它（「别处来的话」第 3 条）。
    pub(super) fn hide(&mut self, turns: &[u64], hidden: bool) {
        for &turn in turns {
            self.undos.decided.insert(turn, hidden);
        }
        for entry in self.entries.iter_mut().filter(|e| e.from.is_none()) {
            if entry.turn.is_some_and(|t| turns.contains(&t)) {
                entry.hidden = hidden;
            }
        }
    }

    /// 画过几行撤销说明。
    pub fn undo_count(&self) -> u64 {
        self.undos.lines
    }

    /// 读回来的一段放回去以前（`pieces.rs` 的 `restore`）：后来撤掉、恢复的照最后的样子藏、露，后来去掉的说明不放回去。
    pub(super) fn settle_undone(&self, entries: &mut Vec<Entry>) {
        if self.undos.decided.is_empty()
            && self.undos.gone.is_empty()
            && self.undos.undid.is_empty()
        {
            return;
        }
        entries.retain(|e| {
            e.kind != Kind::Undo || !e.seq.is_some_and(|s| self.undos.gone.contains(&s))
        });
        for entry in entries.iter_mut().filter(|e| e.kind == Kind::Undo) {
            if let Some(said) = entry.seq.and_then(|s| self.undos.said.get(&s)) {
                entry.text.clone_from(said);
            }
            if let Some(&undid) = entry.seq.and_then(|s| self.undos.undid.get(&s)) {
                entry.undo = Some(Box::new(undid));
            }
        }
        for entry in entries.iter_mut().filter(|e| e.from.is_none()) {
            if let Some(&hidden) = entry.turn.and_then(|t| self.undos.decided.get(&t)) {
                entry.hidden = hidden;
            }
        }
    }
}
