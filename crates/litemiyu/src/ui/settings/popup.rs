//! 画选择窗、编辑窗、勾选窗（Miyu 终端 `ui/floating.rs` 的 `choose`、`ui/settings/more_popups.rs` 的 `line`、
//! `dialog_popups.rs` 的 `ticks`，蓝图「配置页」第 33、34、47a 条）：盖在整屏正中，背后调暗；浅一档的底、不画边框，标题
//! 强调色加粗，一个选项一行（选中的那一行铺底、左边竖条，现在的值不另加记号），编辑窗一行字（遮住的写成点），填错的红字写在
//! 窗里；问一句的窗（恢复默认）一排按钮；按键提示写在窗里最下面。

use ratatui::layout::{Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use super::floating::{self, Lines, fit};
use crate::settings::texts::Texts;
use crate::settings::{Hits, Popup, Window};
use crate::theme;
use crate::ui::frame::Frame;

/// 窗多宽、最高占几成（Miyu `layout.json` 的 `popup_wide`、`popup_width`、`popup_height_percent`）：问一句的窗 66，选择窗、
/// 编辑窗、勾选窗 78（Miyu `ui/settings/popup.rs` 的 `wide`）。
const WIDTH: u16 = 66;
const WIDE: u16 = 78;
const HEIGHT_PERCENT: u16 = 86;

/// 整屏调暗，在正中画 `popup`；编辑窗交回光标的位置。`_area` 是配置界面那一块（窗照整屏摆，同 Miyu）。`vim`：按键模式是
/// Vim（[`lines`]）。
pub fn draw(
    frame: &mut Frame,
    _area: Rect,
    (popup, hits): (&Popup, &Hits),
    (texts, vim): (&Texts, bool),
) -> Option<Position> {
    let area = frame.area();
    frame
        .buffer_mut()
        .set_style(area, Style::new().add_modifier(Modifier::DIM));
    let wide = !matches!(popup.window, Window::Confirm(_));
    let width = if wide { WIDE } else { WIDTH }.min(area.width.saturating_sub(2));
    let max_height = (area.height * HEIGHT_PERCENT / 100).max(6).min(area.height);
    let lines = lines(popup, texts, width, vim);
    floating::centered(frame, area, (width, max_height), &lines, hits)
}

/// 窗里的一行行（`width` 是窗宽）：开头配色方案那一步不压暗、照它摆在格子下面（`opening.rs` 的 `picker`）。按键模式是 Vim
/// （`vim`）时按键提示里的「↑/↓」写成「j/k」（`hint_keys.rs`，同开头最下面那一行、对话里各框的下边框；几种窗都认 `j` `k`，
/// `settings/popup.rs`）：第二十六趟验收时报的，开头「明暗」的窗里写「↑/↓ 选择」、屏幕底下写「j/k 选择」。Miyu 的窗
/// 里也照写「↑/↓」，没换（`ui/oobe/theme_step.rs` 的 `picker`），这一处照 Miyu 别处的规矩统一。
pub fn lines(popup: &Popup, texts: &Texts, width: u16, vim: bool) -> Lines {
    let room = usize::from(width.saturating_sub(6));
    let mut lines = Lines::default();
    match &popup.window {
        Window::Choose(choose) => {
            lines.title(choose.title.clone(), String::new());
            for (i, name) in choose.names.iter().enumerate() {
                let name = fit(name, room);
                lines.row(Line::from(Span::raw(format!("  {name}"))), i == choose.at);
            }
            lines.hints.clone_from(&texts.key_hints.choose);
        }
        Window::Ticks(ticks) => {
            lines.title(ticks.title.clone(), String::new());
            for (i, (name, on)) in ticks.names.iter().zip(&ticks.checked).enumerate() {
                let mark = if *on { "[x] " } else { "[ ] " };
                lines.row(
                    Line::from(vec![
                        Span::raw("  "),
                        Span::styled(mark, theme::accent()),
                        Span::raw(fit(name, room.saturating_sub(4))),
                    ]),
                    i == ticks.at,
                );
            }
            lines.hints.clone_from(&texts.pools.ticks_hints);
        }
        Window::Confirm(confirm) => {
            lines.title(confirm.title.clone(), String::new());
            for line in &confirm.lines {
                lines.plain(Line::styled(line.clone(), theme::dim()));
            }
            lines.buttons.clone_from(&confirm.buttons);
            lines.button = Some(confirm.at);
            lines.hints.clone_from(&texts.models.popup_hints[3]);
        }
        Window::Edit(edit) => {
            lines.title(edit.title.clone(), String::new());
            let shown = edit.shown();
            let before: String = shown.chars().take(edit.cursor).collect();
            lines.caret = Some((0, 2 + u16::try_from(before.width()).unwrap_or(u16::MAX)));
            lines.row(Line::raw(format!("  {shown}")), true);
            lines.error.clone_from(&edit.error);
            lines.hints.clone_from(&texts.key_hints.edit);
        }
    }
    for hint in &mut lines.hints {
        for part in hint.iter_mut() {
            *part = crate::ui::hint_keys::vim(part, vim);
        }
    }
    lines
}
