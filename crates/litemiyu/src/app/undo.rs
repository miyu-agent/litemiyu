//! `/undo`（Miyu 终端 `app/takeback.rs` 的 `undo`、`app/updates.rs` 收撤销回应的那一段）：有排着队的先退回最新的那一条（一次
//! 一条），没有的才撤回上一轮；撤销成了，那一句放回输入框（框里空着才放，全选着，回车就重发）。
//!
//! 撤销成了的回应（[`Update::Undone`]）和撤掉那几轮的推送走两条路，谁先到不一定：发的时候记下画过几行说明，回应到了比一比，
//! 这一次的那一行还没画的，等推送来了再放。

use super::App;
use crate::core::{Command, Update};
use crate::transcript::Kind;

impl App {
    /// `/undo`。
    pub(super) fn undo(&mut self) {
        let command = if self.talk.transcript.queued().next().is_some() {
            Command::Withdraw(None)
        } else {
            self.talk.undo_from = Some(self.talk.transcript.undo_count());
            Command::Revert
        };
        self.link.core.send(command);
    }

    /// 撤销成了（[`Update::Undone`]）：放回那一句；说明那一行还没画的记着等推送。恢复成了：撤销时放回来的那句还没动过的
    /// 收回去（Miyu `app/updates.rs`）。别的交回假，照常办。
    pub(super) fn undone(&mut self, update: &Update) -> bool {
        let undid = match update {
            Update::Undone { restore: true, .. } => {
                self.typing.input.take_back();
                return true;
            }
            Update::Undone {
                restore: false,
                undid,
            } => *undid,
            _ => return false,
        };
        let from = self.talk.undo_from.take().unwrap_or_default();
        let drawn = self.talk.transcript.undo_count() > from;
        // 撤掉了几次压缩、清空，停了几个任务：写进这一次的那行说明，还没画的等推送来了画的时候写（`transcript/undo.rs`）。
        self.talk.transcript.undid(undid, drawn);
        if drawn {
            self.put_back_undone();
        } else {
            self.talk.undo_put_back = true;
        }
        true
    }

    /// 撤掉那几轮的推送办过了：回应先到的，这时放回那一句。
    pub(super) fn after_reverted(&mut self) {
        if std::mem::take(&mut self.talk.undo_put_back) {
            self.put_back_undone();
        }
    }

    /// 照最近那一行撤销说明放回你说的那句。
    fn put_back_undone(&mut self) {
        let line = self
            .talk
            .transcript
            .entries
            .iter()
            .rev()
            .find(|e| e.kind == Kind::Undo);
        if let Some((said, chips)) = line
            .map(|e| (e.text.clone(), e.pasted.clone()))
            .filter(|(said, _)| !said.is_empty())
        {
            let images = super::paste::pasted(chips);
            self.typing.input.put_back(&said, &images);
        }
    }
}
