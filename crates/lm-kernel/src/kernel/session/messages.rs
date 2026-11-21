//! 父子之间的留言，内核这一头（施工 7-7，`docs/blueprint/agents.md` 第六条，`docs/blueprint/kernel/session.md`「子代理的
//! 留言」）。
//!
//! 父会话发给子代理的留言就是父会话发来的 `message.user`，和交代一样走「发一条消息」：内核不另认。子代理发给父会话的留言，
//! 发命令的是这个会话派的子代理（`by` 是它的子会话），照回报的规矩到（`jobs.rs` 的 [`Session::land`]）：不带回合编号，
//! 闲着、这时开得了由它开一轮，正忙排进这一轮的回报队、下一步看到，没人看着的一次性会话只记下；派它的那一轮撤掉了的不叫醒。
//! 它不是人说的话：打断时不撤回，不作废在等人的调用，撤销时留着。
//!
//! 执行器派 `send_message` 的调用之前，照 [`Session::subagents`] 抄一份这个会话派出去的子代理，工具照它认 `to`。
//!
//! 别的 harness 发来的话（`by` 是 `harness`，施工 7-10，`kernel/session.md`「别的 harness 发来的话」）也是别处来的，走同一条
//! 路：它不是哪个任务的，派它的那一轮撤掉了这回事没有，一律叫醒。别的会话发来的话（施工 C-2，`peers.rs`）先过防刷屏，
//! 再走这一条路。

use std::collections::BTreeMap;

use super::Session;
use super::action::Action;
use super::jobs::Waker;
use crate::kernel::block::Block;
use crate::kernel::event::{Body, MessageUser, TypedCommand};
use crate::kernel::id::{CommandId, JobId, SessionId};
use crate::kernel::ledger::Ledger;
use crate::kernel::origin::By;
use crate::kernel::time::Timestamp;

/// 这个会话派出去的一个子代理（施工 7-7）：`send_message` 发给它之前照它认。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Subagent {
    /// 它的子会话：留言发到这里。
    pub session: SessionId,
    /// 被停掉了（以 `stopped`、`undone` 报过）：不再收留言。
    pub stopped: bool,
}

impl Session {
    /// 这个会话派出去的子代理，照编号（施工 7-7）：派它的那一轮撤掉了的不在（她看不到派它的那次调用，也就不是她的）。
    /// 做完了、崩了报过的照样在：它的会话还在，留言开它的下一轮。纯查询：执行器派每一次调用之前抄一份交给 `send_message`。
    pub fn subagents(&self) -> BTreeMap<JobId, Subagent> {
        self.ledger
            .subagents()
            .filter(|(job, _, _)| {
                self.history
                    .dispatched(job)
                    .is_some_and(|dispatched| !dispatched.undone)
            })
            .map(|(job, session, stopped)| {
                let session = session.clone();
                (job, Subagent { session, stopped })
            })
            .collect()
    }

    /// 发消息的是别处：这个会话派的子代理，别的 harness（施工 7-10），别的会话（施工 C-2）。交回它叫醒她时是谁的。
    pub(super) fn elsewhere(&self, by: &By) -> Option<Waker> {
        sent_by(&self.ledger, by)
    }

    /// 别处来的一句（子代理的留言、别的 harness 发来的话、别的会话发来的话）：记一条 `message.user`，`by` 照发的，`cause`
    /// 是这个命令，不带回合编号；照回报的规矩叫不叫醒她。落了盘回应，附上这一条的序号。别的会话发来的一律收：LiteMiyu 没有
    /// 共同的核心，收的一方拒了发的一方不知道，防刷屏改在发的一方当场拦（L7 二，`lm-engine` 的 `session/peers/outbox.rs`）。
    ///
    /// 定时任务交来的、带着人敲的那一行的（[`crate::kernel::session::Command::Typed`]，`by` 是 `schedule`）：人亲手敲
    /// `/loop` 的那一刻交进来的第一句（LM-34）。它是人的一次操作，和人说一句话一样：`message.user` 带上那一行，还能恢复撤销
    /// 时照样开一轮（开了以后那次撤销就恢复不了，账本本来就这样）；别的几条照看，没人看着的一次性会话、要重启了的照旧只记下。
    /// 别处来的其余几种不认那一行，照旧不带。
    ///
    /// # Panics
    ///
    /// 过不了账本：不带回合编号的 `message.user` 哪条规矩都不犯，过不了只会是内核自己的 bug。
    pub(super) fn elsewhere_says(
        &mut self,
        id: CommandId,
        by: By,
        at: Timestamp,
        (blocks, command): (Vec<Block>, Option<TypedCommand>),
        waker: Waker,
    ) -> Vec<Action> {
        let command = command.filter(|_| waker == Waker::Schedule);
        let typed = command.is_some();
        let body = Body::MessageUser(MessageUser {
            blocks,
            command,
            shell: false,
        });
        let events = match self.land_as(at, by, Some(id.clone()), body, Some(waker), typed) {
            Ok(events) => events,
            Err(error) => {
                panic!("a message from elsewhere failed the ledger, a kernel bug: {error}")
            }
        };
        self.accept(id, vec![events[0].seq]);
        vec![Action::Append(events)]
    }
}

/// `by` 是别处：这个会话派的子代理（它的子会话），交回它的任务，被停掉的、撤掉的回合里派的也认；
/// 别的会话，既不是父会话、也不是派的子代理（施工 C-2）；到点的定时任务（施工 J-1 上）。别的（人、父会话……）没有：照「发一条消息」。
pub(super) fn sent_by(ledger: &Ledger, by: &By) -> Option<Waker> {
    match by {
        By::Session(session) => match ledger.subagent_in(&session.id) {
            Some(job) => Some(Waker::Job(job)),
            None => ledger.is_peer(&session.id).then_some(Waker::Peer),
        },
        By::Schedule(_) => Some(Waker::Schedule),
        _ => None,
    }
}
