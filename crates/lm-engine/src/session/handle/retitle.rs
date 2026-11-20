//! 人要求重起标题（Miyu 施工 TT-1～TT-3，`/rename` 不带名字）：内核的 `Command::Retitle`
//! 照最近几轮起；起好的标题算人起的（`session.meta_changed` 的 `by` 是要它的人），照常推一条事件。
//!
//! 和 Miyu 不一样的一处：Miyu 没起成时推一条 `session.retitle_failed` 的瞬时事件给订阅着的头；LiteMiyu 的界面照
//! `TransientBody` 逐种认，加一种界面就编不过，所以这里交回一个等结果的 [`Receiver`]：起好了是 [`Retitled::Titled`]，没起成是
//! [`Retitled::Failed`]（错和给人看的那一句，同 Miyu 那条瞬时事件的 `body`）。actor 的那一半在 `actor/retitle.rs`。

use tokio::sync::oneshot::{self, Receiver};

use super::{Handle, Message, Stopped};
use lm_kernel::kernel::event::RetitleFailed;
use lm_kernel::kernel::id::CommandId;
use lm_kernel::kernel::origin::By;
use lm_kernel::kernel::session::{Outcome, Reason};

/// 人要求的重起标题怎么样了。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Retitled {
    /// 标题变了，这是变了以后的（同时照常推一条 `session.meta_changed`）。等着的时候人又改了名的，是人改的那一个：内核照
    /// Miyu 的规矩留人改的，不再起（`lm-kernel` 的 `kernel/session/title.rs`）。
    Titled(String),
    /// 没起成：超时（辅助请求最多等 180 秒，`port::ASIDE_LIMIT`）、出错、回复里取不出标题（模型要了工具的也算）。标题没变。
    Failed(RetitleFailed),
}

impl Handle {
    /// 照最近的对话重起一次标题（Miyu 的 `session.retitle`）：`id` 是命令编号（[`crate::session::new_command_id`]），`by` 是要它
    /// 的人。收下就回，不等模型：交回的 [`Receiver`] 等结果。没东西可起的（她一个带正文的回复都没有、这时也没有回合在进行）
    /// 交回 `Err(Reason::NothingToTitle)`。会话在结果出来以前停了的，`Receiver` 收到的是出错。
    ///
    /// # Errors
    ///
    /// 会话停了。
    pub async fn retitle(
        &self,
        id: CommandId,
        by: By,
    ) -> Result<Result<Receiver<Retitled>, Reason>, Stopped> {
        let (reply, answer) = oneshot::channel();
        let (done, result) = oneshot::channel();
        self.send(Message::Retitle {
            id,
            by,
            reply,
            done,
        })?;
        Ok(match answer.await.map_err(|_| Stopped)? {
            Outcome::Rejected { reason, .. } => Err(reason),
            _ => Ok(result),
        })
    }
}
