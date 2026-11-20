//! 发一条 `!` 打的命令（蓝图 `tui.md`「人自己跑命令 `!`」第 2 条）：照普通消息发（开会话、排队都一样），交给引擎的是
//! [`Command::Shell`]；记进输入历史时前面加 `!`。Miyu 这里还拦附件（核心 SH-1 回 `bad_params`）：LiteMiyu 还没有附件。

use super::App;
use crate::core::Command;
use crate::input::Draft;

impl App {
    /// 回车发出去的是一条命令。
    pub(super) fn submit_shell(&mut self, draft: Draft) {
        // 不能带文件块（Miyu 终端 `app/shell_send.rs::submit_shell`：核心 SH-1 回 `bad_params`；LiteMiyu 没有附件，只拦文件块）：
        // 弹一句，字留在框里。
        if draft.blocks.iter().any(|b| b.path.is_some()) {
            let note = self.config.text().shell.no_attachments.clone();
            self.hint(note, false);
            self.typing.input.editor.set_draft(draft);
            return;
        }
        // 引擎起不来：发不出去，字留在框里（「连核心」第 8 条）。
        if !self.reachable() {
            self.typing.input.editor.set_draft(draft);
            return;
        }
        let typed = self.typing.input.as_typed(&draft);
        self.typing.input.leave_shell();
        self.screen.view.follow();
        let command = draft.expand();
        self.talk.unsent = Some(super::Unsent {
            draft: Some(typed.clone()),
            original: None,
            turn: None,
        });
        self.typing.input.remember(typed);
        self.typing.input.forget_cleared();
        self.talk.transcript.shell_user(command.clone());
        self.awaiting_turn();
        self.link.core.send(Command::Shell { text: command });
    }
}
