//! 你说的话（蓝图 `tui.md`「正文」第 2 条）：行首竖线，上下各多一行只有竖线的空行。里面有粘贴块的，块照输入框里的样子写，
//! 整条能点：点开原地把每一块换成全文，再点换回块；悬停时块亮一档。

use ratatui::style::Style;
use ratatui::text::Span;
use unicode_width::UnicodeWidthStr;

use super::rows::{CopyBlock, Ctx, Row, Target};
use crate::input::wrap_words;
use crate::theme;
use crate::transcript::{Chip, Entry};
use std::rc::Rc;

/// 排成的行。`i` 是它在正文里是第几条。
pub fn rows(i: usize, entry: &Entry, ctx: &Ctx) -> Vec<Row> {
    // 你用 `!` 打的命令（「人自己跑命令 `!`」第 4 条）。
    if let Some(shell) = &entry.shell {
        return super::shell_rows::rows(entry, shell, ctx);
    }
    // 别处来的话（别的会话发来的）。
    if let Some(from) = &entry.from {
        return super::foreign_rows::rows(i, entry, from, ctx);
    }
    let bar_style = theme::user_bar();
    let bar = Span::styled(ctx.config.layout.user_bar.clone(), bar_style);
    let said = entry.text.trim_matches('\n');
    // 有粘贴块的才能点开。
    let clickable = !entry.pasted.is_empty();
    let hovered = clickable && ctx.hover == Some(Target::Entry(i));
    let (text, pieces) = shaped(said, &entry.pasted, entry.open, hovered);
    let mut out = vec![ctx.row(bar.clone(), Vec::new())];
    let mut prev_end = None;
    for line in wrap_words(&text, ctx.width.max(1)) {
        let styled: Vec<(usize, usize, Style)> =
            pieces.iter().map(|p| (p.from, p.to, p.style)).collect();
        let mut row = ctx.row(bar.clone(), spans(&text, line.start, line.end, &styled));
        row.copy_blocks = copy_blocks(i, &text, line.start, line.end, &pieces);
        row.joined = prev_end == Some(line.start);
        prev_end = Some(line.end);
        out.push(row);
    }
    out.push(ctx.row(bar, Vec::new()));
    if clickable {
        for row in &mut out {
            row.target = Some(Target::Entry(i));
        }
    }
    out
}

/// 排好的字里的一块：字节范围、样子；收着的另带它的原文（复制时换回原文）。
pub(super) struct Piece {
    pub(super) from: usize,
    pub(super) to: usize,
    pub(super) style: Style,
    replacement: Option<(usize, Rc<str>)>,
}

/// 排成的字，和每一块。收着的块写块上的字，悬停时亮一档；点开了粘贴块原地换成全文，悬停时粘的那几段铺上块的底色。
pub(super) fn shaped(
    text: &str,
    chips: &[Chip],
    open: bool,
    hovered: bool,
) -> (String, Vec<Piece>) {
    let chip = if hovered {
        theme::chip_hover()
    } else {
        theme::chip()
    };
    let mut out = String::with_capacity(text.len());
    let mut pieces = Vec::new();
    let mut at = 0;
    for (index, ((start, end), c)) in block_ranges(text, chips).into_iter().zip(chips).enumerate() {
        out.push_str(&text[at..start]);
        let from = out.len();
        if open {
            out.push_str(c.full.trim_matches('\n'));
            if hovered {
                let style = theme::chip_ground();
                pieces.push(Piece {
                    from,
                    to: out.len(),
                    style,
                    replacement: None,
                });
            }
        } else {
            out.push_str(&text[start..end]);
            let style = chip;
            pieces.push(Piece {
                from,
                to: out.len(),
                style,
                replacement: Some((index, Rc::from(c.full.as_str()))),
            });
        }
        at = end;
    }
    out.push_str(&text[at..]);
    (out, pieces)
}

/// 把收起的块投影到折行的显示列，复制不依赖标签的文字；同一块跨行共用原文和编号。
fn copy_blocks(
    entry: usize,
    text: &str,
    start: usize,
    end: usize,
    pieces: &[Piece],
) -> Vec<CopyBlock> {
    let cols = |a: usize, b: usize| u16::try_from(text[a..b].width()).unwrap_or(u16::MAX);
    pieces
        .iter()
        .filter_map(|p| {
            let (index, full) = p.replacement.as_ref()?;
            let (s, e) = (p.from.max(start), p.to.min(end));
            if s >= e {
                return None;
            }
            let from = cols(start, s);
            Some(CopyBlock {
                id: (entry, *index),
                cols: (from, from + cols(s, e)),
                text: full.clone(),
            })
        })
        .collect()
}

/// 字里每一块占的字节范围：照先后一块一块往后找它的样子（两块写出来一样也各对各）。
fn block_ranges(text: &str, chips: &[Chip]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut from = 0;
    for c in chips {
        let Some(at) = text[from..].find(c.label.as_str()) else {
            break;
        };
        out.push((from + at, from + at + c.label.len()));
        from += at + c.label.len();
    }
    out
}

/// `[start, end)` 这一截排成几段：落在块里的（折行折到一半的也算）用块的样子，别的原色。
pub(super) fn spans(
    text: &str,
    start: usize,
    end: usize,
    pieces: &[(usize, usize, Style)],
) -> Vec<Span<'static>> {
    let mut out = Vec::new();
    let mut at = start;
    for &(s, e, style) in pieces {
        let (s, e) = (s.max(start), e.min(end));
        if s >= e {
            continue;
        }
        if at < s {
            out.push(Span::styled(text[at..s].to_string(), Style::new()));
        }
        out.push(Span::styled(text[s..e].to_string(), style));
        at = e;
    }
    if at < end {
        out.push(Span::styled(text[at..end].to_string(), Style::new()));
    }
    out
}
