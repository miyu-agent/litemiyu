//! `/jump`（蓝图「跳到说过的话」）：和输入历史列表同一个列表，列这个会话正文里你说的话；选中的那条顶到视口第一行，竖线
//! 亮一下。
//!
//! 正文只留几屏（红线 5，`window.rs`）以后，放掉的段里你说的话界面这头没有了：开 `/jump` 时有放掉的段，先让引擎在读完就退
//! 的线程里把日志里你说的话读一遍交来（[`Command::Spoken`]，`transcript/said.rs`），到了再开列表；开着时不放段，常驻的段照
//! 正文列、放掉的照交来的列，关了就扔。跳到放掉的段里的一句：先读回那一段，到了、排好了再跳（`ui/body.rs` 的 `goto`）。
//!
//! 全留的这两处都是当场的。只留几屏时在等的这一段里按的键、粘贴先攒着（[`Held`]），等完了照先后补办，落点和全留的一样
//! （LM-15：原来落进输入框，按 `↑` 翻出输入历史里的 `/jump`，列表开了以后选中的差一句）。

use std::borrow::Cow;
use std::collections::VecDeque;
use std::time::{Duration, Instant, SystemTime};

use crossterm::event::{Event, KeyCode, KeyEventKind};

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
    /// 要了（哪个会话的），到了就开 `/jump`。
    Waiting(String),
    /// 到了（`/jump` 开着）：照先后。
    Ready(Vec<Said>),
}

/// 攒着的按键最多扣多久：日志里你说的话、要跳的那一段一直不来的（引擎那一头没回话），从攒下头一个起到点就不等了，照读
/// 不了办，攒着的照先后补办。
pub const HOLD: Duration = Duration::from_secs(5);

/// 在等的时候攒着的按键、粘贴（LM-15）。
#[derive(Debug, Default)]
pub struct Held {
    /// 照先后。
    events: VecDeque<Event>,
    /// 攒下头一个（补办到一半又要等的，是又等起的那一刻）：[`HOLD`] 从这里算；没攒着的是 `None`。
    pub since: Option<Instant>,
}

impl Held {
    /// 攒着几个。
    #[cfg(test)]
    pub fn count(&self) -> usize {
        self.events.len()
    }

    /// 攒着的里面有全留时会关掉列表、换了列表的样子的键：回车（挑中、跳过去），按键模式是 Vim 时的 `/`（进搜索，之后的
    /// `Esc` 只退出搜索）。这时来的 `Esc` 也攒着，照先后办才和全留的一样。
    fn leaves_list(&self, vim: bool) -> bool {
        self.events.iter().any(|event| {
            matches!(event, Event::Key(key) if key.kind != KeyEventKind::Release
                && (key.code == KeyCode::Enter || (vim && key.code == KeyCode::Char('/'))))
        })
    }
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
            // 同读回一样排进 `wanted`，画完这一帧交给引擎（`window.rs` 的 `send_wanted`）。
            let session = self.talk.transcript.session.clone().unwrap_or_default();
            self.talk.listing = Listing::Waiting(session.clone());
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

    /// 日志里你说的话到了（`Update::Spoken`）：在等的、同一个会话的，开列表；读不了的照正文开。等的时候切走了（看着的
    /// 不是要的那个会话了）的不开。攒着的键接着补办。
    pub(super) fn spoken_arrived(&mut self, update: Update) {
        let Update::Spoken { session, said } = update else {
            return;
        };
        if !matches!(&self.talk.listing, Listing::Waiting(asked) if *asked == session) {
            return;
        }
        if self.talk.transcript.session.as_deref() != Some(session.as_str()) {
            self.talk.listing = Listing::None;
            return self.release_held();
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
        self.release_held();
    }

    /// 在等：`/jump` 回车了、日志里你说的话还没到；跳到放掉的段里的一句，那一段还没读回、还没排好跳过去。
    fn waiting(&self) -> bool {
        matches!(self.talk.listing, Listing::Waiting(_))
            || self.talk.going.is_some()
            || self.screen.view.goto.is_some()
    }

    /// 终端来的一件事（[`App::handle`]）：在等、或者还有攒着没补办完的，按键、粘贴攒着，交回 `None`。等列表时按 `Esc`
    /// 当场取消：不等了、不开列表，攒着的扔掉（全留的这时列表开着，攒着的键落在列表上，`Esc` 关了列表就都没了）；攒着的
    /// 里面有回车的除外（[`Held::leaves_list`]）。鼠标、窗口改大小这些当场办：点的是眼前这一帧画的东西。
    pub(super) fn hold(&mut self, event: Event) -> Option<Event> {
        let keyed = matches!(event, Event::Key(_) | Event::Paste(_));
        if !keyed || (!self.waiting() && self.talk.held.events.is_empty()) {
            return Some(event);
        }
        if let Event::Key(key) = &event
            && key.code == KeyCode::Esc
            && key.kind != KeyEventKind::Release
            && matches!(self.talk.listing, Listing::Waiting(_))
            && !self.talk.held.leaves_list(self.config.layout.vim)
        {
            self.talk.listing = Listing::None;
            self.talk.held = Held::default();
            // 这一下用掉了：上一下 `Esc` 不算了（LM-38）。
            self.typing.esc_at = None;
            return None;
        }
        let held = &mut self.talk.held;
        held.since.get_or_insert_with(Instant::now);
        held.events.push_back(event);
        None
    }

    /// 不等了：攒着的照先后办，办到又要等的停下（补办的回车又开了 `/jump`、跳到放掉的段里的一句），剩下的接着攒；翻了
    /// 一下页的也停下，画完这一帧接着办。日志里你说的话到了、每一帧画以前（`main.rs`）、到点不等了都调。
    pub(crate) fn release_held(&mut self) {
        let mut done = false;
        while !self.waiting() {
            let Some(event) = self.talk.held.events.pop_front() else {
                break;
            };
            // 翻页照上一帧画出来的第一行算：翻了一下的，画完这一帧再接着办（全留的每下各画一帧）。
            let paged = matches!(&event, Event::Key(key) if key.kind != KeyEventKind::Release
                && matches!(key.code, KeyCode::PageUp | KeyCode::PageDown));
            self.handle_now(event);
            done = true;
            if paged {
                break;
            }
        }
        let held = &mut self.talk.held;
        if held.events.is_empty() {
            held.since = None;
        } else if done {
            held.since = Some(Instant::now());
        }
    }

    /// 不在等了、还有攒着没补办完的（补办到翻页停下，等画完这一帧）：主循环当场醒来接着办。
    pub(super) fn held_due(&self) -> Option<Instant> {
        (!self.waiting() && !self.talk.held.events.is_empty()).then(Instant::now)
    }

    /// 攒着的扣满 [`HOLD`] 了：不等了。在等日志里你说的话的照读不了办（照正文开列表），在等跳过去的不跳了；攒着的照先后
    /// 补办。
    pub(super) fn held_overdue(&mut self, now: Instant) {
        if !self
            .talk
            .held
            .since
            .is_some_and(|since| now >= since + HOLD)
        {
            return;
        }
        if let Listing::Waiting(asked) = &self.talk.listing {
            let same = self.talk.transcript.session.as_deref() == Some(asked.as_str());
            self.talk.listing = Listing::None;
            if same {
                self.show_jump(spoken(&self.talk.transcript.entries).is_empty());
            }
        }
        self.talk.going = None;
        self.screen.view.goto = None;
        self.release_held();
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
                    // 找不到它在哪一段的没法读回，不等（不然按的键一直攒着）。
                    if let Some(p) = self.talk.transcript.piece_of(seq) {
                        self.want_run(p, 1);
                        self.talk.going = Some(seq);
                    }
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
