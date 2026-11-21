//! 画技能页、MCP 页（Miyu 终端 `ui/settings/mcp_list.rs`：一个服务一行，名字、状态）：名字照最长的对齐、开关、暗色的状态；
//! 选中的那一行左边竖条、整行铺底（同 Miyu 的列表行）。一个都没有的写一句「还没有」；读不成的接在后面，红色一行一句。

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use crate::settings::{Extra, Settings};
use crate::theme;
use crate::ui::rows::clip;

/// 那一页的几行；光标在第 `at` 行。另外交回光标那一行被截掉的状态（说明）的全文：列表下面写全（LM-38），放得下的是 `None`。
pub fn lines(
    settings: &Settings,
    extra: Extra,
    at: usize,
    width: u16,
) -> (Vec<Line<'static>>, Option<String>) {
    let texts = &settings.texts.extras;
    let listed = &settings.extra_list;
    let mut lines = Vec::new();
    let mut cut = None;
    if listed.entries.is_empty() {
        let empty = match extra {
            Extra::Skills => &texts.empty[0],
            // 预设页不走这里（`presets.rs`）。
            Extra::Mcp | Extra::Presets => &texts.empty[1],
        };
        lines.push(Line::styled(format!("  {empty}"), theme::dim()));
    }
    let longest = listed
        .entries
        .iter()
        .map(|entry| entry.shown.width())
        .max()
        .unwrap_or(0);
    for (i, entry) in listed.entries.iter().enumerate() {
        let pad = " ".repeat(longest.saturating_sub(entry.shown.width()) + 2);
        let switch = &settings.texts.toggle[usize::from(!entry.on)];
        let chosen = i == at;
        // 同 Miyu 的列表行：选中的左边强调色竖条、整行铺选中的底色；名字、开关（加粗，同一页里的值）、暗色的状态。
        let mut spans = vec![super::floating::lead(chosen, true)];
        let name = if entry.on { Style::new() } else { theme::dim() };
        spans.push(Span::styled(format!("{}{pad}", entry.shown), name));
        spans.push(Span::styled(
            format!("{switch}  "),
            Style::new().add_modifier(Modifier::BOLD),
        ));
        let used: usize = spans.iter().map(|span| span.content.width()).sum();
        let room = usize::from(width).saturating_sub(used);
        let state = clip(&entry.state, u16::try_from(room).unwrap_or(u16::MAX));
        if chosen && state != entry.state {
            cut = Some(entry.state.clone());
        }
        let rest = room.saturating_sub(state.width());
        spans.push(Span::styled(state, theme::dim()));
        spans.push(Span::raw(" ".repeat(rest)));
        let line = Line::from(spans);
        lines.push(if chosen {
            line.patch_style(theme::row_focus())
        } else {
            line
        });
    }
    for broken in &listed.broken {
        lines.push(Line::styled(
            format!("  {}", clip(broken, width.saturating_sub(2))),
            theme::error(),
        ));
    }
    (lines, cut)
}
