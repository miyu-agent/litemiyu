//! 别处来的话（Miyu 终端 `ui/foreign_rows.rs`，蓝图 `tui.md`「别处来的话」）：和你说的话一样左边一根竖线、上下各一行空竖线，
//! 竖线和字都暗；收着是一行 `从会话 B 收到消息 · 预览`（换行换成空格，放不下截掉），点一下这一行换成来处、空一行、全文，
//! 再点收起。
//!
//! 和 Miyu 不一样的：全文里不认链接、不画附件块（LiteMiyu 你说的话也不认，别的会话发来的只有字）。

use ratatui::style::Style;
use ratatui::text::Span;

use super::rows::{Ctx, Row, Target, clip};
use crate::input::wrap_words;
use crate::theme;
use crate::transcript::Entry;

/// 排成的行。`i` 是它在正文里是第几条，`from` 是来处那一句。
pub fn rows(i: usize, entry: &Entry, from: &str, ctx: &Ctx) -> Vec<Row> {
    let target = Target::Entry(i);
    let style = if ctx.hover == Some(target) {
        theme::hover()
    } else {
        theme::dim()
    };
    let bar = Span::styled(ctx.config.layout.user_bar.clone(), theme::dim());
    let said = entry.text.trim_matches('\n');
    let mut out = vec![ctx.row(bar.clone(), Vec::new())];
    if entry.open {
        let mut head = ctx.row(bar.clone(), vec![Span::styled(from.to_string(), style)]);
        head.copy = false;
        out.push(head);
        // 来处和全文之间空一行（Miyu 2026-09-30 项目主人）。
        out.push(ctx.row(bar.clone(), Vec::new()));
        let mut prev_end = None;
        let (text, pieces) = super::user_rows::shaped(said, &entry.pasted, false, false);
        let styled: Vec<_> = pieces.iter().map(|p| (p.from, p.to, p.style)).collect();
        for line in wrap_words(&text, ctx.width.max(1)) {
            let end = line.start + text[line.start..line.end].trim_end_matches('\n').len();
            let spans = super::user_rows::spans(&text, line.start, end, &styled)
                .into_iter()
                .map(|s| {
                    if s.style == Style::new() {
                        s.style(style)
                    } else {
                        s
                    }
                })
                .collect();
            let mut row = ctx.row(bar.clone(), spans);
            row.joined = prev_end == Some(line.start);
            prev_end = Some(line.end);
            out.push(row);
        }
    } else {
        let flat = said.split_whitespace().collect::<Vec<_>>().join(" ");
        let words = &ctx.config.text().jobs;
        let line = words
            .received
            .replace("{from}", from)
            .replace("{text}", &flat);
        let mut row = ctx.row(
            bar.clone(),
            vec![Span::styled(clip(&line, ctx.width), style)],
        );
        row.copy = false;
        out.push(row);
    }
    out.push(ctx.row(bar, Vec::new()));
    for row in &mut out {
        row.target = Some(target);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::rows;
    use crate::theme;
    use crate::transcript::Transcript;
    use crate::ui::rows::Row;
    use crate::ui::test_support::Fixture;

    fn text(rows: &[Row]) -> Vec<String> {
        rows.iter()
            .map(|r| r.line.to_string().trim_end().to_string())
            .collect()
    }

    #[test]
    fn a_message_from_elsewhere_is_one_dim_line_that_opens_in_place() {
        // Miyu 2026-09-30 项目主人：别处发来的画成一个样子，左边竖线，字都暗，`从 xx 收到消息 · 预览`，点开替换成全文。
        let f = Fixture::new();
        let mut t = Transcript::default();
        t.foreign(
            3,
            "从会话 B 收到消息".into(),
            "我是 B：\n只回复「收到 B」".into(),
        );
        let entry = &t.entries[0];
        let closed = rows(0, entry, "从会话 B 收到消息", &f.ctx());
        let bar = f.config.layout.user_bar.trim_end();
        assert_eq!(
            text(&closed),
            [
                bar.to_string(),
                format!("{bar} 从会话 B 收到消息 · 我是 B： 只回复「收到 B」"),
                bar.to_string()
            ]
        );
        let line = &closed[1].line.spans;
        assert!(
            line.iter().any(
                |s| s.content.contains("从会话 B 收到消息 · 我是 B") && s.style == theme::dim()
            ),
            "字暗：{line:?}"
        );
        assert!(closed.iter().all(|r| r.target.is_some()), "整条能点");
        let mut open = entry.clone();
        open.open = true;
        let opened = text(&rows(0, &open, "从会话 B 收到消息", &f.ctx()));
        assert_eq!(
            opened,
            [
                bar.to_string(),
                format!("{bar} 从会话 B 收到消息"),
                bar.to_string(),
                format!("{bar} 我是 B："),
                format!("{bar} 只回复「收到 B」"),
                bar.to_string()
            ]
        );
    }

    /// LM-41（2026-11-22 项目主人）：循环到点交进来的那一句画成一行暗色的旁白，带着循环这次说的原话；AI 自己定的
    /// 到点继续写「定时触发」。和别处来的话不同，撤回那一轮时它跟着藏起来。
    #[test]
    fn a_loop_wakeup_is_one_dim_line_with_its_own_words_and_undo_takes_it() {
        use crate::core::{Push, Update};
        let f = Fixture::new();
        let mut t = Transcript::default();
        let texts = f.config.text();
        for (seq, looping) in [(3, true), (9, false)] {
            let woke = Push::Woke {
                seq,
                text: "看一眼时间".into(),
                looping,
            };
            t.update(Update::Push(woke), texts);
            t.update(Update::Push(Push::TurnStarted(seq + 1, Some(seq))), texts);
        }
        let bar = f.config.layout.user_bar.trim_end();
        let drawn = |i: usize| text(&crate::ui::user_rows::rows(i, &t.entries[i], &f.ctx()));
        let (first, second) = (0, t.entries.len() - 1);
        assert_eq!(drawn(first)[1], format!("{bar} 循环 · 看一眼时间"));
        assert_eq!(drawn(second)[1], format!("{bar} 定时触发 · 看一眼时间"));
        let rows = crate::ui::user_rows::rows(first, &t.entries[first], &f.ctx());
        assert!(
            rows[1].line.spans.iter().any(|s| s.style == theme::dim()),
            "字暗：{:?}",
            rows[1].line.spans
        );
        let reverted = Push::Reverted {
            turns: vec![10],
            seq: 20,
            cause: None,
        };
        t.update(Update::Push(reverted), texts);
        assert!(t.entries[second].hidden, "撤回带走到点交进来的那一句");
        assert!(!t.entries[first].hidden);
    }
}
