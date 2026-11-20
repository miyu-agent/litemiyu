//! 重做、编辑上一句要的（Miyu 终端 `transcript/redo.rs`，蓝图 `tui.md`「斜杠命令」`/redo`、「输入框」第 13 条）：开最后一轮的
//! 那一句是哪条；重做发出去时先藏掉最后一轮、没发出去再显示回来。说的话没发出去时撤掉先画上的那句在 `queue.rs`。

use super::{Entry, Kind, Transcript};

impl Transcript {
    /// 开最后一轮（没藏着的）的那一句你说的话；最后一轮不是你说的话开的（别处来的话、到点的循环）是 `None`。
    pub fn last_said(&self) -> Option<&Entry> {
        let shown = || self.entries.iter().filter(|e| !e.hidden);
        let last = shown().filter_map(|e| e.turn).max()?;
        // 别处来的话开的那一轮不算（「别处来的话」第 3 条）。
        shown().find(|e| e.kind == Kind::User && e.turn == Some(last) && e.from.is_none())
    }

    /// 重做发出去时先在界面上藏掉最后一轮：重发的那句落在它原来的位置，不先在底下闪一下（会话推来的 `turn.reverted` 照样
    /// 再藏一次）。交回藏掉的是第几轮；最后一轮不是你说的话开的是 `None`。
    pub fn hide_last_turn(&mut self) -> Option<u64> {
        let turn = self.last_said()?.turn?;
        self.hide(&[turn], true);
        Some(turn)
    }

    /// 重做没发出去：先藏掉的那一轮显示回来。
    pub fn show_turn(&mut self, turn: u64) {
        self.hide(&[turn], false);
    }
}
