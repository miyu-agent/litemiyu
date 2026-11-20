//! `/jump`（蓝图「跳到说过的话」）：和输入历史列表同一个列表，列这个会话正文里你说的话；选中的那条顶到视口第一行，竖线
//! 亮一下。
//!
//! 正文只留几屏（红线 5，`window.rs`）以后，放掉的段里你说的话界面这头没有了：开 `/jump` 时有放掉的段，先让引擎在读完就退
//! 的线程里把日志里你说的话读一遍交来（[`Command::Spoken`]，`transcript/said.rs`），到了再开列表；开着时不放段，常驻的段照
//! 正文列、放掉的照交来的列，关了就扔。跳到放掉的段里的一句：先读回那一段，到了、排好了再跳（`ui/body.rs` 的 `goto`）。

use std::borrow::Cow;
use std::time::{Duration, Instant, SystemTime};

use super::App;
use crate::core::{Command, Update};
use crate::input::{Draft, Sent};
use crate::transcript::{Entry, Kind, Said};

/// 列表里的一条跳到哪：常驻的条目（编号），或者放掉的段里的一句（序号）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spot {
    /// 常驻的条目。
    Entry(u64),
    /// 放掉的段里序号是这个的那一句。
    Said(u64),
}

/// 日志里你说的话：没要、要了在等、到了。
#[derive(Debug, Default)]
pub enum Listing {
    /// 没要。
    #[default]
    None,
    /// 要了，到了就开 `/jump`。
    Waiting,
    /// 到了（`/jump` 开着）：照先后。
    Ready(Vec<Said>),
}

impl App {
    /// 列表列的那一份，照先后：输入历史；开的是 `/jump` 时是这个会话里你说的话，另交每一句跳到哪（和前一份一一对应）。
    pub fn list_items(&self) -> (Cow<'_, [Sent]>, Vec<Spot>) {
        if !self.typing.history.jump {
            return (Cow::Borrowed(self.typing.input.sent()), Vec::new());
        }
        let (sent, spots) = match &self.talk.listing {
            Listing::Ready(said) => self.merged(said),
            _ => spoken(&self.talk.transcript.entries),
        }
        .into_iter()
        .unzip();
        (Cow::Owned(sent), spots)
    }

    /// 常驻的段照正文列、放掉的照日志里读来的列，照段的先后。
    fn merged(&self, said: &[Said]) -> Vec<(Sent, Spot)> {
        let t = &self.talk.transcript;
        let mut out = Vec::new();
        for p in 0..t.pieces().len() {
            if t.pieces().resident(p) {
                out.extend(spoken(&t.entries[t.span(p)]));
                continue;
            }
            let from = t.first_seq(p).unwrap_or(0);
            let to = t.first_seq(p + 1).unwrap_or(u64::MAX);
            let start = said.partition_point(|s| s.seq < from);
            let end = said.partition_point(|s| s.seq < to);
            out.extend(said[start..end.max(start)].iter().map(|s| {
                let sent = Sent {
                    draft: Draft::plain(&s.text),
                    at: s.at.map_or_else(SystemTime::now, SystemTime::from),
                };
                (sent, Spot::Said(s.seq))
            }));
        }
        out
    }

    /// `/jump`：有放掉的段的，先要日志里你说的话，到了再开；没有的照正文。这个会话里还没说过话的提示一句，不开（「跳到说过
    /// 的话」第 4 条）。
    pub(super) fn open_jump(&mut self) {
        let t = &self.talk.transcript;
        let shed = (0..t.pieces().len()).rfind(|&p| !t.pieces().resident(p));
        if let Some(upto) = shed.and_then(|p| t.first_seq(p + 1)) {
            let texts = Box::new(self.config.text().clone());
            self.talk.listing = Listing::Waiting;
            // 同读回一样排进 `wanted`，画完这一帧交给引擎（`window.rs` 的 `send_wanted`）。
            let session = self.talk.transcript.session.clone().unwrap_or_default();
            self.talk.wanted.push(Command::Spoken {
                session,
                upto: upto.saturating_sub(1),
                texts,
            });
            return;
        }
        self.show_jump(spoken(&self.talk.transcript.entries).is_empty());
    }

    /// 开 `/jump` 的列表；`empty` 的提示一句不开。
    fn show_jump(&mut self, empty: bool) {
        if empty {
            let note = self.config.text().jump.none.clone();
            self.hint(note, false);
        } else {
            self.typing.history.open_jump();
        }
    }

    /// 日志里你说的话到了（`Update::Spoken`）：在等的、同一个会话的，开列表；读不了的照正文开。
    pub(super) fn spoken_arrived(&mut self, update: Update) {
        let Update::Spoken { session, said } = update else {
            return;
        };
        let same = self.talk.transcript.session.as_deref() == Some(session.as_str());
        if !same || !matches!(self.talk.listing, Listing::Waiting) {
            return;
        }
        match said {
            Ok(said) => {
                let empty = self.merged(&said).is_empty();
                if !empty {
                    self.talk.listing = Listing::Ready(said);
                }
                self.show_jump(empty);
            }
            Err(_) => self.show_jump(spoken(&self.talk.transcript.entries).is_empty()),
        }
        if !matches!(self.talk.listing, Listing::Ready(_)) {
            self.talk.listing = Listing::None;
        }
    }

    /// `/jump` 关了：读来的扔掉。
    pub(super) fn drop_listing(&mut self) {
        let open = self.typing.history.open && self.typing.history.jump;
        if !open && matches!(self.talk.listing, Listing::Ready(_)) {
            self.talk.listing = Listing::None;
        }
    }

    /// 跳到 `spot`：常驻的照上一帧排好的行跳；放掉的段里的一句先读回那一段，到了、排好了再跳。
    pub(super) fn go(&mut self, spot: Spot) {
        match spot {
            Spot::Entry(id) => self.jump_to(id),
            Spot::Said(seq) => match self.said_entry(seq) {
                Some(id) => self.jump_to(id),
                None => {
                    if let Some(p) = self.talk.transcript.piece_of(seq) {
                        self.want_run(p, 1);
                    }
                    self.talk.going = Some(seq);
                }
            },
        }
    }

    /// 常驻的条目里序号是 `seq` 的那一句你说的话。
    fn said_entry(&self, seq: u64) -> Option<u64> {
        self.talk
            .transcript
            .entries
            .iter()
            .find(|e| e.kind == Kind::User && e.seq == Some(seq))
            .map(|e| e.id)
    }

    /// 读回来以后：在等着跳的那一句常驻了，下一帧排好了就跳（`BodyView::goto`）。
    pub(super) fn go_when_back(&mut self) {
        let Some(seq) = self.talk.going else {
            return;
        };
        if let Some(id) = self.said_entry(seq) {
            self.talk.going = None;
            self.screen.view.goto = Some((id, self.flash_until()));
        }
    }

    /// 跳到条目 `id`：照上一帧排好的行找它从第几行起，顶到视口第一行，竖线亮一下（第 3 条）。
    pub(super) fn jump_to(&mut self, id: u64) {
        let Some(row) = self.screen.view.rows.start_of(id) else {
            return;
        };
        let until = self.flash_until();
        self.screen.view.jump_to(row, id, until);
    }

    /// 跳到的那一条亮到几时。
    fn flash_until(&self) -> Instant {
        Instant::now() + Duration::from_millis(self.config.layout.jump_flash_ms)
    }
}

impl App {
    /// 点了侧边栏「跳转消息」第 `row` 行（Miyu 排期第 15 条）：跳到那一条，同 `/jump` 选它。
    pub(super) fn jump_clicked(&mut self, row: u16) {
        if let Some(id) = self.screen.jumps.get(usize::from(row)).copied() {
            self.jump_to(id);
        }
    }
}

/// 正文里你说的话（别处来的也算，撤掉的不算），照先后：列表的一条和它跳到哪。
fn spoken(entries: &[Entry]) -> Vec<(Sent, Spot)> {
    entries
        .iter()
        .filter(|e| e.kind == Kind::User && !e.hidden)
        .map(|e| {
            let at = e.at.map_or_else(SystemTime::now, SystemTime::from);
            let sent = Sent {
                draft: Draft::plain(&e.listed()),
                at,
            };
            (sent, Spot::Entry(e.id))
        })
        .collect()
}

#[cfg(test)]
mod tests;
