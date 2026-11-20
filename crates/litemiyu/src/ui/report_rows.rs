//! 正文里后台任务结束、别的会话空了那几行（Miyu 终端 `ui/job_rows.rs`，蓝图 `tui.md`「后台命令、子代理和侧边栏」第 5 条）：
//! 一行，记号有颜色、字暗，放不下截掉；有全文（子代理的报告、命令和它的输出、不允许的理由）的能点开，悬停加粗，点开整块铺
//! 底色，那一行连同全文，照时间线点开一步，再点收起。

use ratatui::style::{Modifier, Style};
use ratatui::text::Span;

use super::rows::{Ctx, Row, Target, clip};
use crate::input::pieces;
use crate::theme;
use crate::transcript::{Entry, MarkTone};

/// 排成的行。`i` 是它在正文里是第几条。
pub fn rows(i: usize, entry: &Entry, ctx: &Ctx) -> Vec<Row> {
    let target = Target::Entry(i);
    let can_open = !entry.detail.trim().is_empty();
    let hovered = can_open && ctx.hover == Some(target);
    let lit = |s: Style| {
        if hovered {
            s.add_modifier(Modifier::BOLD)
        } else {
            s
        }
    };
    let mark = entry.mark.as_deref().unwrap_or_default();
    let mark_style = match entry.mark_tone {
        MarkTone::Good => theme::good(),
        MarkTone::Bad => theme::error(),
        MarkTone::Quiet => theme::dim(),
        MarkTone::Accent => theme::accent(),
    };
    let width = ctx
        .width
        .saturating_sub(u16::try_from(mark.chars().count() + 1).unwrap_or(0));
    let mut head = ctx.row(
        ctx.blank_slot(),
        vec![
            Span::styled(mark.to_string(), mark_style),
            Span::styled(clip(&entry.text, width), lit(theme::dim())),
        ],
    );
    if !can_open {
        return vec![head];
    }
    head.target = Some(target);
    let mut out = vec![head];
    if entry.open {
        let width = ctx.width.saturating_sub(2).max(1);
        let blank = || ctx.row(ctx.blank_slot(), Vec::new());
        out.push(blank());
        for (piece, joined) in pieces(entry.detail.trim_matches('\n'), width) {
            let mut row = ctx.led_row(
                ctx.blank_slot(),
                vec![Span::raw("  ")],
                vec![Span::raw(piece)],
            );
            row.joined = joined;
            out.push(row);
        }
        out.push(blank());
        // 点开是整块换成铺底色的，连那一行一起（Miyu 2026-09-30 项目主人）。
        for row in &mut out {
            row.shade = true;
            row.target = Some(target);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::rows;
    use crate::theme;
    use crate::transcript::{MarkTone, Transcript};
    use crate::ui::rows::Target;
    use crate::ui::test_support::Fixture;

    #[test]
    fn a_finished_agent_opens_to_its_whole_report() {
        let f = Fixture::new();
        let mut t = Transcript::default();
        t.marked_note(
            "● ".into(),
            MarkTone::Good,
            "后台任务完成 · 查资料 · 40s".into(),
        );
        t.entries[0].detail = "第一段报告\n第二段报告".into();
        let shut = rows(0, &t.entries[0], &f.ctx());
        assert_eq!(shut.len(), 1, "收着只有一行");
        assert_eq!(
            shut[0].line.to_string().trim(),
            "● 后台任务完成 · 查资料 · 40s"
        );
        assert_eq!(shut[0].target, Some(Target::Entry(0)), "这一行能点");
        assert!(
            shut[0]
                .line
                .spans
                .iter()
                .any(|s| s.content == "● " && s.style == theme::good())
        );
        t.entries[0].open = true;
        let open = rows(0, &t.entries[0], &f.ctx());
        let text: Vec<String> = open.iter().map(|r| r.line.to_string()).collect();
        assert!(text.iter().any(|l| l.contains("第一段报告")), "{text:?}");
        assert!(text.iter().any(|l| l.contains("第二段报告")), "{text:?}");
        assert!(open.iter().all(|r| r.shade), "点开整块铺底色");
        assert!(open.iter().all(|r| r.target == Some(Target::Entry(0))));
    }

    /// Miyu `job_rows` 的 `the_mark_is_coloured_and_the_words_are_plain`、`a_stopped_note_has_a_dim_dot`：记号有颜色、字暗；
    /// 没有全文的不能点。
    #[test]
    fn the_mark_is_coloured_and_a_row_without_detail_cannot_open() {
        let f = Fixture::new();
        let mut t = Transcript::default();
        t.marked_note(
            "✗ ".into(),
            MarkTone::Bad,
            "后台命令失败 · make · 退出码 1".into(),
        );
        t.marked_note(
            "● ".into(),
            MarkTone::Quiet,
            "后台命令已停止 · sleep 300".into(),
        );
        let failed = rows(0, &t.entries[0], &f.ctx());
        let spans = &failed[0].line.spans;
        assert!(
            spans
                .iter()
                .any(|s| s.content == "✗ " && s.style == theme::error())
        );
        assert!(
            spans
                .iter()
                .any(|s| s.content.starts_with("后台命令失败") && s.style == theme::dim()),
            "字暗"
        );
        assert!(failed[0].target.is_none(), "没有全文的不能点");
        let stopped = rows(1, &t.entries[1], &f.ctx());
        assert!(
            stopped[0]
                .line
                .spans
                .iter()
                .any(|s| s.content == "● " && s.style == theme::dim()),
            "停了的圆点是暗的"
        );
    }

    /// Miyu `job_rows::rows`：放不下的不折行，截到「宽 − 记号字数 − 1」，末尾写 `…`。
    #[test]
    fn a_long_row_is_clipped_to_one_line() {
        let f = Fixture::new();
        let mut t = Transcript::default();
        let long = format!("后台命令完成 · {} · 2s", "x".repeat(400));
        t.marked_note("● ".into(), MarkTone::Good, long);
        let shown = rows(0, &t.entries[0], &f.ctx());
        assert_eq!(shown.len(), 1, "一行");
        assert!(shown[0].line.to_string().trim_end().ends_with('…'));
    }

    /// Miyu `job_rows` 的 `command_output_keeps_its_leading_spaces`：点开的命令输出第一行的缩进不被吃掉。
    #[test]
    fn command_output_keeps_its_leading_spaces() {
        let f = Fixture::new();
        let mut t = Transcript::default();
        let output = "   Compiling a\n   Compiling b".to_string();
        t.marked_with(
            "● ".into(),
            MarkTone::Good,
            "后台命令完成 · cargo · 9s".into(),
            output,
        );
        t.entries[0].open = true;
        let open = rows(0, &t.entries[0], &f.ctx());
        let text: Vec<String> = open
            .iter()
            .map(|r| r.line.to_string().trim_end().to_string())
            .collect();
        assert_eq!(text[2], text[3].replace('b', "a"), "第一行的缩进不被吃掉");
    }
}
