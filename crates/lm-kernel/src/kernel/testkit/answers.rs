//! 替身上人回答确认、回答提问的命令（施工 M-7 上从 `stage.rs` 挪出来：那一份到了行数上限）。

use super::Stage;
use crate::kernel::event::{Decision, Response};
use crate::kernel::id::{CallId, CommandId};
use crate::kernel::session::{Answer, Command};

impl Stage {
    /// 回答调用 `call_id` 的确认。
    pub fn decide(
        &mut self,
        call_id: CallId,
        decision: Decision,
        reason: Option<&str>,
    ) -> CommandId {
        self.command(Command::Answer {
            call_id,
            answer: Answer::Approval {
                decision,
                reason: reason.map(str::to_string),
                attachments: Vec::new(),
            },
        })
    }

    /// 回答调用 `call_id` 问的那组题。
    pub fn reply(&mut self, call_id: CallId, answers: Vec<Response>) -> CommandId {
        self.command(Command::Answer {
            call_id,
            answer: Answer::Questions(answers),
        })
    }
}
