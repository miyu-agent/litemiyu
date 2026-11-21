//! 悬浮窗（Miyu 终端 `ui/floating.rs`，蓝图「配置页」第 14、15、20 条）：浅一档的底、不画边框；标题、一行一项，选中的那一行
//! 左边强调色竖条、底色深一档；报错、按钮、按键提示。配置界面的几种窗都照它画，盖在整屏正中，背后调暗（`mod.rs`）。
//! 点得到的行、按钮记进 [`Hits`]（`settings/mouse.rs`）。

use ratatui::layout::{Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use super::super::frame::Frame;
use crate::settings::{Hit, Hits};
use crate::theme;

/// 窗里的一行行，画之前先排好。
#[derive(Default)]
pub struct Lines {
    /// 标题和右边暗着写的一截。
    pub title: Option<(String, String)>,
    /// 一行行。
    pub body: Vec<Body>,
    /// 出错的一句（红字，写在按钮上面）。
    pub error: Option<String>,
    /// 按钮：名字、是不是删除那种。
    pub buttons: Vec<(String, bool)>,
    /// 按钮的快捷键，暗着写在按钮名字后面；没有的不写。
    pub keys: Vec<String>,
    /// 停在第几个按钮上。
    pub button: Option<usize>,
    /// 按键提示。
    pub hints: Vec<[String; 2]>,
    /// 正在改的那一行：第几行、光标在第几列。
    pub caret: Option<(usize, u16)>,
    /// 前几行钉在窗顶不滚（Miyu `Lines::fixed`，9c2f8691：池的窗到搜索框为止）；0 是整个一起滚。
    pub fixed: usize,
    /// 照哪一行滚（Miyu `Lines::focus`）：光标画在一行里的一格上、整行不铺底的（预设的窗的两列）记在这里；没有的照选中的那一行。
    pub focus: Option<usize>,
}

/// 一行：字、选中没有。
pub struct Body {
    /// 字。
    pub line: Line<'static>,
    /// 选中的（铺底、左边竖条）。
    pub focused: bool,
    /// 点它是哪一处；点不动的没有。
    pub hit: Option<Hit>,
}

impl Lines {
    /// 标题、右边暗着写的一截。
    pub fn title(&mut self, title: String, sub: String) {
        self.title = Some((title, sub));
    }

    /// 选不中的一行。
    pub fn plain(&mut self, line: Line<'static>) {
        self.body.push(Body {
            line,
            focused: false,
            hit: None,
        });
    }

    /// 选得中的一行。
    pub fn row(&mut self, line: Line<'static>, focused: bool) {
        self.body.push(Body {
            line,
            focused,
            hit: None,
        });
    }

    /// 选得中、点得动的一行：点它是 `hit`。
    pub fn target(&mut self, line: Line<'static>, focused: bool, hit: Hit) {
        self.body.push(Body {
            line,
            focused,
            hit: Some(hit),
        });
    }

    /// 窗有多高：一行一项，加上标题、按钮、按键提示和中间的空行，报错折几行就多几行；最高 `max`。
    pub fn height(&self, width: u16, max: u16) -> u16 {
        let errors = self.error.as_deref().map_or(0, |e| {
            error_rows(e, usize::from(width.saturating_sub(4))).len()
        });
        let rows = u16::try_from(self.body.len() + 8 + errors).unwrap_or(u16::MAX);
        rows.min(max)
    }
}

/// 在 `area` 正中画一个 `width` 宽、最高 `max_height` 的窗；点得到的行、按钮记进 `hits`；交回输入光标的位置（有正在改的
/// 一行时）。窗里从第几行露起照 `hits` 记着的上一帧接着（Miyu `popup_top`）。
pub fn centered(
    frame: &mut Frame,
    area: Rect,
    size: (u16, u16),
    lines: &Lines,
    hits: &Hits,
) -> Option<Position> {
    centered_from(frame, area, size, lines, (hits, hits.top_cell()))
}

/// 同 [`centered`]，窗里从第几行露起照 `top` 接着（叠在别的窗下面、自己记着滚到哪的窗用：预设的窗）。
pub fn centered_from(
    frame: &mut Frame,
    area: Rect,
    (width, max_height): (u16, u16),
    lines: &Lines,
    (hits, top): (&Hits, &std::cell::Cell<usize>),
) -> Option<Position> {
    let height = lines.height(width, max_height);
    let rect = Rect::new(
        area.x + (area.width.saturating_sub(width)) / 2,
        area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    );
    paint(frame, rect, lines, (hits, top))
}

/// 铺底色、写标题、滚着写一行行、出错的一句、按钮、按键提示。
fn paint(
    frame: &mut Frame,
    rect: Rect,
    lines: &Lines,
    (hits, top_cell): (&Hits, &std::cell::Cell<usize>),
) -> Option<Position> {
    // 先把格子整个清掉（字色、修饰都不留，不然窗里的字带着后面那页的暗色），再铺窗自己的底（`theme::float`）：
    // 铺不铺整屏的底色都盖得住后面。
    let buf = frame.buffer_mut();
    theme::float(buf, rect);
    let inner = Rect::new(
        rect.x + 2,
        rect.y + 1,
        rect.width.saturating_sub(4),
        rect.height.saturating_sub(2),
    );
    if let Some((title, sub)) = &lines.title {
        buf.set_string(
            inner.x,
            inner.y,
            fit(title, usize::from(inner.width)),
            theme::accent().add_modifier(Modifier::BOLD),
        );
        let w = u16::try_from(sub.width()).unwrap_or(u16::MAX);
        let title_w = u16::try_from(title.width()).unwrap_or(u16::MAX);
        if !sub.is_empty() && w.saturating_add(title_w).saturating_add(2) < inner.width {
            buf.set_string(inner.right() - w, inner.y, sub, theme::dim());
        }
    }
    let errors = lines
        .error
        .as_deref()
        .map(|e| error_rows(e, usize::from(inner.width)))
        .unwrap_or_default();
    let error_count = u16::try_from(errors.len()).unwrap_or(u16::MAX);
    let room = usize::from(
        inner
            .height
            .saturating_sub(6u16.saturating_add(error_count)),
    );
    let focus = lines
        .focus
        .or_else(|| lines.body.iter().position(|b| b.focused))
        .unwrap_or(0);
    // 钉在顶上的几行（`fixed`）：后面那一段至少露三行，窗太矮时钉住的先让出来、跟着滚（Miyu `paint`，9c2f8691）。后面那一段
    // 照上一帧露的接着露，选中的碰到上下边才挪（`menu::edge`，Miyu `popup_top`）。
    let fixed = lines
        .fixed
        .min(lines.body.len())
        .min(room.saturating_sub(3));
    let scroll = room - fixed;
    let rest = lines.body.len() - fixed;
    let prev = top_cell.get();
    let top = if focus >= fixed {
        crate::menu::edge(focus - fixed, None, prev, rest, scroll)
    } else {
        prev.min(rest.saturating_sub(scroll))
    };
    let top = if fixed == 0 && lines.caret.is_some_and(|(row, _)| row == 0) {
        0
    } else {
        top
    };
    top_cell.set(top);
    let shown = (0..fixed).chain(fixed + top..(fixed + top + scroll).min(lines.body.len()));
    let mut caret = None;
    for (offset, at) in shown.enumerate() {
        let body = &lines.body[at];
        let y = inner.y + 2 + u16::try_from(offset).unwrap_or(u16::MAX);
        let row = Rect::new(inner.x, y, inner.width, 1);
        if body.focused {
            frame.buffer_mut().set_style(row, theme::row_focus());
            bar(frame, row);
        }
        frame.buffer_mut().set_line(row.x, y, &body.line, row.width);
        // 没有颜色可靠的（`theme::arrows`）只靠行首的箭头看出选中：每一行行首空两格，箭头在字后面再写一遍，不让那两格空白盖掉
        // （Miyu `paint`，8a2c7d25）。有颜色的照旧靠底色。
        if body.focused && theme::arrows() {
            bar(frame, row);
        }
        if let Some(hit) = body.hit {
            hits.push(row, hit);
        }
        if let Some((caret_at, x)) = lines.caret
            && caret_at == at
        {
            caret = Some(Position::new(
                (row.x + x).min(row.right().saturating_sub(1)),
                y,
            ));
        }
    }
    let mut y = inner.bottom().saturating_sub(3);
    // 报错、按钮、按键提示：落在窗里的才画。
    let inside = |y: u16| y >= inner.y && y < inner.bottom();
    // 报错折行写全（最多三行），写在按钮上面。
    for (i, text) in errors.iter().rev().enumerate() {
        let up = u16::try_from(i).unwrap_or(u16::MAX).saturating_add(1);
        if let Some(above) = y.checked_sub(up).filter(|&r| inside(r)) {
            frame
                .buffer_mut()
                .set_string(inner.x, above, text, theme::error());
        }
    }
    if inside(y) {
        let placed = keyed_row(
            frame,
            Position::new(inner.x, y),
            (&lines.buttons, &lines.keys),
            lines.button,
        );
        for (i, rect) in placed.into_iter().enumerate() {
            hits.push(rect, Hit::Button(i));
        }
    }
    // 按钮和按键提示之间空一行。
    y += 2;
    if inside(y) {
        frame.buffer_mut().set_line(
            inner.x,
            y,
            &hint_line_within(&lines.hints, usize::from(inner.width)),
            inner.width,
        );
    }
    caret
}

/// 一排按钮从 `at` 往右画：`[ 名字 ]`，隔两列；停着的铺选中色，危险的停着时反白红色。第几个按钮在 `keys` 里有快捷键的，
/// 名字后面空一格暗着写它：`[ 保存 s ]`。交回每个按钮画在哪（点得到的地方）。
pub fn keyed_row(
    frame: &mut Frame,
    at: Position,
    (buttons, keys): (&[(String, bool)], &[String]),
    selected: Option<usize>,
) -> Vec<Rect> {
    let mut placed = Vec::new();
    let mut x = at.x;
    for (i, (label, danger)) in buttons.iter().enumerate() {
        let style = match (selected == Some(i), danger) {
            (true, true) => theme::error().add_modifier(Modifier::REVERSED),
            (true, false) => theme::picked_bar(),
            _ => Style::new(),
        };
        let key = keys.get(i).filter(|k| !k.is_empty());
        let key_style = if selected == Some(i) {
            style
        } else {
            theme::dim()
        };
        let mut spans = vec![Span::styled(format!("[ {label}"), style)];
        if let Some(key) = key {
            spans.push(Span::styled(" ", style));
            spans.push(Span::styled(key.clone(), key_style));
        }
        spans.push(Span::styled(" ]", style));
        let line = Line::from(spans);
        let w = u16::try_from(line.width()).unwrap_or(u16::MAX);
        frame.buffer_mut().set_line(x, at.y, &line, w);
        placed.push(Rect::new(x, at.y, w, 1));
        x = x.saturating_add(w).saturating_add(2);
    }
    placed
}

/// 报错折成几行：最多三行，还放不下的第三行截掉加「…」。
fn error_rows(error: &str, width: usize) -> Vec<String> {
    let mut rows = crate::cells::wrap(error, width.max(1));
    if rows.len() > 3 {
        let rest: String = rows.split_off(2).concat();
        rows.push(fit(&rest, width));
    }
    rows
}

/// 一行按键提示：键暗色、说明更暗，每格隔三列（Miyu「配置页」第 42 条）；放不进 `width` 列时把几条之间的空收窄（两格、一格）；隔一格还放不下的从后面整条去掉，不切半条。
pub fn hint_line_within(hints: &[[String; 2]], width: usize) -> Line<'static> {
    if let Some(line) = (1..=3)
        .rev()
        .map(|gap| gapped(hints, gap))
        .find(|line| line.width() <= width)
    {
        return line;
    }
    (0..hints.len())
        .rev()
        .map(|keep| gapped(&hints[..keep], 1))
        .find(|line| line.width() <= width)
        .unwrap_or_default()
}

fn gapped(hints: &[[String; 2]], gap: usize) -> Line<'static> {
    let mut spans = Vec::new();
    for (i, [key, what]) in hints.iter().enumerate() {
        if i > 0 {
            spans.push(Span::raw(" ".repeat(gap)));
        }
        spans.push(Span::styled(key.clone(), theme::dim()));
        spans.push(Span::styled(format!(" {what}"), theme::faint()));
    }
    Line::from(spans)
}

/// 选中的那一行左边的竖条（Miyu 默认主题的 `bar`）。
pub const BAR: &str = "┃";

/// 没有颜色可靠时选中行行首的箭头（Miyu 主题的 `marks.pointer`）。
pub const POINTER: &str = "❯ ";

/// 选中的那一行左边的竖条：强调色。没有颜色可靠的（`theme::arrows`）换成箭头，只写第一行：行首两格（Miyu `bar`，8a2c7d25）。
pub fn bar(frame: &mut Frame, row: Rect) {
    if theme::arrows() {
        frame
            .buffer_mut()
            .set_string(row.x, row.y, POINTER, theme::picked());
        return;
    }
    for y in row.y..row.bottom() {
        frame
            .buffer_mut()
            .set_string(row.x, y, BAR, theme::picked_mark());
    }
}

/// 写成一行字的那几处（主菜单、清单页、MCP、技能页）选中行行首那两格：有颜色的强调色竖条，没有颜色可靠的箭头、只写这一项的
/// 第一行（`first`）；没选中的空两格。同 [`bar`]。
pub fn lead(chosen: bool, first: bool) -> Span<'static> {
    match (chosen, theme::arrows()) {
        (true, false) => Span::styled(format!("{BAR} "), theme::accent()),
        (true, true) if first => Span::styled(POINTER, theme::picked()),
        _ => Span::raw("  "),
    }
}

/// 字太长裁掉、末尾写「…」。
pub fn fit(text: &str, width: usize) -> String {
    if text.width() <= width {
        return text.to_string();
    }
    let mark = crate::sheet::ellipsis();
    let mut out = String::new();
    for c in text.chars() {
        let w = unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
        if out.width() + w + mark.width() > width {
            break;
        }
        out.push(c);
    }
    out.push_str(&mark);
    out
}
