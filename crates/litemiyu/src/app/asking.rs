//! 会话推来的确认、提问接到抽屉上（蓝图「确认和提问的抽屉」开头一段、第 1、6、7 条；核心 D-1、D-2）：来了开抽屉、
//! 这次调用了结了收起；推来的回答写进正文；交了被拒的重新打开、弹一句。

use super::App;
use crate::core::{Asking, Push, Update};
use crate::drawer::{Answered, Approval, Asked, Decided, Drawer, Outcome};

impl App {
    /// 先看一眼引擎的消息：确认、提问归抽屉（交回 `true`，不再往下走）；工具有了结果的收起它的抽屉（照常往下走）。
    pub(super) fn asking_update(&mut self, update: &Update) -> bool {
        match update {
            Update::Push(push) => {
                // 主会话等人的那几段不算进用时（Miyu `17f15122`，`transcript/waited.rs`）。
                if let Push::Asking(asking) = push {
                    self.talk.transcript.person_asked(asking);
                }
                let owner = self.talk.main_session().unwrap_or_default();
                self.asking_push_from(&owner, None, push)
            }
            Update::AnswerRefused {
                call,
                reason,
                message,
            } => {
                // 已经答过、了结了：不说，抽屉照推送收（第 7 条）。别的重新打开、弹原话。
                let owner = self.talk.main_session().unwrap_or_default();
                if reason.as_deref() != Some("not_asking")
                    && let Some(drawer) = self.talk.asks.again(&owner, call)
                {
                    self.talk.drawers.reopen(drawer);
                    // 认得的原因照界面的短话写（`refusal_hints`，例如核心 YC-7 的 `id_reused`），认不得的照原因码。
                    let hint = reason
                        .as_deref()
                        .and_then(|r| self.config.text().refusal_hints.get(r).cloned())
                        .unwrap_or_else(|| message.clone());
                    self.hint(hint, false);
                }
                true
            }
            _ => false,
        }
    }

    /// `owner` 是问的会话的整个编号，`who` 是抽屉第一行写谁在问（子代理问的才有；有它的答了结果不写进主会话的正文）。
    pub(super) fn asking_push_from(
        &mut self,
        owner: &str,
        who: Option<String>,
        push: &Push,
    ) -> bool {
        match push {
            Push::Asking(asking) => {
                self.asking(owner, who, asking);
                true
            }
            Push::ToolResult { call_id, .. } => {
                self.talk.asks.settle(owner, call_id);
                self.talk.drawers.settle(owner, call_id);
                false
            }
            _ => false,
        }
    }

    fn asking(&mut self, owner: &str, who: Option<String>, asking: &Asking) {
        let mut drawer = match asking {
            Asking::Asked(asked) => Drawer::question(Asked::of(asked)),
            Asking::Approval(asked) => Drawer::approval(Approval::of(asked)),
            Asking::Answered(answered) => {
                let a = Answered::of(answered);
                let child = who.is_some();
                self.answered(owner, child, &a.call_id.clone(), &Outcome::Answered(a));
                return;
            }
            Asking::Decided(decided) => {
                if let Some(d) = Decided::of(decided) {
                    let child = who.is_some();
                    self.answered(owner, child, &d.call_id.clone(), &Outcome::Decided(d));
                }
                return;
            }
        };
        drawer.owner = owner.to_string();
        drawer.who = who;
        // 跑命令的确认：守卫的 `detail` 没带短标题、命令的，照时间线里这一步的参数写。
        let args = self.talk.transcript.tool_args(&drawer.call_id);
        if drawer.command.is_none()
            && let Some(command) = args.and_then(|a| a["command"].as_str())
        {
            let title = args
                .and_then(|a| a["description"].as_str())
                .map(str::to_string);
            drawer.command = Some((title.filter(|t| !t.trim().is_empty()), command.to_string()));
        }
        if self.talk.asks.asked(&drawer) {
            self.talk.drawers.push(drawer);
        }
    }

    /// 会话落了盘的回答：收起抽屉；主会话的在正文末尾写结果（第 6 条）。子代理（`child`）问的不往主会话的正文写（Miyu
    /// `answered` 的 `asked.filter(|_| session.is_none())`）。
    fn answered(&mut self, owner: &str, child: bool, call_id: &str, outcome: &Outcome) {
        let asked = self.talk.asks.settle(owner, call_id);
        self.talk.drawers.settle(owner, call_id);
        let Some(drawer) = asked.filter(|_| !child) else {
            return;
        };
        super::drawer::report_into(
            &mut self.talk.transcript,
            &drawer,
            outcome,
            self.config.text(),
        );
    }
}

#[cfg(test)]
mod tests;
