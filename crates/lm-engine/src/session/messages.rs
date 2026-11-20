//! 发话，执行器这一头（`docs/blueprint/agents.md` 第六条，`session/tools.md`「父子之间留言」，施工 7-7，从 Miyu
//! `miyu-session/src/messages.rs` 搬来）：`to` 认出是哪个会话，把话作为这个会话发来的话（`by` 是这个会话）经会话表的端口
//! （[`SessionPort`]）送过去，对方落了盘就交回。
//!
//! 每一次调用照内核这一刻交的这个会话派出去的子代理（[`Session::subagents`]）造一个端口：那以后才派的，她还不知道编号；
//! 送出去之前被停掉的，送过去它照样收（它的会话还在），父会话不会再认它的回报，和她停它之前刚发出去一样。
//!
//! 别的会话（施工 C-5，`Recipient::Session`）要列会话的端口才认得出（`send_message` 照 `call.sessions` 找）。LiteMiyu（L7 二）
//! 不经会话表送：写进它的收件目录（`crate::session::peers::Outbox`），防刷屏在那里当场拦。Miyu 送到以后问一句对方是不是没人看着的一次性会话（`held`）、带附件（施工 C-9）这两样不搬：
//! 会话表里只有主会话和它派的子会话，都不是一次性的。拒绝的原因码（防刷屏的三种）译成 [`NotSent`] 的那几种；别的原因码
//! （这几条规矩之外的，内核不会真的给，留着兜底）照送不到算。
//!
//! [`Session::subagents`]: lm_kernel::kernel::session::Session::subagents
//! [`SessionPort`]: crate::session::spawn::SessionPort

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::tool::{Delivered, MessagePort, NotSent, Recipient, Sending};
use lm_kernel::kernel::block::{Block, Text};
use lm_kernel::kernel::id::{CallId, CommandId, JobId, SessionId};
use lm_kernel::kernel::origin::{By, Session};
use lm_kernel::kernel::session::{Command, Outcome, Reason, Subagent};

use crate::session::agents::Agents;
use crate::session::peers::Outbox;

/// 交给一次调用的发话端口：照这个会话的 `agents`，`subagents` 是这一刻派出去的子代理，命令编号照这次调用 `call` 定。
/// 发给别的窗口的写进它的收件目录（`outbox`，L7 二）：只有主会话有。
pub(crate) fn for_call(
    agents: &Arc<Agents>,
    call: CallId,
    subagents: BTreeMap<JobId, Subagent>,
    outbox: Option<Arc<Outbox>>,
) -> Arc<dyn MessagePort> {
    Arc::new(Messenger {
        agents: Arc::clone(agents),
        call,
        subagents,
        outbox,
    })
}

/// 一次调用的发话端口。
struct Messenger {
    agents: Arc<Agents>,
    call: CallId,
    subagents: BTreeMap<JobId, Subagent>,
    outbox: Option<Arc<Outbox>>,
}

impl MessagePort for Messenger {
    fn send<'a>(&'a self, to: Recipient, message: &'a str) -> Sending<'a> {
        Box::pin(async move {
            // 别的窗口的会话不在会话表里：经会话表送会把它当子会话载入进这个进程、两个进程写一份日志。写进它的收件目录。
            if let Recipient::Session(session) = to {
                let outbox = self.outbox.as_ref().ok_or(NotSent::Undelivered)?;
                return outbox.send(session, self.call, message).await;
            }
            let (session, _) = self.recipient(to)?;
            let agents = &self.agents;
            let by = By::Session(Session {
                id: agents.session.clone(),
            });
            let send = Command::Send {
                blocks: vec![Block::Text(Text {
                    text: message.to_string(),
                })],
                urgent: false,
            };
            let outcome = agents.port.command(session, self.id(), by, send).await;
            match outcome {
                Ok(Outcome::Accepted { .. } | Outcome::Reverted { .. }) => Ok(Delivered::Sent),
                Ok(Outcome::Rejected { reason, .. }) => Err(match reason {
                    Reason::TooManyMessages => NotSent::TooMany,
                    Reason::DuplicateMessage => NotSent::Duplicate,
                    Reason::InboxFull => NotSent::InboxFull,
                    _ => NotSent::Undelivered,
                }),
                Err(_) => Err(NotSent::Undelivered),
            }
        })
    }
}

impl Messenger {
    /// 发给哪个会话，和运行日志里怎么写它：父会话写 `parent`，子代理写它的编号，别的会话（施工 C-5）写短编号。
    fn recipient(&self, to: Recipient) -> Result<(SessionId, String), NotSent> {
        match to {
            Recipient::Parent => {
                let parent = self.agents.parent.clone().ok_or(NotSent::NoParent)?;
                Ok((parent, "parent".to_string()))
            }
            Recipient::Child(job) => match self.subagents.get(&job) {
                None => Err(NotSent::NotYours),
                Some(subagent) if subagent.stopped => Err(NotSent::Stopped),
                Some(subagent) => Ok((subagent.session.clone(), job.to_string())),
            },
            // 上面已经写进收件目录了，到不了这里。
            Recipient::Session(_) => Err(NotSent::Undelivered),
        }
    }

    /// 命令编号：`<这个会话>/message/<调用编号>`。会话编号整个数据根里不重，调用编号一个会话里不重，所以它在哪儿都不重；
    /// 一次调用只送一次。
    fn id(&self) -> CommandId {
        CommandId::parse(&format!("{}/message/{}", self.agents.session, self.call))
            .unwrap_or_else(|e| unreachable!("会话编号、调用编号都短，合命令编号的写法：{e}"))
    }
}
