//! 「空了告诉我」，被等的这一边（L7 三，从 Miyu `miyu-session/src/actor/watchers.rs` 搬来，施工 C-6，2026-10-01 项目主人定的
//! 状态机）：谁在等这个会话空下来，只记在内存里，不进内核、不进日志，窗口一关就没了（关的时候给名单上的回一封「关了」）。
//! 同一个会话只记一个，后订的替掉先订的。
//!
//! 名单上每一项多一个「上膛」的标记。订进来的时候这个会话已经空着：不当场发，等它下一次忙完才发，免得带的是它上一轮的旧回答；
//! 除非订的那一刻不晚于它上一次忙完的那一刻（带话又订、这边手快先忙完了一轮的情形；登记在忙完以后才读到的也是这样），照样
//! 当场上膛、发出去。两个时刻都照系统时间（LM-12）：登记里带订的那一刻的系统时间，这边记忙完的那一刻的系统时间。原来比的是
//! 两个会话各自的钟（Miyu 照旧这样），一个会话的钟在系统时间往回拨以后停在前头不走，比出来就反了：等的一方的停在前头，订在
//! 忙完以前的当成订在以后、一直不回；被等的一方的停在前头，订在忙完以后的当成以前、当场回上一轮的旧话。这个会话一忙起来，
//! 名单上的每一项都上膛：它下一次空下来时就能发。
//!
//! 每送完一批看一次：忙着的，记下「忙过」，名单上的都上膛；闲着、刚才还记着忙过的，清掉「忙过」，记下这一刻刚忙完。然后给名单
//! 上上膛的每一个发通知、清掉它们；没上膛的留着。
//!
//! 通知：Miyu 经会话表给等的那个会话一个命令 `PeerIdle`，这里往它的收件目录放一封 `idle`（带起算时刻、最近结束的那一轮最后
//! 一条有字的回复的第一行），那边读到了交同一个命令、同一个编号（`listen.rs`）。发通知不挡着 actor：在阻塞线程里写。那一边正在
//! 撤销、恢复的退避着再交，挪到了收的一方（`listen.rs`）。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use lm_kernel::kernel::id::SessionId;
use lm_kernel::kernel::time::Timestamp;

use super::letter::{self, Letter};
use crate::session::blocking::blocking;
use crate::store::paths::Bell;

/// 名单上的一项：等的会话这次订的起算时刻，和有没有「上膛」。
#[derive(Debug, Clone, Copy)]
struct Waiter {
    since: Timestamp,
    armed: bool,
}

/// 被等的这一边：谁在等、这个会话上一批还忙不忙、最近一次从忙变空的那一刻（系统时间，LM-12）。
#[derive(Debug, Default)]
pub(super) struct Waiters {
    waiters: BTreeMap<SessionId, Waiter>,
    busy_seen: bool,
    finished_at: Option<Timestamp>,
}

impl Waiters {
    /// 交给 actor 时这个会话忙不忙（Miyu 造 actor 时照 `!session.vacant()` 记）。
    pub(super) fn start(&mut self, vacant: bool) {
        self.busy_seen = !vacant;
    }

    /// 会话 `watcher` 等这个会话空下来：记进名单（后订的替掉先订的）。这个会话正忙着，或者订的那一刻 `wall`（系统时间）不晚于
    /// 它上一次忙完的那一刻（系统时间），当场上膛；不然不上膛。`since` 是 `watcher` 会话的钟记的起算时刻，只原样带回去，不拿来
    /// 比（LM-12）：会话的钟在系统时间往回拨以后停着不走，各会话停的地方不一样。`vacant` 是这个会话这时空不空，空着的照上膛的发。
    pub(super) fn add(
        &mut self,
        watcher: SessionId,
        (since, wall): (Timestamp, Timestamp),
        vacant: bool,
        notice: Notice<'_>,
    ) {
        let armed = !vacant || self.finished_at.is_some_and(|at| at >= wall);
        self.waiters.insert(watcher, Waiter { since, armed });
        self.notify_if_vacant(vacant, notice);
    }

    /// 一批送完了：这个会话忙不忙跟着记——忙着的，名单上的都上膛；刚从忙变空的，记下忙完的这一刻（`now`，系统时间）。然后给
    /// 上膛的发通知。
    pub(super) fn after_batch(&mut self, vacant: bool, now: Timestamp, notice: Notice<'_>) {
        if !vacant {
            self.busy_seen = true;
            for waiter in self.waiters.values_mut() {
                waiter.armed = true;
            }
        } else if self.busy_seen {
            self.busy_seen = false;
            self.finished_at = Some(now);
        }
        self.notify_if_vacant(vacant, notice);
    }

    /// 空了：给名单上上膛的每一个发通知、清掉；没上膛的留着。
    fn notify_if_vacant(&mut self, vacant: bool, notice: Notice<'_>) {
        if self.waiters.is_empty() || !vacant {
            return;
        }
        let (armed, pending): (BTreeMap<_, _>, BTreeMap<_, _>) = std::mem::take(&mut self.waiters)
            .into_iter()
            .partition(|(_, waiter)| waiter.armed);
        self.waiters = pending;
        let status = (notice.status)();
        for (watcher, waiter) in armed {
            let letter = Letter::Idle {
                from: notice.me.clone(),
                since: waiter.since.unix_millis(),
                status: status.clone(),
            };
            let (sessions, bell) = (notice.sessions.to_path_buf(), notice.bell.clone());
            tokio::spawn(async move {
                let to = watcher.clone();
                drop(blocking(move || letter::post(&sessions, &to, &letter, &bell)).await);
            });
        }
    }

    /// 窗口关了：名单上的（上没上膛都算）各回一封「关了」，它们不会再等到空下来。当场写（程序可能正在退出）。
    pub(super) fn close(&mut self, (sessions, bell): (&Path, &Bell), me: &SessionId) {
        for (watcher, waiter) in std::mem::take(&mut self.waiters) {
            let letter = Letter::Closed {
                from: me.clone(),
                since: waiter.since.unix_millis(),
            };
            drop(letter::post(sessions, &watcher, &letter, bell));
        }
    }
}

/// 发通知要的：这个会话、会话都在哪、通知带的那一行（要发时才取）。
pub(super) struct Notice<'a> {
    pub(super) me: &'a SessionId,
    pub(super) sessions: &'a PathBuf,
    pub(super) bell: &'a Bell,
    pub(super) status: &'a dyn Fn() -> Option<String>,
}
