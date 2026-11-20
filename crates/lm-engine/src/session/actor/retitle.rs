//! 人要求重起标题的 actor 那一半（Miyu 施工 TT-1～TT-3，`handle/retitle.rs` 写了为什么另交一头等结果）：收下时记着等结果的
//! 那一头，内核拒了的放掉；标题一变（起好了、等着的时候人又改了）交 [`Retitled::Titled`]，内核说没起成交 [`Retitled::Failed`]。
//! 内核那边「人要的」只有一份（几次要求并成一次），这里也是一起交、一起放。

use tokio::sync::oneshot;

use lm_kernel::kernel::event::{Body, Event, RetitleFailed};
use lm_kernel::kernel::id::CommandId;
use lm_kernel::kernel::origin::By;
use lm_kernel::kernel::session::{Command, Input, Outcome, Received};

use super::mail::Mail;
use super::{Actor, answer};
use crate::session::handle::Retitled;

impl Actor {
    /// 收下一次重起标题的要求：记着等结果的那一头，照 [`Command::Retitle`] 送进内核。
    pub(super) fn retitle_asked(
        &mut self,
        id: CommandId,
        by: By,
        reply: oneshot::Sender<Outcome>,
        done: oneshot::Sender<Retitled>,
    ) -> Mail {
        self.wait_for(id.clone(), reply);
        self.retitles.push((id.clone(), done));
        let at = self.clock.now();
        Mail::Input(Box::new(Input::Command(Received {
            id,
            by,
            at,
            command: Command::Retitle,
        })))
    }

    /// 内核回了命令 `id`：拒了的重起要求放掉等结果的那一头（它收到出错）。
    pub(super) fn retitle_replied(&mut self, id: &CommandId, outcome: &Outcome) {
        if matches!(outcome, Outcome::Rejected { .. }) {
            self.retitles.retain(|(asked, _)| asked != id);
        }
    }

    /// 有等着重起的时，要写的这一批里标题变成了什么（最后一次）；没人等、标题没变的没有。
    pub(super) fn retitle_in(&self, events: &[Event]) -> Option<String> {
        if self.retitles.is_empty() {
            return None;
        }
        events.iter().rev().find_map(|event| match &event.body {
            Body::MetaChanged(changed) => changed.title.clone(),
            _ => None,
        })
    }

    /// 这一批写进去了，标题变成了 `title`（[`Actor::retitle_in`]）：交给所有等着的。
    pub(super) fn retitle_landed(&mut self, title: Option<String>) {
        let Some(title) = title else {
            return;
        };
        for (_, done) in self.retitles.drain(..) {
            answer(done, Retitled::Titled(title.clone()));
        }
    }

    /// 内核说人要的重起没起成：运行日志记一行，交给所有等着的。
    pub(super) fn retitle_failed(&mut self, failed: RetitleFailed) {
        for (_, done) in self.retitles.drain(..) {
            answer(done, Retitled::Failed(failed.clone()));
        }
    }
}
