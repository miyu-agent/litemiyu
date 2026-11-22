//! 撤销说明那一行（Miyu 终端 `ui/undo_rows.rs`，蓝图 `tui.md`「正文」第 5 条）：收着一行 `↶ 已撤回 · /restore 恢复 · 那一句的
//! 预览`，撤掉的几轮里有压缩、清空的下面各一行；点开铺底色写你说的那句全文，最后一行停掉了几个任务。几个数照撤销的回应
//! （内核的 `Outcome::Reverted`，同 Miyu 回应的 `Report`）；补发来的没有回应，同 Miyu 补发的样子不写。
//!
//! 和 Miyu 不一样的：改过的文件内核不改回（Miyu 核心 U-1），Miyu 只在旧日志里才有的「已还原 N 个文件」、文件一个一行、差异不画。

use ratatui::text::Span;
use unicode_width::UnicodeWidthStr;

use super::rows::{Ctx, Row, Target, clip};
use crate::input::pieces;
use crate::theme;
use crate::transcript::{Entry, undo_counts};

/// 撤销说明那一条的几行。
pub(super) fn rows(i: usize, entry: &Entry, ctx: &Ctx) -> Vec<Row> {
    let target = Target::Entry(i);
    let style = if ctx.hover == Some(target) {
        theme::hover()
    } else {
        theme::dim()
    };
    let text = &ctx.config.text().undo;
    let mut head = format!("{}{}", ctx.config.layout.undo_icon, text.undone);
    head.push_str(&crate::sheet::sep());
    head.push_str(&text.restore_hint);
    let peek: String = entry.text.split_whitespace().collect::<Vec<_>>().join(" ");
    if !peek.is_empty() {
        head.push_str(&crate::sheet::sep());
        head.push_str(&peek);
    }
    let mut out = vec![ctx.row(
        ctx.blank_slot(),
        vec![Span::styled(clip(&head, ctx.width), style)],
    )];
    let report = entry.undo.as_deref();
    // 撤掉的几轮里有压缩、有清空：下面一行说一句，和「已撤回」对齐（Miyu 施工 6-9、6-8 补，照 `miyu undo`）。
    for (count, said) in [
        (report.map_or(0, |r| r.compactions), &text.undo_compactions),
        (report.map_or(0, |r| r.clears), &text.undo_clears),
    ] {
        if count == 0 {
            continue;
        }
        let indent = " ".repeat(ctx.config.layout.undo_icon.width());
        out.push(ctx.led_row(
            ctx.blank_slot(),
            vec![Span::raw(indent)],
            vec![Span::styled(said.clone(), style)],
        ));
    }
    if entry.open {
        let width = ctx.width.saturating_sub(2).max(1);
        let blank = || ctx.row(ctx.blank_slot(), Vec::new());
        out.push(blank());
        for (piece, joined) in pieces(entry.text.trim(), width) {
            let mut row = ctx.led_row(
                ctx.blank_slot(),
                vec![Span::raw("  ")],
                vec![Span::raw(piece)],
            );
            row.joined = joined;
            out.push(row);
        }
        if let Some(counts) = report.and_then(|r| undo_counts(r, text)) {
            out.push(blank());
            out.push(ctx.led_row(
                ctx.blank_slot(),
                vec![Span::raw("  ")],
                vec![Span::styled(counts, theme::dim())],
            ));
        }
        out.push(blank());
        for row in &mut out {
            row.shade = true;
        }
    }
    for row in &mut out {
        row.target = Some(target);
    }
    out
}
