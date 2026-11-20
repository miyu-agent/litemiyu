//! 推送（施工 9-5，`docs/blueprint/session/actor.md`「推送和订阅」第 7 条）：推给订阅了的头；每批送完写「有没有在跑的回合」
//! 「在不在等人」，和 `Handle` 共用。Miyu 这里还记下列表那一项变了、任务表变了，经会话表的端口报（会话表在 L5 搬）。

use std::sync::Arc;
use std::sync::atomic::Ordering;

use super::Actor;
use crate::session::handle::Pushed;

impl Actor {
    /// 推给订阅了的头；推的事件会让会话列表那一项变的，记下。
    #[expect(
        clippy::let_underscore_must_use,
        reason = "没有订阅者就没人收，照常往下走"
    )]
    pub(super) fn push(&mut self, pushed: Pushed) {
        let _ = self.pushes.send(Arc::new(pushed));
    }

    /// 在不在等人：交给 `Handle` 的那一份（施工 SL-1）。
    pub(crate) fn waiting(&self) -> Arc<std::sync::atomic::AtomicBool> {
        Arc::clone(&self.waiting)
    }

    /// 交上这个窗口对这个主会话的占有（L7 一，[`Message::Window`]）：忙不忙当场照这时的写。
    ///
    /// [`Message::Window`]: crate::session::handle::Message::Window
    pub(super) fn set_window(&mut self, window: crate::session::window::Window) {
        self.window = Some(window);
        if let Some(peers) = self.window.as_mut().and_then(|window| window.peers()) {
            peers.start(&self.session);
        }
        self.settle_busy();
    }

    /// 每批送完：写「有没有在跑的回合」「在不在等人」（和 `Handle` 共用）。
    pub(super) fn settle_busy(&mut self) {
        let busy = !self.session.idle();
        self.busy.store(busy, Ordering::Release);
        // 别的窗口列会话时照这把锁写忙不忙（L7 一）。
        if let Some(window) = &mut self.window {
            window.set_busy(busy);
        }
        // 在不在等人（施工 SL-1）。
        self.waiting
            .store(self.session.waiting_on_person(), Ordering::Release);
    }
}
