//! 正文按段记的小账（红线 5，设计第四节第 4 条「界面排好的行只留视口上下各几屏」）：一段从人在闲着时说的一句话起，到下一句
//! 这样的话前。切在闲着的地方（没在跑、没在压、没有排着的话、说过的话都开过轮了），一段自己从日志补发一遍就和整份补发里
//! 那一段一样，所以放掉的段只留一个槽（16 字节），要看时照槽里的序号从日志读回来（`app/window.rs`）。
//!
//! 常驻的段的条目在 [`Transcript::entries`] 里，照段的先后，每条记着自己是第几段（`Entry::piece`）；放掉的段只有槽。

use super::{Entry, Kind, Slot as Block, Transcript};

/// 一段的槽。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Slot {
    /// 这一段在日志里第一条事件的序号：开这一段的那句话落盘的序号。第 0 段（第一句话以前）是 0；放掉以前才记。
    pub seq: u32,
    /// 放掉时这一段画几行（每一条看得见的条目连同它前面的空行）；常驻的不记。
    pub rows: u32,
    /// 放掉时这一段看得见的字数：换了宽以后照它重估行数。
    pub chars: u32,
    /// `rows` 是照多宽量的；照估的是 0。
    pub width: u16,
    /// 记号：[`RESIDENT`]、[`PINNED`]、[`ASKED`]。
    pub flags: u16,
}

/// 条目在 `entries` 里。
pub const RESIDENT: u16 = 1;
/// 钉住，不放：里面有日志里没有的条目（只在内存里的旁白、界面那一头写的几样），或者人点过展开、收起。
pub const PINNED: u16 = 2;
/// 放掉了、已经向引擎要了读回，还没到。
pub const ASKED: u16 = 4;

/// 全部的段。
#[derive(Debug, Default)]
pub struct Pieces {
    slots: Vec<Slot>,
}

impl Pieces {
    /// 一共几段；还没有一条正文时是 0。
    pub fn len(&self) -> usize {
        self.slots.len()
    }

    /// 第 `p` 段的槽。
    pub fn get(&self, p: usize) -> Option<Slot> {
        self.slots.get(p).copied()
    }

    /// 第 `p` 段在不在 `entries` 里。
    pub fn resident(&self, p: usize) -> bool {
        self.flag(p, RESIDENT)
    }

    /// 第 `p` 段有没有记号 `flag`。
    pub fn flag(&self, p: usize, flag: u16) -> bool {
        self.slots.get(p).is_some_and(|s| s.flags & flag != 0)
    }

    /// 给第 `p` 段打上、去掉记号。
    fn set(&mut self, p: usize, flag: u16, on: bool) {
        if let Some(slot) = self.slots.get_mut(p) {
            match on {
                true => slot.flags |= flag,
                false => slot.flags &= !flag,
            }
        }
    }

    /// 最后一段是第几段：新的条目都记在它名下。还没有段时开第 0 段。
    fn last(&mut self) -> u32 {
        if self.slots.is_empty() {
            self.open();
        }
        index(self.slots.len() - 1)
    }

    /// 开新的一段（常驻）。
    fn open(&mut self) {
        self.slots.push(Slot {
            flags: RESIDENT,
            ..Slot::default()
        });
    }
}

fn index(p: usize) -> u32 {
    u32::try_from(p).unwrap_or(u32::MAX)
}

impl Transcript {
    /// 全部的段。
    pub fn pieces(&self) -> &Pieces {
        &self.pieces
    }

    /// 记一条进正文末尾，归最后一段；`opens` 的先开新的一段（闲着时说的话）。
    pub(super) fn place(&mut self, mut entry: Entry) {
        if entry.kind == Kind::User && self.settled() {
            self.pieces.open();
        }
        entry.piece = self.pieces.last();
        self.entries.push(entry);
    }

    /// 能在这里切一段：没在跑、没在压、没在接的块、没有排着的话，说过的话（`!` 跑的命令以外）都开过轮了。
    fn settled(&self) -> bool {
        self.running.is_none()
            && self.compacting.is_none()
            && self.blocks.is_empty()
            && !self.entries.iter().any(|e| {
                e.queued || (e.kind == Kind::User && e.turn.is_none() && e.shell.is_none())
            })
    }

    /// 第 `p` 段的条目在 `entries` 里从第几条到第几条（不含）。
    pub fn span(&self, p: usize) -> std::ops::Range<usize> {
        let p = index(p);
        let start = self.entries.partition_point(|e| e.piece < p);
        let end = start + self.entries[start..].partition_point(|e| e.piece == p);
        start..end
    }

    /// 有条目常驻的段（第 `upto` 段以前的），照先后：`entries` 照段排好，挨着的同一段只算一次。
    pub fn resident_pieces(&self, upto: usize) -> Vec<usize> {
        let mut out: Vec<usize> = Vec::new();
        for entry in &self.entries {
            let p = entry.piece as usize;
            if p >= upto {
                break;
            }
            if out.last() != Some(&p) {
                out.push(p);
            }
        }
        out
    }

    /// 第 `p` 段能不能放：常驻、没钉住、不是最后一段、开它的那句话落了盘（有序号），下一段开头那句也落了盘（读回要知道读到哪）。
    /// 开头的序号放不进槽（`u32`，一份会话要过 42 亿条事件）的不放：放了就记不住从哪读回（审计 #062）。
    pub fn sheddable(&self, p: usize) -> bool {
        let next = p + 1;
        self.pieces.resident(p)
            && !self.pieces.flag(p, PINNED)
            && next < self.pieces.len()
            && self
                .first_seq(p)
                .is_some_and(|seq| u32::try_from(seq).is_ok())
            && self.first_seq(next).is_some()
    }

    /// 第 `p` 段开头那句话的序号：第 0 段是 0；放掉了的照槽；常驻的照它第一条（还没落盘的没有）。
    pub fn first_seq(&self, p: usize) -> Option<u64> {
        if p == 0 {
            return Some(0);
        }
        if !self.pieces.resident(p) {
            return self.pieces.get(p).map(|s| u64::from(s.seq));
        }
        let first = self.span(p).next()?;
        self.entries[first].seq
    }

    /// 放掉第 `p` 段：条目拿走，记下行数、字数、照多宽量的（`width` 是 0 的是估的）。记着的下标跟着往前挪。
    pub fn shed(&mut self, p: usize, rows: u32, chars: u32, width: u16) {
        if !self.sheddable(p) {
            return;
        }
        let seq = self
            .first_seq(p)
            .and_then(|s| u32::try_from(s).ok())
            .unwrap_or(0);
        let span = self.span(p);
        let gone = span.len();
        // 闲着的段里的调用早有结果了，结果不会再来：记着的调用编号一起丢（不丢的话一万轮的会话记着两万个）。
        self.calls.retain(|_, (i, _)| !span.contains(i));
        // 放掉的段里你说的话带的图片照旧算数：输入框里新贴的接着它们编号（`chips.rs` 的 `image_count`）。
        self.shed_images += images_in(&self.entries[span.clone()]);
        self.entries.drain(span.clone());
        self.shift(span.start, -isize::try_from(gone).unwrap_or(0));
        self.reshaped += 1;
        if let Some(slot) = self.pieces.slots.get_mut(p) {
            *slot = Slot {
                seq,
                rows,
                chars,
                width,
                flags: slot.flags & !RESIDENT & !ASKED,
            };
        }
    }

    /// 读回来的从第 `first` 段起的 `count` 段（引擎照日志补发、草稿里办好的条目）放回去：草稿照同一个规矩切出的第 `k` 段是
    /// 这里的第 `first + k` 段（只要一段的整份草稿都归它）；换上这一份的编号，插在该在的地方，记着的下标跟着往后挪。
    /// 草稿切出来的段数对不上的不放，交回假（界面一段段重要）。
    pub fn restore(&mut self, first: usize, count: usize, mut draft: Transcript) -> bool {
        if count != 1 && draft.pieces.len() != count {
            return false;
        }
        let mut runs: Vec<Vec<Entry>> = (0..count).map(|_| Vec::new()).collect();
        for entry in std::mem::take(&mut draft.entries) {
            let k = if count == 1 { 0 } else { entry.piece as usize };
            if let Some(run) = runs.get_mut(k) {
                run.push(entry);
            }
        }
        for (k, mut mine) in runs.into_iter().enumerate() {
            let p = first + k;
            if self.pieces.resident(p) || p >= self.pieces.len() {
                continue;
            }
            self.settle_undone(&mut mine);
            let at = self.entries.partition_point(|e| e.piece < index(p));
            for entry in &mut mine {
                entry.id = self.fresh_id();
                entry.piece = index(p);
            }
            let added = mine.len();
            self.shed_images = self.shed_images.saturating_sub(images_in(&mine));
            self.entries.splice(at..at, mine);
            self.shift(at, isize::try_from(added).unwrap_or(0));
            self.pieces.set(p, RESIDENT, true);
            self.pieces.set(p, ASKED, false);
        }
        self.reshaped += 1;
        true
    }

    /// 序号是 `seq` 的那一条在第几段：开头那句话的序号不比它大的最后一段。
    pub fn piece_of(&self, seq: u64) -> Option<usize> {
        let mut lo = 0;
        let mut hi = self.pieces.len();
        while lo < hi {
            let mid = (lo + hi) / 2;
            match self.first_seq(mid) {
                Some(s) if s <= seq => lo = mid + 1,
                _ => hi = mid,
            }
        }
        lo.checked_sub(1)
    }

    /// 钉住第 `p` 段，不放（只在内存里的条目、人点过的）。
    pub fn pin(&mut self, p: usize) {
        self.pieces.set(p, PINNED, true);
    }

    /// 钉住最后一段（刚写进去的条目日志里没有，读回来就没了）。
    pub fn pin_last(&mut self) {
        if let Some(p) = self.pieces.len().checked_sub(1) {
            self.pin(p);
        }
    }

    /// 要了还没到的读回都不算要过（这一份正文停放起来了，到了的读回会话编号对不上、不要；换回来时照视口重要）。
    pub fn forget_asked(&mut self) {
        for p in 0..self.pieces.len() {
            self.pieces.set(p, ASKED, false);
        }
    }

    /// 向引擎要了第 `p` 段的读回（没到以前不再要）；`false` 是读不回来、不再要的记号去掉。
    pub fn asked(&mut self, p: usize, on: bool) {
        self.pieces.set(p, ASKED, on);
    }

    /// 第 `at` 条起拿走（`by` 是负的）或者插进了几条：记着的下标（在接的块、还没配上编号的调用、调用编号到那一步、
    /// 正在压缩那一行）在它后面的跟着挪。
    fn shift(&mut self, at: usize, by: isize) {
        let moved = |i: &mut usize| {
            if *i >= at {
                *i = i.saturating_add_signed(by);
            }
        };
        for slot in self.blocks.values_mut() {
            match slot {
                Block::Reply(i) | Block::Step(i, _) => moved(i),
            }
        }
        for (i, _) in self.unnamed.iter_mut().chain(self.calls.values_mut()) {
            moved(i);
        }
        if let Some(i) = self.compacting.as_mut() {
            moved(i);
        }
    }
}

/// 这几条里你说的话还留着几张图片（撤销藏起来的不算）。
pub(super) fn images_in(entries: &[Entry]) -> usize {
    entries
        .iter()
        .filter(|e| e.kind == super::Kind::User && !e.hidden)
        .flat_map(|e| &e.pasted)
        .filter(|c| c.image)
        .count()
}
