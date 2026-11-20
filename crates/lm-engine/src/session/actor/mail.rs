//! 人的那条收件箱里的一封怎么办（`docs/blueprint/session/actor.md` 第 3 条第 4 点）：命令照 actor 的时钟记下到的时刻送进
//! 内核，订阅当场办（连同补发补到哪一条，施工 3-8 六补）；拿着订阅的头从没有到有、从有到没有，交内核 `Watched`
//! （施工 7-9），和 `Handle` 共用的旗一起写（施工 C-5）。施工 7-9 从 `actor.rs` 挪出来。
//! 头读后台命令的输出（施工 7-4 补）不进内核：另起一个任务读，当场办完。别的会话等这个会话空下来（施工 C-6）也不进内核：
//! 记进名单，当场办完。

use std::sync::atomic::Ordering;

use tokio::sync::oneshot;

use lm_kernel::kernel::id::Seq;
use lm_kernel::kernel::session::{Input, Reason, Received};

use super::{Actor, Stop, answer};
use crate::session::handle::{Halt, Message, Taken};

/// 收件箱里的一封怎么办。
pub(super) enum Mail {
    /// 送进内核。装在盒子里，同 [`Message::Command`]（clippy 的 `large_enum_variant`）。
    Input(Box<Input>),
    /// 当场办完了。
    Done,
    /// 有计划地停下，停好了回这一头。
    Stop(oneshot::Sender<()>),
    /// 删会话之前停下（施工 3-8 三补，`stop.rs`）。
    Delete(bool, oneshot::Sender<Result<(), Reason>>),
    /// 停掉派出去的任务（施工 7-4）：要等杀掉、存好，在 `halt.rs` 里办。
    Halt(Halt),
}

impl Actor {
    /// 收件箱里的一封：命令照 actor 的时钟记下到的时刻，订阅当场办，看着的头有没有变了送进内核（施工 7-9）。
    /// 收件箱里的一封，`None` 是收件箱关了：订阅的先把攒着的写进去（补发的那一截照日志读，要齐，速度线二），再照
    /// [`Actor::mail`] 办。写不进去的停下。
    pub(super) async fn post(&mut self, message: Option<Message>) -> Result<Option<Mail>, Stop> {
        let Some(message) = message else {
            return Ok(None);
        };
        if matches!(message, Message::Subscribe(_)) {
            self.write_held().await?;
        }
        // 头问上下文一块块的（`parts.rs`）：收着的要先从日志读回来、累计的要读整份日志，都得等。
        if let Message::Context(asked) = message {
            self.parts(asked).await?;
            return Ok(Some(Mail::Done));
        }
        Ok(Some(self.mail(message)))
    }

    pub(super) fn mail(&mut self, message: Message) -> Mail {
        match message {
            Message::Command {
                id,
                by,
                command,
                reply,
            } => {
                self.wait_for(id.clone(), reply);
                let at = self.clock.now();
                Mail::Input(Box::new(Input::Command(Received {
                    id,
                    by,
                    at,
                    command: *command,
                })))
            }
            // 补发补到哪一条和订阅在这同一步里拿（施工 3-8 六补）：这一步之前落了盘的都推过了，之后的都还没推。正在流的
            // 那次请求推过的增量也在这一步里拼好（施工 9-8 八补）：之前推过的都在里面，之后的照常推。
            Message::Subscribe(reply) => {
                let taken = Taken {
                    pushes: self.pushes.subscribe(),
                    upto: self.session.landed().map_or(0, Seq::get),
                    log: self.tools.log(),
                    current: self.current(),
                    streaming: self.session.streaming(self.clock.now()),
                };
                answer(reply, taken);
                self.watch(true)
            }
            Message::Current(reply) => {
                answer(reply, self.current());
                Mail::Done
            }
            Message::Unsubscribed => self.watch(false),
            Message::Stop(reply) => Mail::Stop(reply),
            Message::Delete { force, reply } => Mail::Delete(force, reply),
            Message::Halt(halt) => Mail::Halt(halt),
            // `post` 已经办了。
            Message::Context(_) => Mail::Done,
            Message::Retitle {
                id,
                by,
                reply,
                done,
            } => self.retitle_asked(id, by, reply, done),
            // 头读后台命令的输出（施工 7-4 补）：是什么当场照名册看，开文件另起一个任务。
            Message::Output { job, reply } => {
                let reading = self.jobs.command_output(job);
                tokio::spawn(async move { answer(reply, reading.await) });
                Mail::Done
            }
            // 人开、停循环（L5 四，`looping.rs`）：不进内核，当场办完。
            Message::Loop(order, reply) => self.loop_order(order, reply),
            Message::Window(window) => {
                self.set_window(*window);
                Mail::Done
            }
            // 收件目录里读到的（L7 二，`actor/peers.rs`）：不进内核，当场办完。
            Message::Peer(peer) => self.peer_mail(peer),
            // 没人能确认（`Handle::unattend`）：只改内核手里的策略，不记事件。
            Message::Unattended => {
                self.session.set_attended(false);
                Mail::Done
            }
        }
    }

    /// 多了（`more`）或者少了一个拿着订阅的头（施工 7-9）：从没有到有、从有到没有，交内核 `Watched`，和 `Handle` 共用的
    /// 那面旗一起写（施工 C-5，照 `busy` 的做法）；别的当场办完。
    fn watch(&mut self, more: bool) -> Mail {
        let before = self.watchers;
        self.watchers = match more {
            true => before + 1,
            false => before.saturating_sub(1),
        };
        self.watched.store(self.watchers > 0, Ordering::Release);
        match (before, self.watchers) {
            (0, 1) => Mail::Input(Box::new(Input::Watched { watched: true })),
            (1, 0) => Mail::Input(Box::new(Input::Watched { watched: false })),
            _ => Mail::Done,
        }
    }
}
