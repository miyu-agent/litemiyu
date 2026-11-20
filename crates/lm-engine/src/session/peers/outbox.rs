//! 发给别的窗口的话（L7 二）：写进对方的收件目录（[`super::letter::post`]），对方开没开着都一样，没开着的下次打开先读。
//!
//! 防刷屏在发的一方当场拦（定的做法，和 Miyu 不同）：Miyu 是收的一方的内核照账本拒、核心当场把原因回给发的一方；LiteMiyu
//! 没有共同的核心，收的一方拒了发的一方不知道。所以这里照 Miyu 同样的数（[`PEERS`]：同一个对方 600 秒里 5 句、一字不差的
//! 不发、50 句没回）和同样的原因（[`NotSent`] 的三种，`send_message` 照它说 Miyu 那几句）自己数：
//!
//! - 一字不差（`duplicate`）：600 秒里给它发过一字不差的一句。先查，不占数。
//! - 太多（`too-many`）：600 秒里、它上一次回话以后，给它发了 5 句。
//! - 没读（`inbox-full`）：它上一次回话以后给它发了 50 句。Miyu 数的是对方还没听到的；发的一方看不到，照「它回过话就算
//!   读过了」数。
//!
//! 「回话」是收件目录里读到它发来的话、空了的通知（[`Outbox::heard`]）。数只在内存里，窗口关了清零（Miyu 照日志算回来）。
//! 一个主会话一份，用到才有项。

use std::collections::{BTreeMap, VecDeque};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::PathBuf;
use std::sync::{Mutex, PoisonError};

use lm_kernel::kernel::id::{CallId, SessionId};
use lm_kernel::policy::PEERS;

use super::letter::{self, Letter};
use crate::session::blocking::blocking;
use crate::session::clock::wall_now;
use crate::store::paths::Bell;
use crate::tool::{Delivered, NotSent};

/// 一个主会话发给别的窗口的话，和它给每个对方发过的账。
pub(crate) struct Outbox {
    /// `<数据>/sessions`。
    sessions: PathBuf,
    /// 这个会话。
    me: SessionId,
    /// 放进对方收件目录以后敲它的门铃（LM-4）。
    bell: Bell,
    sent: Mutex<BTreeMap<SessionId, Sent>>,
}

/// 给一个对方发过的：窗口里的几句（时刻、字的哈希），它上一次回话的时刻，那以后发了几句。
#[derive(Default)]
struct Sent {
    recent: VecDeque<(i64, u64)>,
    replied: i64,
    unanswered: u64,
}

impl Outbox {
    /// 会话 `me` 的，会话都在 `sessions` 下，门铃照 `bell`。
    pub(crate) fn new(sessions: PathBuf, me: SessionId, bell: Bell) -> Outbox {
        Outbox {
            sessions,
            me,
            bell,
            sent: Mutex::new(BTreeMap::new()),
        }
    }

    /// 第 `call` 次调用把 `text` 发给 `to`：过了防刷屏，写进它的收件目录。写不进去（盘满、它的会话目录不在了）的照送不到。
    pub(crate) async fn send(
        &self,
        to: SessionId,
        call: CallId,
        text: &str,
    ) -> Result<Delivered, NotSent> {
        let now = wall_now().unix_millis();
        let digest = digest(text);
        self.check(&to, now, digest)?;
        let letter = Letter::Message {
            from: self.me.clone(),
            call,
            text: text.to_string(),
        };
        let (sessions, target, bell) = (self.sessions.clone(), to.clone(), self.bell.clone());
        match blocking(move || letter::post(&sessions, &target, &letter, &bell)).await {
            Ok(()) => {
                self.record(to, now, digest);
                Ok(Delivered::Sent)
            }
            Err(_) => Err(NotSent::Undelivered),
        }
    }

    /// 读到了 `from` 发来的话、空了的通知：它回过话了，限速、没读的数从这以后算。
    pub(crate) fn heard(&self, from: &SessionId) {
        let mut sent = self.sent.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(sent) = sent.get_mut(from) {
            sent.replied = wall_now().unix_millis();
            sent.unanswered = 0;
        }
    }

    /// 防刷屏（照 Miyu 内核的 `flooding` 的先后）：先查一字不差的，再查限速，最后查没读的。
    fn check(&self, to: &SessionId, now: i64, digest: u64) -> Result<(), NotSent> {
        let mut all = self.sent.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(sent) = all.get_mut(to) else {
            return Ok(());
        };
        let window = i64::try_from(PEERS.window.saturating_mul(1000)).unwrap_or(i64::MAX);
        let since = now.saturating_sub(window);
        // 窗口以前的不再要：一字不差、限速都只看窗口里的。
        while sent.recent.front().is_some_and(|(at, _)| *at < since) {
            sent.recent.pop_front();
        }
        if sent.recent.iter().any(|(_, said)| *said == digest) {
            return Err(NotSent::Duplicate);
        }
        let replied = sent.replied;
        let recent = sent.recent.iter().filter(|(at, _)| *at > replied).count();
        if u64::try_from(recent).unwrap_or(u64::MAX) >= PEERS.burst {
            return Err(NotSent::TooMany);
        }
        if sent.unanswered >= PEERS.unread {
            return Err(NotSent::InboxFull);
        }
        Ok(())
    }

    /// 发出去了一句：记下。
    fn record(&self, to: SessionId, now: i64, digest: u64) {
        let mut all = self.sent.lock().unwrap_or_else(PoisonError::into_inner);
        let sent = all.entry(to).or_default();
        sent.recent.push_back((now, digest));
        sent.unanswered = sent.unanswered.saturating_add(1);
    }
}

/// 一句话的字的哈希：只在这个进程里比一字不差，不落盘。
fn digest(text: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}
