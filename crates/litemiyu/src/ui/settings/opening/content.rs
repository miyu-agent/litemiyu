//! 开头里中间那一块的一行行（照 Miyu 终端 `ui/oobe/content.rs` 搬，去掉几行字的格、头像、色块那几样 LiteMiyu 没有的）：标题、
//! 暗色说明、选一行的列表、打字的格子、左右换的一格、动作行、能滚的长列表。

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use crate::settings::opening::{DOT, Field, unlink};
use crate::theme;

/// 光标停着的那一行前面的记号（Miyu 主题的 `marks.pointer`）。
pub const POINTER: &str = "❯ ";
/// 没选中的行前面空两格。
const GAP: &str = "  ";
/// 一格最多露几行（Miyu `FIELD_ROWS`）。
const FIELD_ROWS: usize = 5;

/// 拼好的一块。
#[derive(Debug, Default)]
pub struct Content {
    /// 一行行。
    pub lines: Vec<Line<'static>>,
    /// 多宽。
    pub width: u16,
    /// 最多多高（长列表照它开窗）；0 是不限。
    pub height: u16,
    /// 光标停着的是第几行。
    pub focus: Option<u16>,
    /// 在编辑的那一格的输入光标：第几行、第几列。
    pub caret: Option<(u16, u16)>,
    /// 光标停着、没在编辑的格子右端暗色写的（「Enter 编辑」「Enter 选择」）。
    pub cue: String,
}

impl Content {
    /// `width` 列宽、最多 `height` 行高的一块。
    pub fn sized(width: u16, height: u16) -> Self {
        Self {
            width,
            height,
            ..Self::default()
        }
    }

    /// 有几行。
    pub fn len(&self) -> u16 {
        u16::try_from(self.lines.len()).unwrap_or(u16::MAX)
    }

    /// 标题（加粗）、暗色一句说明（照宽折行），下面空一行。
    pub fn heading(&mut self, title: &str, sub: &str) {
        self.lines.push(Line::styled(
            title.to_string(),
            Style::new().add_modifier(Modifier::BOLD),
        ));
        if !sub.is_empty() {
            self.note(sub, theme::dim());
        }
        self.blank();
    }

    /// 空一行。
    pub fn blank(&mut self) {
        self.lines.push(Line::default());
    }

    /// 一段字，照块宽折行。
    pub fn note(&mut self, text: &str, style: Style) {
        self.note_at(0, text, style);
    }

    /// 一段字，每一行都往右缩 `indent` 列、在剩下的宽里折行（和格子的值对齐的说明）。
    pub fn note_at(&mut self, indent: usize, text: &str, style: Style) {
        let room = usize::from(self.width).saturating_sub(indent).max(1);
        let lead = " ".repeat(indent);
        let room = u16::try_from(room).unwrap_or(u16::MAX);
        for (row, _) in crate::input::pieces(text, room) {
            self.lines
                .push(Line::styled(format!("{lead}{}", row.trim_start()), style));
        }
    }

    /// 选一行的列表的一行：选着的前面 `❯`、铺底色到整块宽；右边那一截靠右。
    pub fn row(&mut self, selected: bool, left: Vec<Span<'static>>, right: Option<Span<'static>>) {
        if selected {
            self.focus = Some(self.len());
        }
        let lead = if selected {
            Span::styled(POINTER, theme::accent())
        } else {
            Span::raw(GAP)
        };
        let mut spans = vec![lead];
        spans.extend(left);
        if let Some(right) = right {
            let used: usize = spans.iter().map(|s| s.content.width()).sum();
            let pad = usize::from(self.width).saturating_sub(used + right.content.width());
            spans.push(Span::raw(" ".repeat(pad.max(1))));
            spans.push(right);
        }
        self.lines
            .push(fill(spans, self.width, selected.then(theme::row_focus)));
    }

    /// 段名（暗、加粗），不能选。
    pub fn section(&mut self, name: &str) {
        self.lines.push(Line::styled(
            name.to_string(),
            theme::dim().add_modifier(Modifier::BOLD),
        ));
    }

    /// 打字的一格：左边名字（照 `label_w` 对齐），右边铺底色的字；空着的暗色写 `hint`。光标在的那一格前面 `❯`、名字强调色，
    /// 格子浅一档；在编辑的记下输入光标，没在编辑的格子右端写 [`Content::cue`]。
    pub fn field(&mut self, label: &str, label_w: usize, field: &Field, hint: &str, focused: bool) {
        let cue = if focused {
            self.cue.clone()
        } else {
            String::new()
        };
        self.boxed_field((label, label_w), field, hint, focused, &cue);
    }

    /// 搜索那一格（搜模型）：在搜的时候才算光标停着；没在搜时右端写 `cue`。
    pub fn search(&mut self, label: &str, label_w: usize, field: &Field, cue: &str) {
        self.boxed_field((label, label_w), field, "", field.editing(), cue);
    }

    /// 打字的格子（Miyu `boxed_field`）：字长了照格子宽折行，最多露 [`FIELD_ROWS`] 行，光标碰到上边、下边才挪（`menu::edge`）；
    /// 密钥不折行，太长的只露后面那一截（光标多半在最后）。没在编辑时第一行右端写 `cue`。
    fn boxed_field(
        &mut self,
        (label, label_w): (&str, usize),
        field: &Field,
        hint: &str,
        focused: bool,
        cue: &str,
    ) {
        let lead_w = GAP.width() + label_w + 2;
        let room = usize::from(self.width).saturating_sub(lead_w + 1).max(4);
        let ground = ground(focused);
        if focused {
            self.focus = Some(self.len());
        }
        let shown = if field.unlinked && !field.editing() {
            unlink(&field.shown())
        } else {
            field.shown()
        };
        let hint = if field.unlinked {
            unlink(hint)
        } else {
            hint.to_string()
        };
        let before = &field.text()[..field.editor.cursor().min(field.text().len())];
        let (rows, (row, col)) = if field.secret {
            let dots = shown.chars().count().min(room.saturating_sub(1));
            let at = before.chars().count().min(dots);
            (vec![DOT.repeat(dots)], (0, at))
        } else {
            // 地址那一格画的字多了零宽空格：光标照画的字算。
            let cursor = if field.unlinked && !field.editing() {
                unlink(before).len()
            } else {
                before.len()
            };
            wrap(&shown, cursor.min(shown.len()), room)
        };
        let first = crate::menu::edge(row, None, field.top.get(), rows.len(), FIELD_ROWS);
        field.top.set(first);
        let blank_lead = " ".repeat(lead_w);
        for (i, text) in rows.iter().enumerate().skip(first).take(FIELD_ROWS) {
            let head = if i == first {
                labelled(label, label_w, focused)
            } else {
                vec![Span::raw(blank_lead.clone())]
            };
            let body = if shown.is_empty() {
                Span::styled(hint.clone(), ground.patch(theme::faint()))
            } else {
                Span::styled(text.clone(), ground)
            };
            // 光标只在编辑时出现。
            if focused && field.editing() && i == row {
                self.caret = Some((self.len(), u16::try_from(lead_w + col).unwrap_or(u16::MAX)));
            }
            let cue = if !field.editing() && i == first {
                cue
            } else {
                ""
            };
            let mut spans = head;
            spans.extend(boxed(body, room, cue, ground));
            self.lines.push(Line::from(spans));
        }
    }

    /// 一个动作行（「测试连接 →」「下一步 →」）：停着的前面 `❯`、字变强调色，没停的照常色，都加粗、不铺底。
    pub fn action(&mut self, focused: bool, text: &str) {
        if focused {
            self.focus = Some(self.len());
        }
        let (lead, style) = if focused {
            (Span::styled(POINTER, theme::accent()), theme::accent())
        } else {
            (Span::raw(GAP), Style::new())
        };
        self.lines.push(Line::from(vec![
            lead,
            Span::styled(text.to_string(), style.add_modifier(Modifier::BOLD)),
        ]));
    }

    /// 不打字的一格（配色方案的明暗、铺底色）：和打字的格一样宽、一样的底，停着时右端写 [`Content::cue`]。回车开选择窗。
    pub fn chip(&mut self, label: &str, label_w: usize, text: &str, focused: bool) {
        if focused {
            self.focus = Some(self.len());
        }
        let room = usize::from(self.width)
            .saturating_sub(GAP.width() + label_w + 2 + 1)
            .max(4);
        let ground = ground(focused);
        let cue = if focused {
            self.cue.clone()
        } else {
            String::new()
        };
        let mut spans = labelled(label, label_w, focused);
        spans.extend(boxed(
            Span::styled(text.to_string(), ground),
            room,
            &cue,
            ground,
        ));
        self.lines.push(Line::from(spans));
    }

    /// 左右换的一格（自定义的接口）：只写选着的那一个，两边 `‹` `›`，不铺底色；光标在的时候箭头用强调色。
    pub fn switch(&mut self, label: &str, label_w: usize, name: &str, focused: bool) {
        if focused {
            self.focus = Some(self.len());
        }
        let arrow = if focused {
            theme::accent()
        } else {
            theme::faint()
        };
        let mut spans = labelled(label, label_w, focused);
        spans.push(Span::styled("‹ ", arrow));
        spans.push(Span::raw(name.to_string()));
        spans.push(Span::styled(" ›", arrow));
        self.lines.push(Line::from(spans));
    }

    /// 一长串选一行的（试通了的模型）：比剩下的高度多的开一个窗，照光标滚；上面、下面还有的那一头暗色写一行「还有 N 个」。
    /// `below` 是这串下面还要留几行（红字、转轮）。
    pub fn window(
        &mut self,
        rows: Vec<Vec<Span<'static>>>,
        cursor: usize,
        below: u16,
        more: (&str, &str),
    ) {
        let total = rows.len();
        let room = if self.height == 0 {
            total
        } else {
            usize::from(self.height.saturating_sub(self.len() + below)).max(3)
        };
        let (start, end) = if total <= room {
            (0, total)
        } else {
            let shown = room.saturating_sub(2).max(1);
            let start = cursor.saturating_sub(shown / 2).min(total - shown);
            (start, start + shown)
        };
        let note = |c: &mut Self, text: &str, n: usize| {
            c.lines.push(Line::styled(
                format!("{GAP}{}", text.replace("{n}", &n.to_string())),
                theme::faint(),
            ));
        };
        if total > room {
            if start > 0 {
                note(self, more.0, start);
            } else {
                self.blank();
            }
        }
        for (i, spans) in rows.into_iter().enumerate().take(end).skip(start) {
            self.row(i == cursor, spans, None);
        }
        if total > room && end < total {
            note(self, more.1, total - end);
        }
    }
}

/// 照显示宽度折行（照 Miyu 终端 `oobe/wrap.rs` 的 `wrap` 搬）：交回一行行和光标（字节下标 `cursor`）落在第几行第几列。换行符
/// 另起一行；照字素簇算。
fn wrap(text: &str, cursor: usize, width: usize) -> (Vec<String>, (usize, usize)) {
    use unicode_segmentation::UnicodeSegmentation;
    let mut rows = vec![String::new()];
    let mut caret = (0, 0);
    let mut col = 0;
    for (i, g) in text.grapheme_indices(true) {
        if i == cursor {
            caret = (rows.len() - 1, col);
        }
        if g == "\n" {
            rows.push(String::new());
            col = 0;
            continue;
        }
        let w = g.width();
        if col + w > width && col > 0 {
            rows.push(String::new());
            col = 0;
        }
        if let Some(last) = rows.last_mut() {
            last.push_str(g);
        }
        col += w;
    }
    if cursor >= text.len() {
        caret = (rows.len() - 1, col);
    }
    (rows, caret)
}

/// 一格前面的名字（照 `label_w` 对齐）：光标在的那一格前面 `❯`、名字强调色加粗，别的暗色。
fn labelled(label: &str, label_w: usize, focused: bool) -> Vec<Span<'static>> {
    let (lead, style) = if focused {
        (
            Span::styled(POINTER, theme::accent()),
            theme::accent().add_modifier(Modifier::BOLD),
        )
    } else {
        (Span::raw(GAP), theme::dim())
    };
    vec![
        lead,
        Span::styled(label.to_string(), style),
        Span::raw(" ".repeat(label_w.saturating_sub(label.width()) + 2)),
    ]
}

/// 格子里的一行，补到 `room + 1` 列宽：`cue` 不是空的暗色写在右端，字太长的截掉写「…」。
fn boxed(body: Span<'static>, room: usize, cue: &str, ground: Style) -> Vec<Span<'static>> {
    let total = room + 1;
    if cue.is_empty() {
        let pad = total.saturating_sub(body.content.width());
        return vec![body, Span::styled(" ".repeat(pad), ground)];
    }
    let cue_w = cue.width();
    let room_for_text = u16::try_from(total.saturating_sub(cue_w + 2)).unwrap_or(u16::MAX);
    let text = crate::ui::rows::clip(&body.content, room_for_text);
    let pad = total.saturating_sub(text.width() + cue_w + 1);
    vec![
        Span::styled(text, body.style),
        Span::styled(" ".repeat(pad), ground),
        Span::styled(cue.to_string(), ground.patch(theme::faint())),
        Span::styled(" ", ground),
    ]
}

/// 格子的底：光标在的那一格浅一档，别的铺小块的底。
fn ground(focused: bool) -> Style {
    if focused {
        theme::row_dim()
    } else {
        theme::chip_ground()
    }
}

/// 一行补到 `width` 列宽，铺上 `ground` 的底色（没有的不补）。
fn fill(mut spans: Vec<Span<'static>>, width: u16, ground: Option<Style>) -> Line<'static> {
    let Some(ground) = ground else {
        return Line::from(spans);
    };
    let used: usize = spans.iter().map(|s| s.content.width()).sum();
    spans.push(Span::raw(
        " ".repeat(usize::from(width).saturating_sub(used)),
    ));
    for span in &mut spans {
        span.style = ground.patch(span.style);
    }
    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::{Content, wrap};
    use crate::settings::opening::Field;

    /// 同 Miyu `oobe/wrap.rs` 的 `wrapping_keeps_the_caret_on_its_character`。
    #[test]
    fn wrapping_keeps_the_caret_on_its_character() {
        let (rows, caret) = wrap("ab\ncdef", 4, 3);
        assert_eq!(rows, ["ab", "cde", "f"]);
        assert_eq!(caret, (1, 1), "c 后面");
        let (_, end) = wrap("一二三", "一二三".len(), 4);
        assert_eq!(end, (1, 2), "宽字两列，第三个字折到下一行");
        assert_eq!(wrap("", 0, 5), (vec![String::new()], (0, 0)));
    }

    /// 一格的字长了折成几行（Miyu `boxed_field`）：最多露五行，光标在最后的露最后五行、输入光标在最后一行；没在编辑的照上一帧
    /// 露的接着露。
    #[test]
    fn a_long_field_wraps_into_at_most_five_rows_following_the_caret() {
        let mut field = Field::line();
        field.begin();
        field.paste(&"x".repeat(200));
        // 宽 30、名字两列：格子里一行 30 - (2 + 2 + 2) - 1 = 23 列，200 个字折成 9 行。
        let mut c = Content::sized(30, 0);
        c.field("ID", 2, &field, "", true);
        assert_eq!(c.lines.len(), 5, "最多露五行");
        assert_eq!(c.caret.map(|(row, _)| row), Some(4), "光标在露出的最后一行");
        assert_eq!(field.top.get(), 4, "露的是第 5 到第 9 行");
        let first: String = c.lines[0]
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert!(first.contains("ID"), "名字写在露出的第一行：{first:?}");
        let second: String = c.lines[1]
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert!(
            second.starts_with("      x"),
            "折下来的和字对齐：{second:?}"
        );
        // 光标挪到开头：碰到上边才往上滚。
        field.editor.move_to(0, false);
        let mut c = Content::sized(30, 0);
        c.field("ID", 2, &field, "", true);
        assert_eq!(field.top.get(), 0);
        assert_eq!(c.caret, Some((0, 6)));
    }
}
