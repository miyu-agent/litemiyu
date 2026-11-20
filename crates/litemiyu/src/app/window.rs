//! 正文只留视口上下几屏（红线 5，设计第四节第 4 条；界面线第十趟「做法」）：底下几屏（连同侧边栏「跳转消息」
//! 要的最近几句话）和在进行的那一段永远常驻；翻上去看着的地方上下各几屏读回来，离开两倍远的放掉。放掉的段只留槽
//! （`transcript/pieces.rs`），读回走日志：引擎照槽里的序号读那一截、照补发读成推送（`core/engine/readback.rs`），这里在一份
//! 草稿正文里照推送办出条目再放回去，和打开旧会话时补发办出来的一样。

use std::ops::RangeInclusive;

use super::App;
use crate::core::{Command, Push, Update};
use crate::transcript::{Kind, Transcript, pieces};
use crate::ui::estimate;
use crate::ui::row_cache::RowCache;

/// 视口上下各留几屏（第十趟「做法」第 6 条取 2）：逐格对照里只留几屏的那一份照它。
#[cfg(test)]
pub const SCREENS: usize = SHIPPED;

/// 发版的程序留几屏（第十二趟开：`/jump` 接上了、对照都过了）。
pub const SHIPPED: usize = 2;

/// 这一次留几屏：照 [`SHIPPED`]；环境变量 `LITEMIYU_KEEP_SCREENS` 写了数的照它（`all` 是全留），对照、查问题时用
/// （伪终端里同一份会话全留和只留几屏逐行比，`tests/pty/window.rs`）。单元测试里的界面没有引擎交读回，照全留、不看环境
/// 变量；要只留几屏的测试自己设 `keep_screens`（`window/tests`）。
pub fn shipped() -> usize {
    if cfg!(test) {
        return usize::MAX;
    }
    match std::env::var("LITEMIYU_KEEP_SCREENS")
        .ok()
        .as_deref()
        .map(str::trim)
    {
        Some("all") => usize::MAX,
        Some(n) => n.parse().ok().filter(|&n| n > 0).unwrap_or(SHIPPED),
        None => SHIPPED,
    }
}

impl App {
    /// 照上一帧的视口定哪几段常驻：要的、不在的向引擎要读回（排进 `wanted`，[`App::send_wanted`] 发出去），不要的放掉。
    /// 看着子会话时照它那一份正文办（读回照命令里的会话编号走它自己的日志，「做法」第 9 条）。
    pub(crate) fn keep_window(&mut self) {
        self.drop_listing();
        let keep = self.media.keep_screens;
        if keep == usize::MAX || self.talk.transcript.pieces().len() < 2 {
            return;
        }
        let height = usize::from(self.screen.view.area.height.max(1));
        let width = self.text_width();
        let rows = keep.saturating_mul(height);
        let tail = self.tail_from(rows, width);
        let near = self.near(rows, height);
        let far = self.near(rows.saturating_mul(2), height);
        if let Some(near) = near {
            self.want_all(near);
        }
        // `/jump` 开着、在等日志里你说的话：不放（列表照交来的那一截和常驻的段拼，`jump.rs`）。
        if !matches!(self.talk.listing, super::jump::Listing::None) {
            return;
        }
        let held = self.held(height);
        let going = self
            .talk
            .going
            .and_then(|seq| self.talk.transcript.piece_of(seq));
        let anchor = self.gap_anchor();
        // 只看有条目常驻的段（`entries` 照段排好）：一段段数全部的段，打开一万轮的会话时每开一段数一遍，是平方的。
        for p in self.talk.transcript.resident_pieces(tail) {
            let t = &self.talk.transcript;
            if !t.sheddable(p)
                || going == Some(p)
                || anchor == Some(p)
                || far.as_ref().is_some_and(|f| f.contains(&p))
                || held.as_ref().is_some_and(|h| h.contains(&p))
            {
                continue;
            }
            let (rows, chars) = piece_rows(t, &self.media.row_cache.borrow(), p, width);
            self.talk.transcript.shed(p, rows, chars, width);
        }
    }

    /// 要回 `pieces` 里放掉、还没要的段：连着的并成一串一起要（一万轮翻一趟不是一万条命令、一万个线程）。
    fn want_all(&mut self, pieces: RangeInclusive<usize>) {
        let mut run: Option<(usize, usize)> = None;
        for p in pieces {
            let t = self.talk.transcript.pieces();
            let missing = !t.resident(p) && !t.flag(p, pieces::ASKED);
            run = match (run, missing) {
                (Some((first, n)), true) if first + n == p => Some((first, n + 1)),
                (Some((first, n)), _) => {
                    self.want_run(first, n);
                    missing.then_some((p, 1))
                }
                (None, true) => Some((p, 1)),
                (None, false) => None,
            };
        }
        if let Some((first, n)) = run {
            self.want_run(first, n);
        }
    }

    /// 正文的字多宽：照上一帧；还没画过正文的照正文区减去两边的槽。
    fn text_width(&self) -> u16 {
        let view = &self.screen.view;
        match view.text_width {
            0 => view.area.width.saturating_sub(4).max(1),
            width => width,
        }
    }

    /// 排着的读回交给引擎线程。
    pub(crate) fn send_wanted(&mut self) {
        for command in std::mem::take(&mut self.talk.wanted) {
            self.link.core.send(command);
        }
    }

    /// 读回来的一串段（`Update::ReadBack`）：同一个会话的，在草稿正文里照推送办出条目放回去；草稿切出来的段数对不上的
    /// 一段段重要；读不回来的留着空档、不再要。在等着跳的那一句回来了，下一帧跳过去（`jump.rs`）。
    pub(super) fn read_back(&mut self, update: Update) {
        let Update::ReadBack {
            session,
            piece,
            count,
            pushes,
        } = update
        else {
            return;
        };
        if self.talk.transcript.session.as_deref() != Some(session.as_str()) {
            return;
        }
        let (piece, count) = (piece as usize, count as usize);
        let Ok(pushes) = pushes else {
            // 读不回来：在等着跳过去的那一句在这几段里的，不跳了（不然一直等着，按的键一直攒着）。
            let lost = self
                .talk
                .going
                .and_then(|seq| self.talk.transcript.piece_of(seq))
                .is_some_and(|p| (piece..piece + count).contains(&p));
            if lost {
                self.talk.going = None;
            }
            return;
        };
        let draft = replayed(pushes, self.config.text());
        if !self.talk.transcript.restore(piece, count, draft) {
            for p in piece..piece + count {
                self.talk.transcript.asked(p, false);
                self.want_run(p, 1);
            }
        }
        self.go_when_back();
    }

    /// 向引擎要从第 `first` 段起的 `count` 段：日志里从第一段开头那句话读到再下一段开头那句以前。序号还不知道的（没落盘）
    /// 不要。
    pub(super) fn want_run(&mut self, first: usize, count: usize) {
        let t = &self.talk.transcript;
        let end = first + count;
        let taken = |p| t.pieces().resident(p) || t.pieces().flag(p, pieces::ASKED);
        if (first..end).any(taken) {
            return;
        }
        let (Some(from), Some(next)) = (t.first_seq(first), t.first_seq(end)) else {
            return;
        };
        let (Ok(piece), Ok(count32)) = (u32::try_from(first), u32::try_from(count)) else {
            return;
        };
        let session = t.session.clone().unwrap_or_default();
        for p in first..end {
            self.talk.transcript.asked(p, true);
        }
        self.talk.wanted.push(Command::ReadBack {
            session,
            piece,
            count: count32,
            after: from.saturating_sub(1),
            upto: next.saturating_sub(1),
        });
    }

    /// 底下那几屏从第几段起：从最后一段往上数，够 `rows` 行、够侧边栏要的那几句话为止。
    fn tail_from(&self, rows: usize, width: u16) -> usize {
        let t = &self.talk.transcript;
        let cache = self.media.row_cache.borrow();
        let want = self.config.layout.sidebar_jumps;
        let (mut got, mut said) = (0usize, 0usize);
        let mut p = t.pieces().len();
        while p > 0 && (got < rows || said < want) {
            p -= 1;
            got += piece_rows(t, &cache, p, width).0 as usize;
            said += t.entries[t.span(p)]
                .iter()
                .filter(|e| e.kind == Kind::User && !e.queued && !e.hidden)
                .count();
        }
        p
    }

    /// 翻上去看着的：视口连同上下各 `margin` 行落在第几段到第几段；跟着最新的是 `None`。
    fn near(&self, margin: usize, height: usize) -> Option<RangeInclusive<usize>> {
        let view = &self.screen.view;
        view.top?;
        let last = self.talk.transcript.pieces().len().checked_sub(1)?;
        let from = self.piece_at(view.first.saturating_sub(margin))?;
        let to = self
            .piece_at(view.first.saturating_add(height).saturating_add(margin))
            .unwrap_or(last);
        Some(from..=to.max(from))
    }

    /// 视口顶上落在空档里时读回以后照哪一条找回视口（`ui/body.rs` 的 `gap_anchor`）：那一条所在的段不放，不然找不回来。
    fn gap_anchor(&self) -> Option<usize> {
        let view = &self.screen.view;
        let height = usize::from(view.area.height);
        let (id, _) = crate::ui::body::gap_anchor(&view.rows, view.top?, height, view.seen_side)?;
        let id = id?;
        let t = &self.talk.transcript;
        t.entries
            .iter()
            .find(|e| e.id == id)
            .map(|e| e.piece as usize)
    }

    /// 选区碰到的段：不放（复制要的是原文）。
    fn held(&self, height: usize) -> Option<RangeInclusive<usize>> {
        let (a, b) = self.screen.view.select?;
        let (lo, hi) = (a.0.min(b.0), a.0.max(b.0));
        let from = self.piece_at(lo)?;
        let to = self
            .piece_at(hi.saturating_add(height))
            .unwrap_or(usize::MAX);
        Some(from..=to)
    }

    /// 上一帧的第 `row` 行是第几段的：条目的照条目；放掉的段照那一串里往下数；条目间的空行照下面那一条。
    fn piece_at(&self, row: usize) -> Option<usize> {
        let t = &self.talk.transcript;
        let rows = &self.screen.view.rows;
        if let Some((first, mut offset)) = rows.gap_at(row) {
            let width = self.text_width();
            let mut p = first as usize;
            while let Some(slot) = t.pieces().get(p).filter(|_| !t.pieces().resident(p)) {
                let here = estimate::rewrap(slot.rows, slot.chars, slot.width, width) as usize;
                if offset < here {
                    return Some(p);
                }
                offset -= here;
                p += 1;
            }
            return Some(p.saturating_sub(1));
        }
        let id = rows.entry_at(row)?;
        t.entries
            .iter()
            .find(|e| e.id == id)
            .map(|e| e.piece as usize)
    }
}

/// 第 `p` 段画几行（每条看得见的条目连同前面的空行）、看得见的字多宽：常驻的照排好记着的行（没排过的照估的），放掉的照槽。
fn piece_rows(t: &Transcript, cache: &RowCache, p: usize, width: u16) -> (u32, u32) {
    if !t.pieces().resident(p) {
        return t.pieces().get(p).map_or((0, 0), |slot| {
            (
                estimate::rewrap(slot.rows, slot.chars, slot.width, width),
                slot.chars,
            )
        });
    }
    let (mut rows, mut chars) = (0u32, 0u32);
    for entry in t.entries[t.span(p)]
        .iter()
        .filter(|e| crate::ui::rows::shown(e))
    {
        let (guess, cols) = estimate::estimate(entry, width);
        let laid = cache
            .rows_of(entry.id)
            .and_then(|n| u32::try_from(n + 1).ok());
        rows = rows.saturating_add(laid.unwrap_or(guess));
        chars = chars.saturating_add(cols);
    }
    (rows, chars)
}

/// 照读回来的推送办出的草稿正文：同打开旧会话时补发的那一路（只是正文自己办的那几样；界面那一头写进正文的几样所在的段
/// 钉住了，不会放、不会读回）。
fn replayed(pushes: Vec<Push>, texts: &crate::config::Texts) -> Transcript {
    let mut draft = Transcript::default();
    for push in pushes {
        draft.update(Update::Push(push), texts);
    }
    draft
}

#[cfg(test)]
mod tests;
