//! 窗口之间（L7 二、三，`crate::session::peers`）：收件目录里读到的、不进内核的那几样当场办完；每批送完照内核看一次「空了告诉
//! 我」的两边（Miyu `actor/watchers.rs` 的 `after_batch`）。

use lm_kernel::kernel::session::Input;

use super::Actor;
use super::mail::Mail;
use crate::session::peers::Peer;
use lm_kernel::kernel::event::IdleReason;

impl Actor {
    /// 收件目录里读到的一样。
    pub(super) fn peer_mail(&mut self, peer: Peer) -> Mail {
        match peer {
            // 对方回过话了：发给它的防刷屏从这以后数（`peers/outbox.rs`）。
            Peer::Heard(from) => self.tools.heard(&from),
            // 别的会话等这个会话空下来：记进名单（`peers/waiters.rs`）。没有窗口占有的（锁文件开不了）收不到登记。
            Peer::Watch { from, since, wall } => {
                if let Some(peers) = self.window.as_mut().and_then(|window| window.peers()) {
                    peers.watched_by(from, (since, wall), &self.session);
                }
            }
            // 等的那个会话关了：还在等的正是那一次的，交内核记下；订了又订的旧的不理。
            Peer::Closed { from, since } => {
                if self
                    .session
                    .watching()
                    .iter()
                    .any(|(session, at)| *session == from && *at == since)
                {
                    return Mail::Input(Box::new(Input::WatchEnded {
                        at: self.clock.now(),
                        session: from,
                        reason: IdleReason::Closed,
                    }));
                }
            }
        }
        Mail::Done
    }

    /// 一批送完了：「空了告诉我」等的一边去订、计时，被等的一边记忙闲、发通知（`crate::session::peers::Peers`）。子会话、
    /// 没有窗口占有的没有。
    pub(super) fn after_batch(&mut self) {
        let now = self.clock.now();
        if let Some(peers) = self.window.as_mut().and_then(|window| window.peers()) {
            peers.after_batch(&self.session, now, &self.backs);
        }
    }
}
