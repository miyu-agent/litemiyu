//! 正文的钟（蓝图 `tui.md`「会话列表 `/sessions`」第 6 条）：看着流出来的照现在；补发来的照事件落盘的时刻
//! （[`Push::Clock`](crate::core::Push::Clock)），用时、时刻和当时一样。
//!
//! 事件的时刻是墙上的钟，用时是 [`Instant`]：补发开始时记下一对（这时的 `Instant`、这时的墙上时刻），事件的时刻
//! 往回倒推成 `Instant`，彼此的差和当时一样。

use std::time::Instant;

use jiff::{Timestamp, Zoned};

use super::{Kind, Transcript};

/// 补发时照的时刻。
#[derive(Debug, Default, Clone, Copy)]
pub struct Clock {
    /// 正在读的那条事件的时刻；看着流出来的是 `None`。
    at: Option<Timestamp>,
    /// 这一次补发开始时的那一对。
    anchor: Option<(Instant, Timestamp)>,
}

impl Clock {
    /// 现在（补发时是那条事件的时刻）。
    pub fn now(&self) -> Instant {
        let (Some(at), Some((base, wall))) = (self.at, self.anchor) else {
            return Instant::now();
        };
        let back = wall.duration_since(at);
        let gap = back.unsigned_abs();
        if back.is_negative() {
            base.checked_add(gap).unwrap_or(base)
        } else {
            base.checked_sub(gap).unwrap_or(base)
        }
    }

    /// 补发来的事件的时刻（`Push::Clock`）：补发开始时记下这一对；`None` 是补完了，回到照现在。
    pub fn set(&mut self, at: Option<Timestamp>) {
        match at {
            Some(at) => {
                self.anchor
                    .get_or_insert_with(|| (Instant::now(), Timestamp::now()));
                self.at = Some(at);
            }
            None => *self = Self::default(),
        }
    }

    /// 墙上的钟（收尾行写的时刻）。
    pub fn wall(&self) -> Zoned {
        self.at.map_or_else(crate::zone::now, |at| {
            at.to_zoned(crate::zone::local().clone())
        })
    }
}

impl Transcript {
    /// 别处来的话（别的会话发来的，Miyu 终端 `transcript/foreign.rs`）：来处那一句是 `from`，记下序号（它开的那一轮照它认）。
    pub fn foreign(&mut self, seq: u64, from: String, text: String) {
        self.push(Kind::User, text);
        if let Some(entry) = self.entries.last_mut() {
            entry.from = Some(from);
            entry.seq = Some(seq);
        }
    }

    /// 补发来的你说的话：照你说的画，记下序号（开轮时照它认，第 6 条）。
    pub(super) fn said(&mut self, seq: u64, text: String) {
        self.user(text, Vec::new());
        if let Some(entry) = self.entries.last_mut() {
            entry.seq = Some(seq);
        }
    }
}
