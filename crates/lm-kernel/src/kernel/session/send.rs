//! 发一条消息（`docs/blueprint/kernel/session.md`「发一条消息」；施工 8-10 从 `session.rs` 挪出来，那边放不下了）：
//! 空的拒绝；别处来的照回报的规矩到（`messages.rs`）；空闲时记下、同一批开一个回合，回合进行中记下、排进队。

use super::action::{Action, Reason};
use super::{Session, rejected};
use crate::kernel::block::Block;
use crate::kernel::event::{Body, MessageUser, TypedCommand};
use crate::kernel::id::CommandId;
use crate::kernel::origin::By;
use crate::kernel::time::Timestamp;

/// 一句话：内容块、斜杠命令、是不是人打的命令。
type Said = (Vec<Block>, Option<TypedCommand>, bool);

impl Session {
    /// 发一条消息：一块内容都没有的拒绝，`empty_message`。别处来的（子代理的留言，施工 7-7；别的 harness 发来的话，
    /// 施工 7-10；别的会话发来的话，施工 C-2）照回报的规矩到。空闲时追加 `message.user`、同一批开一个回合；回合进行中
    /// 追加、带上这个回合、排进队，这一步里在等人的调用作废，在路上的请求一个字都还没回的撤掉重发（施工 4-15），急着插话的
    /// 再跳过还没跑的，叫停的 `CancelTool`、`CancelModel` 排在 `Append` 后面。
    pub(super) fn send(
        &mut self,
        id: CommandId,
        by: By,
        at: Timestamp,
        (blocks, command): (Vec<Block>, Option<TypedCommand>),
        urgent: bool,
    ) -> Vec<Action> {
        self.send_as(id, by, at, (blocks, command, false), urgent)
    }

    /// 人打的一条命令（施工 SH-1 上，`shell.rs`）：不是人说的拒绝 `shell_not_allowed`；空的 `empty_message`；别的照发一条消息，
    /// 记成写了 `shell` 的 `message.user`。
    pub(super) fn send_shell(
        &mut self,
        id: CommandId,
        by: By,
        at: Timestamp,
        text: String,
        urgent: bool,
    ) -> Vec<Action> {
        if !matches!(by, By::Person(_)) {
            return vec![rejected(id, Reason::ShellNotAllowed)];
        }
        if text.trim().is_empty() {
            return vec![rejected(id, Reason::EmptyMessage)];
        }
        let blocks = vec![Block::Text(crate::kernel::block::Text { text })];
        self.send_as(id, by, at, (blocks, None, true), urgent)
    }

    /// 同 [`Self::send`]，第三格是写不写 `shell`。
    fn send_as(
        &mut self,
        id: CommandId,
        by: By,
        at: Timestamp,
        (blocks, command, shell): Said,
        urgent: bool,
    ) -> Vec<Action> {
        if blocks.is_empty() {
            return vec![rejected(id, Reason::EmptyMessage)];
        }
        if let Some(waker) = self.elsewhere(&by) {
            return self.elsewhere_says(id, by, at, blocks, waker);
        }
        let body = Body::MessageUser(MessageUser {
            blocks,
            command,
            shell,
        });
        let message = self.record(at, by.clone(), Some(id.clone()), body);
        self.accept(id.clone(), vec![message.seq]);
        let trigger = message.seq;
        let mut events = vec![message];
        let mut stops = Vec::new();
        if self.turn.is_none() {
            events.extend(self.open_turn(at, trigger, Some(id)));
        } else {
            self.enqueue(trigger, id.clone());
            let (voided, stopped) = self.void_waiting(at, &by, &id);
            events.extend(voided);
            stops = stopped;
            // 在路上的请求一个字都还没回的，撤掉带上它重发（施工 4-15，`supersede.rs`）。
            if let Some((superseded, cancel)) = self.supersede(at, &id) {
                events.extend(superseded);
                stops.push(cancel);
            }
            if urgent {
                events.extend(self.interject(at, by, id));
            }
        }
        let mut actions = vec![Action::Append(events)];
        actions.extend(stops);
        actions
    }
}
