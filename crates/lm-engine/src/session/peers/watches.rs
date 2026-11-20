//! 「空了告诉我」，等的这一边（L7 三，从 Miyu `miyu-session/src/peers.rs` 搬来，施工 C-6，`cross-session.md` 第六条第 3、8、9
//! 款）：工具只报效果 `peer.watch`，订是这里的事。
//!
//! 每送完一批，照内核这时在等的（`Session::watching`）和这里订过的比：新多出来的、起算时刻变了的，往那个会话的收件目录里放
//! 一封登记（Miyu 是经会话表交给它的 actor），同时按 `peers.watch_hours` 起一个计时；不在等了的，计时撤掉。落了盘才订：一批
//! 送完的时候，这一批追加的都已经落了盘。载入以后账本里在等的都算新多出来的，走同一条路（再放一封登记，那边同一个会话只记
//! 一个）；已经到点的不订，当场交作废。
//!
//! 订不上的：那个会话的目录不在了，交内核 `gone`；没有窗口开着它（LiteMiyu 多的一种，Miyu 的核心会替它开），交内核 `closed`，
//! 「当场知道」；别的（写不进它的收件目录）记一行 `WARN watch not placed`，计时照走，到点作废。计时到了交内核 `expired`：
//! 订了又订的，旧的计时到了，内核照账本的起算时刻不理。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio::sync::mpsc;
use tokio::task::AbortHandle;

use lm_kernel::kernel::event::IdleReason;
use lm_kernel::kernel::id::SessionId;
use lm_kernel::kernel::time::Timestamp;

use super::letter::{self, Letter};
use crate::session::blocking::blocking;
use crate::session::clock::wall_now;
use crate::session::port::Back;
use crate::store::paths::Bell;

/// 一小时多少毫秒。
const HOUR_MS: i64 = 3_600_000;

/// 订过的：每个在等的会话，从哪一刻算起，和它的计时。
#[derive(Debug, Default)]
pub(super) struct Watches {
    placed: BTreeMap<SessionId, (Timestamp, AbortHandle)>,
}

impl Watches {
    /// 一批送完了：照内核这时在等的 `watching`（订了 `hours` 小时作废）去订、计时，不在等了的撤掉计时。`now` 是这一刻，这个
    /// 会话是 `me`，会话都在 `sessions` 下（门铃照 `bell`），到点、不在了、没开着交回 `backs`。Miyu 收的是内核的会话（照它取这两样），这里收
    /// 取好的两样：好在单元测试里不造内核。
    pub(super) fn sync(
        &mut self,
        watching: Vec<(SessionId, Timestamp)>,
        hours: u64,
        now: Timestamp,
        (sessions, me, bell): (&Path, &SessionId, &Bell),
        backs: &mpsc::UnboundedSender<Back>,
    ) {
        let watching: BTreeMap<SessionId, Timestamp> = watching.into_iter().collect();
        self.placed.retain(|peer, (since, timer)| {
            let still = watching.get(peer) == Some(since);
            if !still {
                timer.abort();
            }
            still
        });
        let hours = i64::try_from(hours).unwrap_or(i64::MAX);
        for (peer, since) in watching {
            if self.placed.contains_key(&peer) {
                continue;
            }
            let due = since
                .unix_millis()
                .saturating_add(hours.saturating_mul(HOUR_MS));
            let wait = due.saturating_sub(now.unix_millis());
            if wait > 0 {
                place(
                    (sessions.to_path_buf(), bell.clone()),
                    me.clone(),
                    peer.clone(),
                    since,
                    backs.clone(),
                );
            }
            let timer = expire_after(wait, peer.clone(), backs.clone());
            self.placed.insert(peer, (since, timer));
        }
    }
}

/// 计时撤掉：actor 停了，到点了也没人要。
impl Drop for Watches {
    fn drop(&mut self) {
        for (_, timer) in self.placed.values() {
            timer.abort();
        }
    }
}

/// 订不上的为什么。
enum NotWatched {
    /// 会话目录不在了。
    Gone,
    /// 没有窗口开着它。
    Closed,
    /// 写不进它的收件目录。
    Failed,
}

/// 起一个任务，往 `peer` 的收件目录里放一封「`me` 在等你空下来」（带订的那一刻的系统时间，LM-12）。不在了、没开着的交回
/// `gone`、`closed`，别的订不上的记一行。
fn place(
    (sessions, bell): (PathBuf, Bell),
    me: SessionId,
    peer: SessionId,
    since: Timestamp,
    backs: mpsc::UnboundedSender<Back>,
) {
    // 订的那一刻的系统时间（LM-12）：这时离这一批落盘只差几微秒。会话的钟没停的，`since` 就是那一刻的系统时间；停住了的
    // （系统时间往回拨了）`since` 比系统时间晚，照此刻的系统时间。载入以后再放的登记（账本里早订了的）照样取两者里早的：
    // 钟没停的照 `since`，就是当初订的那一刻。
    let wall = since.unix_millis().min(wall_now().unix_millis());
    tokio::spawn(async move {
        let to = peer.clone();
        let placed = blocking(move || {
            let dir = sessions.join(to.as_str());
            if !dir.is_dir() {
                return Err(NotWatched::Gone);
            }
            if !crate::session::window::open_elsewhere(&dir) {
                return Err(NotWatched::Closed);
            }
            let letter = Letter::Watch {
                from: me,
                since: since.unix_millis(),
                wall: Some(wall),
            };
            letter::post(&sessions, &to, &letter, &bell).map_err(|_| NotWatched::Failed)
        })
        .await;
        match placed {
            Ok(()) => {}
            Err(NotWatched::Gone) => send(&backs, peer, IdleReason::Gone),
            Err(NotWatched::Closed) => send(&backs, peer, IdleReason::Closed),
            Err(NotWatched::Failed) => {}
        }
    });
}

/// 起一个计时：`wait` 毫秒以后（已经过了的当场）交回 `expired`。交回撤掉它的那一头。
fn expire_after(wait: i64, peer: SessionId, backs: mpsc::UnboundedSender<Back>) -> AbortHandle {
    let wait = Duration::from_millis(u64::try_from(wait).unwrap_or(0));
    tokio::spawn(async move {
        tokio::time::sleep(wait).await;
        send(&backs, peer, IdleReason::Expired);
    })
    .abort_handle()
}

/// 交回 actor。会话停了就送不进去，丢掉。
#[expect(
    clippy::let_underscore_must_use,
    reason = "会话停了：等不到了也没人要，丢掉"
)]
fn send(backs: &mpsc::UnboundedSender<Back>, session: SessionId, reason: IdleReason) {
    let _ = backs.send(Back::WatchEnded { session, reason });
}
