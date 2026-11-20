//! 正文按条缓存排好的行（蓝图 `tui.md`「正文」第 8 条）：每一条排好的行按编号记着，这一条自己和排版的条件都没变
//! 就直接拿记着的用，只重排变了的、在进行的那几条。排好的行拼成一份 [`Rows`]，视口、鼠标、复制直接引用，
//! 不再每帧复制。

use std::cell::RefCell;
use std::collections::hash_map::DefaultHasher;
use std::collections::{HashMap, HashSet};
use std::hash::{Hash, Hasher};
use std::mem::discriminant;
use std::rc::Rc;
use std::time::{Duration, Instant};

use super::rows::{self, Ctx, Row, Target};
use crate::transcript::{Entry, Step, StepKind, ToolState, Transcript};

/// 这一帧的全部行：一条一块，共享记着的那一份；放掉的段（红线 5，`transcript/pieces.rs`）并成一块空档，只记几行，不造行。
#[derive(Debug, Clone, Default)]
pub struct Rows {
    chunks: Vec<Chunk>,
    /// 每一块结束在第几行（不含）。
    ends: Vec<usize>,
    /// 每一块是谁的。
    owners: Vec<Owner>,
}

/// 一块：排好的行；空档是同一行重复 `repeat` 次。
#[derive(Debug, Clone)]
struct Chunk {
    rows: Rc<[Row]>,
    repeat: usize,
}

impl Chunk {
    fn len(&self) -> usize {
        match self.repeat {
            0 => self.rows.len(),
            n => n,
        }
    }

    fn get(&self, k: usize) -> Option<&Row> {
        match self.repeat {
            0 => self.rows.get(k),
            n => self.rows.first().filter(|_| k < n),
        }
    }
}

/// 一块是谁的。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Owner {
    /// 条目间的空行、等她第一个字的那几行。
    Nobody,
    /// 编号是它的那一条（`Entry::id`）。
    Entry(u64),
    /// 从第几段起的一串放掉的段。
    Gap(u32),
}

impl Rows {
    /// 一共几行。
    pub fn len(&self) -> usize {
        self.ends.last().copied().unwrap_or(0)
    }

    /// 一行都没有。
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// 第 `i` 行落在第几块。
    fn chunk_of(&self, i: usize) -> usize {
        self.ends.partition_point(|&end| end <= i)
    }

    /// 第 `k` 块从第几行起。
    fn start(&self, k: usize) -> usize {
        if k == 0 { 0 } else { self.ends[k - 1] }
    }

    /// 第 `i` 行。
    pub fn get(&self, i: usize) -> Option<&Row> {
        let k = self.chunk_of(i);
        self.chunks.get(k)?.get(i - self.start(k))
    }

    /// 从头到尾每一行。
    pub fn iter(&self) -> impl Iterator<Item = &Row> {
        self.chunks
            .iter()
            .flat_map(|chunk| (0..chunk.len()).filter_map(move |k| chunk.get(k)))
    }

    /// 从第 `first` 行起的 `n` 行，带着行号：只画视口里的，不从头数过去。
    pub fn window(&self, first: usize, n: usize) -> impl Iterator<Item = (usize, &Row)> {
        (first..first.saturating_add(n).min(self.len())).filter_map(|i| Some((i, self.get(i)?)))
    }

    pub(super) fn push(&mut self, chunk: Rc<[Row]>) {
        self.push_owned(
            Chunk {
                rows: chunk,
                repeat: 0,
            },
            Owner::Nobody,
        );
    }

    fn push_owned(&mut self, chunk: Chunk, owner: Owner) {
        if chunk.len() == 0 {
            return;
        }
        let end = self.len() + chunk.len();
        self.chunks.push(chunk);
        self.ends.push(end);
        self.owners.push(owner);
    }

    /// 属于条目（连同放掉的段）的有几行：末尾不属于哪一条的（等第一个字时转着的那几行）不算；一行都不属于的照全部算。
    pub fn content_len(&self) -> usize {
        self.owners
            .iter()
            .rposition(|o| *o != Owner::Nobody)
            .map_or_else(|| self.len(), |k| self.ends[k])
    }

    /// 第 `row` 行是哪一条的（编号）；落在条目间空行、放掉的段上的，照它下面那一条。
    pub fn entry_at(&self, row: usize) -> Option<u64> {
        let k = self.chunk_of(row);
        self.owners.get(k..)?.iter().find_map(|o| match o {
            Owner::Entry(id) => Some(*id),
            _ => None,
        })
    }

    /// 第 `row` 行落在放掉的段上：从第几段起的那一串、它在这一串里是第几行。
    pub fn gap_at(&self, row: usize) -> Option<(u32, usize)> {
        let k = self.chunk_of(row);
        match self.owners.get(k)? {
            Owner::Gap(first) => Some((*first, row - self.start(k))),
            _ => None,
        }
    }

    /// 第 `row` 行落在放掉的段上：那一块空档从第几行起、到第几行（不含）。
    pub fn gap_span(&self, row: usize) -> Option<(usize, usize)> {
        let k = self.chunk_of(row);
        matches!(self.owners.get(k)?, Owner::Gap(_)).then(|| (self.start(k), self.ends[k]))
    }

    /// 第 `row` 行所在那一块以前最后一条（编号）；上面没有条目的是 `None`。
    pub fn entry_before(&self, row: usize) -> Option<u64> {
        let k = self.chunk_of(row);
        self.owners.get(..k)?.iter().rev().find_map(|o| match o {
            Owner::Entry(id) => Some(*id),
            _ => None,
        })
    }

    /// 从第 `first` 段起的那一串放掉的段从第几行起；不是一串的开头（读回了、并进了前面的）是 `None`。
    pub fn gap_start(&self, first: u32) -> Option<usize> {
        let k = self.owners.iter().position(|o| *o == Owner::Gap(first))?;
        Some(self.start(k))
    }

    /// 编号 `id` 的那一条最后一行后面是第几行（不含）；没有这一条的是 `None`。
    pub fn end_of(&self, id: u64) -> Option<usize> {
        let k = self.owners.iter().rposition(|o| *o == Owner::Entry(id))?;
        Some(self.ends[k])
    }

    /// 编号 `id` 的那一条从第几行起。
    pub fn start_of(&self, id: u64) -> Option<usize> {
        let k = self.owners.iter().position(|o| *o == Owner::Entry(id))?;
        Some(self.start(k))
    }
}

/// 这一帧怎么排（蓝图「正文」第 8 条）：要整份重排时一帧最多花多久，先排哪一条附近的。
#[derive(Debug, Clone, Copy, Default)]
pub struct Plan {
    /// 这一帧重排最多花多久；`None` 是不限（一口气排完）。每帧至少排一条。
    pub budget: Option<Duration>,
    /// 视口附近是第几条：从它往两边排；`None` 是跟着最新，从最底下往上排。
    pub anchor: Option<usize>,
}

impl Plan {
    /// 不限时，一口气排完。
    #[cfg(test)]
    pub fn all() -> Self {
        Self::default()
    }
}

impl From<Vec<Row>> for Rows {
    fn from(rows: Vec<Row>) -> Self {
        let mut out = Self::default();
        out.push(rows.into());
        out
    }
}

/// 记着的行：条目的编号到（指纹、排好的行、排出来带不带图）。
#[derive(Debug, Default)]
pub struct RowCache {
    entries: HashMap<u64, (u64, Rc<[Row]>)>,
    /// 上一帧重排了几条（测试、量尺看）。
    pub rebuilt: usize,
    /// 上一帧没轮到、先用旧行的有几条：还有的话下一帧接着排（`App::deadline`）。
    pub stale: usize,
    /// 上一帧重排的是哪几条（编号）。
    fresh: HashSet<u64>,
}

impl RowCache {
    /// 编号 `id` 的那一条记着的行有几行；没排过的是 `None`。
    pub fn rows_of(&self, id: u64) -> Option<usize> {
        self.entries.get(&id).map(|(_, rows)| rows.len())
    }

    /// 编号 `id` 的那一条上一帧重排过。
    #[cfg(test)]
    pub fn fresh(&self, id: u64) -> bool {
        self.fresh.contains(&id)
    }
}

/// 把看得见的正文排成行，一条之间空一行；没变的条目用记着的。和整份重排（测试里的 `fresh_rows`）排出来的一模一样。
/// 要重排的多、`plan` 给了预算的，照预算排视口附近的几条，没轮到的先用旧行，记进 `stale`，下一帧接着排。
/// 放掉的段（红线 5）连着的并成一块空档，行数照槽里记的（换了宽照字数重估），只走常驻的条目。
pub fn build(transcript: &Transcript, ctx: &Ctx, cache: &RefCell<RowCache>, plan: Plan) -> Rows {
    let entries = &transcript.entries;
    let mut cache = cache.borrow_mut();
    cache.rebuilt = 0;
    cache.stale = 0;
    cache.fresh.clear();
    let blank: Rc<[Row]> = Rc::from(vec![ctx.row(ctx.blank_slot(), Vec::new())]);
    let shown: Vec<usize> = (0..entries.len())
        .filter(|&i| rows::shown(&entries[i]))
        .collect();
    // 先看哪几条能直接用记着的，别的记下来要排。
    let mut chunks: Vec<Option<Rc<[Row]>>> = vec![None; shown.len()];
    let mut todo = Vec::new();
    for (k, &i) in shown.iter().enumerate() {
        let entry = &entries[i];
        let key = fingerprint(i, entry, ctx);
        match (key, cache.entries.get(&entry.id)) {
            (Some(key), Some((old, rows))) if *old == key => chunks[k] = Some(rows.clone()),
            _ => todo.push(k),
        }
    }
    // 从视口附近往两边排：跟着最新的从最底下往上。
    let near = plan
        .anchor
        .and_then(|a| shown.iter().position(|&i| i >= a))
        .unwrap_or(shown.len().saturating_sub(1));
    // 离得近的在前、一样近的照原来的先后（共用一份排法，`fuzzy::best_first`：离得越远分越低）。
    let mut todo: Vec<(i64, usize)> = todo
        .into_iter()
        .map(|k| (-i64::try_from(k.abs_diff(near)).unwrap_or(i64::MAX), k))
        .collect();
    crate::fuzzy::best_first(&mut todo);
    let start = Instant::now();
    for (_, k) in todo {
        let i = shown[k];
        let entry = &entries[i];
        let over = cache.rebuilt > 0 && plan.budget.is_some_and(|b| start.elapsed() >= b);
        // 预算用完了：记着旧行、不是在进行的，这一帧先用旧行。
        let stale = over
            .then(|| cache.entries.get(&entry.id).map(|kept| kept.1.clone()))
            .flatten()
            .filter(|_| fingerprint(i, entry, ctx).is_some());
        if let Some(rows) = stale {
            cache.stale += 1;
            chunks[k] = Some(rows);
            continue;
        }
        cache.rebuilt += 1;
        cache.fresh.insert(entry.id);
        let rows: Rc<[Row]> = rows::entry_rows(i, entry, ctx).into();
        if let Some(key) = fingerprint(i, entry, ctx) {
            cache.entries.insert(entry.id, (key, rows.clone()));
        }
        chunks[k] = Some(rows);
    }
    let mut out = Rows::default();
    let mut gaps = Gaps::new(transcript, ctx.width);
    for (chunk, &i) in chunks.into_iter().zip(&shown) {
        gaps.upto(entries[i].piece, &blank, &mut out);
        if !out.is_empty() {
            out.push(blank.clone());
        }
        if let Some(chunk) = chunk {
            out.push_owned(
                Chunk {
                    rows: chunk,
                    repeat: 0,
                },
                Owner::Entry(entries[i].id),
            );
        }
    }
    gaps.upto(u32::MAX, &blank, &mut out);
    // 不在了的（撤销后又被删掉、排队退回的、放掉了的段）不留着。
    let seen: HashSet<u64> = shown.iter().map(|&i| entries[i].id).collect();
    cache.entries.retain(|id, _| seen.contains(id));
    out
}

/// 拼行时还没放进去的放掉的段：照段的先后，到了常驻的段前面把连着的几段并成一块空档放进去。
struct Gaps<'a> {
    transcript: &'a Transcript,
    width: u16,
    /// 下一段从第几段看起。
    next: usize,
}

impl<'a> Gaps<'a> {
    fn new(transcript: &'a Transcript, width: u16) -> Self {
        Self {
            transcript,
            width,
            next: 0,
        }
    }

    /// 第 `piece` 段以前（不含）还没放进去的放掉的段：连着的并成一块。
    fn upto(&mut self, piece: u32, blank: &Rc<[Row]>, out: &mut Rows) {
        let pieces = self.transcript.pieces();
        let end = usize::try_from(piece)
            .unwrap_or(usize::MAX)
            .min(pieces.len());
        while self.next < end {
            if pieces.resident(self.next) {
                self.next += 1;
                continue;
            }
            let first = self.next;
            let mut rows = 0usize;
            while self.next < end && !pieces.resident(self.next) {
                if let Some(slot) = pieces.get(self.next) {
                    rows += super::estimate::rewrap(slot.rows, slot.chars, slot.width, self.width)
                        as usize;
                }
                self.next += 1;
            }
            // 段的行数里每一条都带着前面的空行；最前面那一条没有。
            if out.is_empty() {
                rows = rows.saturating_sub(1);
            }
            let first = u32::try_from(first).unwrap_or(u32::MAX);
            out.push_owned(
                Chunk {
                    rows: blank.clone(),
                    repeat: rows,
                },
                Owner::Gap(first),
            );
        }
        self.next = self.next.max(end);
    }
}

/// 这一条和排版条件的指纹：变了就重排。在进行的那一段（还有步骤在转圈、在走表）、正在压缩的那一行（行首在转圈）
/// 是 `None`，每帧重排。
fn fingerprint(i: usize, entry: &Entry, ctx: &Ctx) -> Option<u64> {
    if let Some(segment) = &entry.segment
        && (!segment.finished || segment.steps.iter().any(Step::busy))
    {
        return None;
    }
    if entry.progress.is_some() {
        return None;
    }
    // 跑着的命令末尾在转圈（「人自己跑命令 `!`」第 4 条）：每帧重排。
    if entry.shell.as_ref().is_some_and(|s| s.ran.is_none()) && !entry.queued {
        return None;
    }
    let mut h = DefaultHasher::new();
    // 排版的条件：行里带着条目的位置（点中的东西），图标烤在行里，颜色照换代数（终端回了颜色，照新颜色重排）。
    (
        i,
        crate::theme::generation(),
        ctx.width,
        ctx.indent.len(),
        &ctx.config.icons.name,
        // 时间线的几个开关（配置页能改，「时间线」第 18 条）。
        ctx.config.timeline.shape(),
        // 语言连同是不是自动：自动的中文和手动的中文代码一样，收起那一行却一个英文一个中文。
        (ctx.config.language.code(), ctx.config.auto),
    )
        .hash(&mut h);
    ctx.hover.filter(|t| owner(*t) == i).hash(&mut h);
    // 这一条自己：字只看长短和末尾（流式的字只往后接），别的看状态。
    discriminant(&entry.kind).hash(&mut h);
    let text = entry.text.as_bytes();
    (text.len(), &text[text.len().saturating_sub(64)..]).hash(&mut h);
    (entry.hidden, entry.queued, entry.open).hash(&mut h);
    entry.shell.hash(&mut h);
    entry.from.hash(&mut h);
    (entry.pasted.len(), &entry.details).hash(&mut h);
    // 后台命令结束那一行：输出读回来了补进点开看的全文（Miyu 哈希 `job.detail.len()`），有了全文才能点。
    (entry.report, entry.detail.len()).hash(&mut h);
    entry.undo.hash(&mut h);
    if let Some(segment) = &entry.segment {
        (segment.finished, segment.open, segment.steps.len()).hash(&mut h);
        for step in &segment.steps {
            (step.took, step.open).hash(&mut h);
            match &step.kind {
                StepKind::Thought { text } => text.len().hash(&mut h),
                StepKind::Tool {
                    name,
                    args,
                    state,
                    output,
                    said,
                    ..
                } => {
                    (name, args.len(), output.len(), said.is_some()).hash(&mut h);
                    discriminant(state).hash(&mut h);
                    if let ToolState::Done(status) = state {
                        discriminant(status).hash(&mut h);
                    }
                }
                // 回报那一步（Miyu `b91ede91`）：字、全文（输出读回来补进去）、记号。
                StepKind::Report { note, text, .. } => {
                    (text.len(), note.detail.len(), &note.mark).hash(&mut h);
                    discriminant(&note.tone).hash(&mut h);
                }
            }
        }
    }
    Some(h.finish())
}

/// 点中的东西属于第几条。
fn owner(target: Target) -> usize {
    match target {
        Target::Segment(i) | Target::Step(i, _) | Target::Entry(i) | Target::Details(i, _) => i,
    }
}

#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;
