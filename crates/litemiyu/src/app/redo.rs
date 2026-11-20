//! 重做、编辑上一句（Miyu 终端 `app/redo.rs`，蓝图 `tui.md`「斜杠命令」`/redo`、「输入框」第 13 条，内核的 `Redo`）：先在界面上
//! 查得出来的（在回答、最后一轮不是你说的话开的）当场提示；发出去的同时先藏掉重来的那一轮、把重发的那句画在它的位置，
//! 会话推来的序号配给它；会话拒了，那句撤掉、那一轮显示回来、编辑的字放回输入框（`updates.rs` 的 `take_back_unsent`）。
//!
//! 和 Miyu 不一样的：LiteMiyu 没有附件，改过的只带字；没有协议一行的上限，不用转成文字附件。

use super::{App, Unsent};
use crate::core::Command;
use crate::input::Draft;
use crate::transcript::Chip;

impl App {
    /// `/redo`：这一轮原样重来一遍。
    pub(super) fn redo(&mut self) {
        let Some((text, chips)) = self.redoable() else {
            return;
        };
        let turn = self.send_redo(text, chips);
        self.link.core.send(Command::Redo { text: None });
        self.talk.unsent = Some(Unsent {
            draft: None,
            original: None,
            turn,
        });
    }

    /// `/edit`：开最后一轮的那句放进输入框改。输入历史里找得到的照发出去时的样子，长文还是块。
    pub(super) fn edit_last(&mut self) {
        let Some((text, chips)) = self.redoable() else {
            return;
        };
        let draft = self
            .typing
            .input
            .sent_by_text(&text)
            .unwrap_or_else(|| Draft::from_pasted(&text, &super::paste::pasted(chips)));
        self.typing.input.start_edit(draft);
    }

    /// 编辑上一句回车了：照改过的重来（粘贴块换回原文发）。
    pub(super) fn submit_edit(&mut self, original: Draft, draft: Draft) {
        let turn = self.send_redo(draft.text.clone(), super::paste::chips(&draft));
        self.link.core.send(Command::Redo {
            text: Some(draft.expand()),
        });
        self.typing.input.remember(draft.clone());
        self.typing.input.forget_cleared();
        self.talk.unsent = Some(Unsent {
            draft: Some(draft),
            original: Some(original),
            turn,
        });
    }

    /// 重做发出去的那一下：重来的那一轮先藏掉，重发的那句画在它的位置，不先在底下闪一下；视口跟到最新，不守撤掉
    /// 之前的底边（长回答撤掉以后视口会停在空处，那句在屏幕外）。交回藏掉的是第几轮。
    fn send_redo(&mut self, text: String, chips: Vec<Chip>) -> Option<u64> {
        let turn = self.talk.transcript.hide_last_turn();
        self.talk.transcript.user(text, chips);
        self.awaiting_turn();
        self.screen.view.settle();
        self.screen.view.follow();
        turn
    }

    /// 能不能重做：在回答时、最后一轮不是你说的话开的，提示一句交回 `None`（不找会话）；能的交回那句的字和块。
    fn redoable(&mut self) -> Option<(String, Vec<Chip>)> {
        let hint = |app: &Self, reason: &str| {
            app.config
                .text()
                .refusal_hints
                .get(reason)
                .cloned()
                .unwrap_or_default()
        };
        if self.talk.transcript.running.is_some() {
            let note = hint(self, "turn_running");
            self.hint(note, false);
            return None;
        }
        let Some(said) = self.talk.transcript.last_said() else {
            let note = hint(self, "not_redoable");
            self.hint(note, false);
            return None;
        };
        Some((said.text.clone(), said.pasted.clone()))
    }
}
