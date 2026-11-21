//! 人开、停循环（L5 四，`/loop` 的引擎那一半；Miyu 在核心的斜杠命令 `commands/looping.rs` 里）：界面照
//! [`crate::schedule::parse`] 读 `/loop` 后面的字，再调这里。循环只记在 actor 内存里（`session/looping.rs`），不进内核、不写盘；
//! 交进来的每一句照「别处来的话」记成 `by: schedule`；开的那一刻交的第一句另带人敲的那一行（LM-34）。

use tokio::sync::oneshot;

use super::{Handle, Message, Stopped};
use crate::session::looping::{Looping, Order};

impl Handle {
    /// 认不认 `/loop`（Miyu 的 `Handle::loops`）：造会话、载入时工具面上有 `schedule_wakeup` 的才认。界面照它决定命令菜单里列不列、
    /// 敲了照「没有这条命令」答。
    pub fn loops(&self) -> bool {
        self.shown().loops
    }

    /// 现在的循环：没开着的没有。actor 每改一次循环写一次，界面在推送以后照它画。
    pub fn looping(&self) -> Option<Looping> {
        self.shown().looping.clone()
    }

    /// 开一个循环，换掉原来的（一个会话同时只有一个，同 Miyu）：`every` 是固定间隔的秒数（照 [`crate::schedule::parse`]
    /// 收过下限），没有的是她自己定节奏；`typed` 是人敲的那一整行（例如 `/loop 10m 看一眼时间`）。开的那一刻先交一次那句话，
    /// 带着 `typed`：算人的一次操作，有一次撤回还能恢复也照样开一轮，正文里照 `typed` 画（LM-34）。交回原来有没有一个被换掉。
    ///
    /// # Errors
    ///
    /// 会话停了。
    pub async fn start_loop(
        &self,
        every: Option<u64>,
        prompt: String,
        typed: String,
    ) -> Result<bool, Stopped> {
        self.order(Order::Start {
            every,
            prompt,
            typed,
        })
        .await
    }

    /// 停掉这个会话的循环（`/loop off`）：交回停掉了一个，还是本来就没有。
    ///
    /// # Errors
    ///
    /// 会话停了。
    pub async fn stop_loop(&self) -> Result<bool, Stopped> {
        self.order(Order::Stop).await
    }

    async fn order(&self, order: Order) -> Result<bool, Stopped> {
        let (reply, answer) = oneshot::channel();
        self.send(Message::Loop(order, reply))?;
        answer.await.map_err(|_| Stopped)
    }

    fn shown(&self) -> std::sync::MutexGuard<'_, crate::session::shown::Shown> {
        self.shown
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}
