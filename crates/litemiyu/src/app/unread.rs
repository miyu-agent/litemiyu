//! 会话一时读不了（Miyu 终端 `app/unread.rs`，核心 YC-2；蓝图 `tui.md`「连核心」第 7 条下）：打开会话时载入、读日志读写出错，
//! 日志没坏（引擎的 `NotLoaded::Unavailable`）。弹一句「几秒后自动重试」，到点再开，最多试 [`TRIES`] 次，还不成的弹「请稍后重试」
//! 不再试；不说会话坏了，也不从列表里拿掉。
//!
//! 和 Miyu 不一样的：Miyu 切过去时正文先换上要开的那个，再试是叫核心再读（读页、订阅）；LiteMiyu 开不成时屏幕上照旧是原来那个，
//! 再试是再开一次（[`App::open_session`]）。「还对着它」照这个换成：失败那一刻在看的会话没换、也没在开别的。日志中间坏了一行
//! （只能看）那一种在 `sessions.rs` 收 `ReadOnly`。

use std::time::{Duration, Instant};

use super::App;
use super::sessions::Opening;

/// 一时读不了以后隔多久再试（Miyu 2026-11-10，YC-12：读写出错多半是打开文件数一下子用光这类，几秒就过去）。
pub const WAIT: Duration = Duration::from_secs(5);

/// 最多自动再试几次（一共等半分钟）。
pub const TRIES: u32 = 6;

/// 等着再试的那一个会话。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reread {
    /// 哪个会话。
    pub session: String,
    /// 读不了那一刻在看的会话（新会话还没说话的是 `None`）：换了就不试了。
    pub from: Option<String>,
    /// 什么时候再试；试过了（`asked`）的是发出去的那一刻。
    pub due: Instant,
    /// 已经发出去再试了，等引擎回。
    pub asked: bool,
    /// 已经试了几次。
    pub tries: u32,
}

impl App {
    /// 会话一时读不了：记着到点再试、弹一句；同一个会话接着读不了的数着次数，到了 [`TRIES`] 次不再试（Miyu 同名函数）。
    pub(super) fn session_unavailable(&mut self, session: String) {
        let now = Instant::now();
        // 刚再试过、又读不了的接着数；隔了好一阵（上一回试成了以后又出的事）从头数。
        let tries = match self.talk.reread.take() {
            Some(before)
                if before.session == session
                    && before.asked
                    && now.duration_since(before.due) < WAIT * 2 =>
            {
                before.tries
            }
            _ => 0,
        };
        let texts = self.config.text();
        if tries >= TRIES {
            self.hint(texts.sessions.unavailable_gave_up.clone(), false);
            return;
        }
        let seconds = WAIT.as_secs();
        let forms = &texts.retry_after.seconds;
        let form = if seconds == 1 { &forms[0] } else { &forms[1] };
        let wait = form.replace("{n}", &seconds.to_string());
        let text = texts.sessions.unavailable_retry.replace("{wait}", &wait);
        self.talk.reread = Some(Reread {
            session,
            from: self.talk.transcript.session.clone(),
            due: now + WAIT,
            asked: false,
            tries,
        });
        self.hint(text, false);
    }

    /// 到点了再试：还对着它（失败那一刻在看的会话没换、没在开别的）的再开一次，切走了的不试了（`tick` 里叫，Miyu 同名函数）。
    pub(super) fn reread_due(&mut self, now: Instant) {
        let Some(due) = self
            .talk
            .reread
            .as_ref()
            .filter(|r| !r.asked && r.due <= now)
        else {
            return;
        };
        let still = self.talk.transcript.session == due.from && self.talk.opening == Opening::None;
        let Some(mut next) = self.talk.reread.take().filter(|_| still) else {
            return;
        };
        next.tries += 1;
        // 再试成了照常画；又读不了的引擎再回一次，[`App::session_unavailable`] 照发出去的时刻接着数。
        next.asked = true;
        next.due = now;
        let session = next.session.clone();
        self.talk.reread = Some(next);
        self.open_session(session);
    }

    /// 下一次要醒来再试的时刻（`deadline.rs`）。
    pub(super) fn reread_deadline(&self) -> Option<Instant> {
        self.talk
            .reread
            .as_ref()
            .filter(|r| !r.asked)
            .map(|r| r.due)
    }
}
