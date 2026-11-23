//! 窗口之间交流（L7 二、三，设计第五节第 8 条，Miyu 蓝图 `cross-session.md`）：Miyu 里核心的会话表替会话们送的话、登记、
//! 通知，LiteMiyu 换成每个主会话目录里的收件目录（`letter.rs`）。
//!
//! - 发（`outbox.rs`）：`send_message` 发给别的会话的话写进它的收件目录；防刷屏在发的一方当场拦。
//! - 门铃（`bell.rs`，LM-4）：开着的窗口绑一个 Unix 数据报套接字，发的一方放好信以后敲一下；只是叫醒，信照旧走文件。
//! - 收（`listen.rs`）：开着的窗口等门铃响、每 30 秒兜底扫一次收件目录（绑不上门铃的每 5 秒扫一次），读到的交给自己的
//!   actor；没开着的下次打开、载入做完以后先读。
//! - 「空了告诉我」（L7 三，[`Peers`]）：等的一边照内核在等的往对方收件目录放登记、计时（`watches.rs`）；被等的一边记名单，
//!   闲下来时回一封 `idle`，窗口关了回一封 `closed`（`waiters.rs`）。
//!
//! 只有主会话有：子会话不在别的窗口的列会话里，发不到，也等不了。

mod bell;
mod letter;
mod listen;
mod outbox;
mod waiters;
mod watches;

pub use bell::private_spot;
pub(crate) use bell::{Placed, forget};
#[cfg(feature = "testkit")]
pub(crate) use bell::{euid, ring, spots};
pub(crate) use letter::{Letter, post_named, refused_mark};
pub(crate) use listen::{Inbox, listen, said_command, watch};
pub(crate) use outbox::Outbox;

use std::path::{Path, PathBuf};

use tokio::sync::mpsc;

use lm_kernel::kernel::id::SessionId;
use lm_kernel::kernel::session::Session;
use lm_kernel::kernel::time::Timestamp;

use crate::session::clock::wall_now;
use crate::session::port::Back;
use crate::store::paths::Bell;

/// 收件目录里读到的、交给 actor 自己办的（不进内核的那几样）。
#[derive(Debug)]
pub(crate) enum Peer {
    /// 读到了这个会话发来的话、空了的通知：它回过话了（防刷屏，`outbox.rs`）。
    Heard(SessionId),
    /// `from` 等这个会话空下来，从 `since` 算起（L7 三，照 `from` 会话的钟）；`wall` 是订的那一刻的系统时间（LM-12）。
    Watch {
        from: SessionId,
        since: Timestamp,
        wall: Timestamp,
    },
    /// 等的 `from` 没等到空下来就关了（L7 三）。
    Closed { from: SessionId, since: Timestamp },
}

/// 一个主会话「空了告诉我」的两边（L7 三）：窗口拿着，actor 每批送完照内核看一次。放下（窗口关了）时计时撤掉，名单上在等
/// 这个会话的各回一封「关了」。
#[derive(Debug)]
pub(crate) struct Peers {
    /// `<数据>/sessions`。
    sessions: PathBuf,
    me: SessionId,
    /// 放进别的会话的收件目录以后敲它的门铃（LM-4）。
    bell: Bell,
    watches: watches::Watches,
    waiters: waiters::Waiters,
}

impl Peers {
    /// 会话 `me`，会话目录是 `dir`，门铃照 `bell`。
    pub(crate) fn new(dir: &Path, me: SessionId, bell: Bell) -> Peers {
        Peers {
            sessions: dir.parent().map(Path::to_path_buf).unwrap_or_default(),
            me,
            bell,
            watches: watches::Watches::default(),
            waiters: waiters::Waiters::default(),
        }
    }

    /// 交给 actor 了：这个会话这时忙不忙。
    pub(crate) fn start(&mut self, session: &Session) {
        self.waiters.start(session.vacant());
    }

    /// 一批送完了（Miyu `watchers.rs` 的 `after_batch`）：等的一边去订、计时（`now` 是会话的钟，和账本里订的时刻比），被等的
    /// 一边记忙闲、发通知（忙完的那一刻记此刻的系统时间，和登记里订的那一刻的系统时间比，LM-12）。
    pub(crate) fn after_batch(
        &mut self,
        session: &Session,
        now: Timestamp,
        backs: &mpsc::UnboundedSender<Back>,
    ) {
        self.watches.sync(
            session.watching(),
            session.watch_hours(),
            now,
            (&self.sessions, &self.me, &self.bell),
            backs,
        );
        let status = || session.last_line();
        let notice = waiters::Notice {
            me: &self.me,
            sessions: &self.sessions,
            bell: &self.bell,
            status: &status,
        };
        self.waiters
            .after_batch(session.vacant(), wall_now(), notice);
    }

    /// 会话 `watcher` 从 `since` 起等这个会话空下来，订在系统时间 `wall` 那一刻。
    pub(crate) fn watched_by(
        &mut self,
        watcher: SessionId,
        (since, wall): (Timestamp, Timestamp),
        session: &Session,
    ) {
        let status = || session.last_line();
        let notice = waiters::Notice {
            me: &self.me,
            sessions: &self.sessions,
            bell: &self.bell,
            status: &status,
        };
        self.waiters
            .add(watcher, (since, wall), session.vacant(), notice);
    }
}

impl Drop for Peers {
    fn drop(&mut self) {
        self.waiters.close((&self.sessions, &self.bell), &self.me);
    }
}

#[cfg(test)]
mod tests;
