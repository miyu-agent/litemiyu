//! 造会话、载入时定下的几样：和 actor 共用的「在不在等人」（施工 SL-1）。Miyu 这里还有 `/goal`、`/loop` 认不认、关没关起来、
//! 记忆的那一间（L3 二删，`/loop` 在 L5 跟着搬回来；`/goal` 2026-11-14 砍了）。

use super::Handle;

impl Handle {
    /// 带上和 actor 共用的「在不在等人」（施工 SL-1）。
    #[must_use]
    pub(crate) fn with_waiting(
        mut self,
        waiting: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ) -> Handle {
        self.waiting = waiting;
        self
    }

    /// 交给 actor 收件目录里读到的、不进内核的一样（L7 二）。
    ///
    /// # Errors
    ///
    /// 会话停了。
    pub(crate) fn peer(&self, peer: crate::session::peers::Peer) -> Result<(), super::Stopped> {
        self.send(super::Message::Peer(peer))
    }

    /// 交给 actor 拿着这个窗口对会话的占有（L7 一）：actor 已经停了的，跟着放下。
    pub(crate) fn keep(&self, window: crate::session::window::Window) {
        if self.send(super::Message::Window(Box::new(window))).is_err() {}
    }

    /// 在不在等人确认、回答（施工 SL-1）：会话列表照它写 `waiting`。会话停了的照最后一次写的。
    pub fn waiting(&self) -> bool {
        self.waiting.load(std::sync::atomic::Ordering::Acquire)
    }
}
