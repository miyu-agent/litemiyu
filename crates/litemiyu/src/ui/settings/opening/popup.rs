//! 浮在一步上面的窗（照 Miyu 终端 `ui/oobe/popup.rs` 搬，开头里只有「更多供应商…」用）：后面调暗，盖一块浅一档的底，最上面
//! 一行标题（强调色加粗），中间照给的一块画（比窗高的照光标滚），最下面一行暗色按键提示。不画边框（同配置页的悬浮窗）。

use ratatui::layout::{Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;

use super::content::Content;
use crate::theme;
use crate::ui::frame::Frame;

/// 占窗口的几成、最宽几列（Miyu `oobe.json` 的 `popup`：`width` 0.8、`height` 0.75、`max_width` 110）。
const WIDTH: f64 = 0.8;
const HEIGHT: f64 = 0.75;
const MAX_WIDTH: u16 = 110;

/// 窗在哪：照占窗口的几成、最宽几列，居中（Miyu `rect`）。
pub fn rect(area: Rect) -> Rect {
    // 几成都小于 1：乘出来在 0 到窗口宽高之间，放得进 u16。
    let part = |whole: u16, share: f64| (f64::from(whole) * share).round() as u16;
    let width = part(area.width, WIDTH)
        .min(MAX_WIDTH)
        .min(area.width.saturating_sub(2));
    let height = part(area.height, HEIGHT).min(area.height.saturating_sub(2));
    Rect::new(
        area.x + area.width.saturating_sub(width) / 2,
        area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    )
}

/// 中间写字的那一块：左右各空两列，标题、按键提示各占一行，和中间各隔一行（Miyu `body`）。
pub fn body(rect: Rect) -> Rect {
    Rect::new(
        rect.x + 2,
        rect.y + 3,
        rect.width.saturating_sub(4),
        rect.height.saturating_sub(6),
    )
}

/// 画一个窗：交回输入光标落在哪（有的话）（Miyu `draw`）。
pub fn draw(
    frame: &mut Frame,
    rect: Rect,
    (title, content, hint): (&str, &Content, &str),
) -> Option<Position> {
    let area = frame.area();
    let buf = frame.buffer_mut();
    buf.set_style(area, Style::new().add_modifier(Modifier::DIM));
    // 先把格子整个清掉（后面调暗的修饰不留到窗里），再铺底色。
    theme::float(buf, rect);
    let inner = body(rect);
    buf.set_line(
        inner.x,
        rect.y + 1,
        &Line::styled(
            title.to_string(),
            theme::accent().add_modifier(Modifier::BOLD),
        ),
        inner.width,
    );
    buf.set_line(
        inner.x,
        rect.bottom().saturating_sub(2),
        &Line::styled(hint.to_string(), theme::faint()),
        inner.width,
    );
    let scroll = place(frame, inner, content);
    let (row, col) = content.caret?;
    let at = Position::new(inner.x + col, inner.y + row.checked_sub(scroll)?);
    inner.contains(at).then_some(at)
}

/// 把一块画进 `rect`，比 `rect` 高的照光标那一行往上滚（Miyu `ui/oobe/mod.rs` 的 `place`，不左右挪）。交回滚了几行。
fn place(frame: &mut Frame, rect: Rect, built: &Content) -> u16 {
    let len = built.len();
    let focus = built.caret.map(|c| c.0).or(built.focus).unwrap_or(0);
    let scroll = if len > rect.height {
        (focus + 3)
            .saturating_sub(rect.height)
            .min(len - rect.height)
    } else {
        0
    };
    let buf = frame.buffer_mut();
    for (i, line) in built
        .lines
        .iter()
        .skip(usize::from(scroll))
        .take(usize::from(rect.height))
        .enumerate()
    {
        let y = rect.y + u16::try_from(i).unwrap_or(u16::MAX);
        buf.set_line(rect.x, y, line, rect.width);
    }
    scroll
}
