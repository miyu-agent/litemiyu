//! 自成一块的几样：代码块（蓝图 `tui.md`「她的回答：Markdown」第 7 条），块级公式。
//!
//! Miyu 把图片、块级公式、mermaid 另排成一张图，画不成图时才照字写；LiteMiyu 界面里不画图（`docs/00-设计.md` 第五节
//! 第 12 条）：公式一律写成 Unicode，mermaid 照代码块写，图片只有 `[图片: …]` 那一行。

use ratatui::style::Style;
use ratatui::text::Span;
use unicode_width::UnicodeWidthStr;

use super::inline::{Folded, Piece};
use super::{MdLine, Renderer, code, math};
use crate::theme;

/// 围栏代码块收完了没有：`block` 是它在原文里的那一截，最后一个非空行是一排（至少三个）``` 或 ~~~，
/// 而且不是开头那一行。
pub(super) fn closed(block: &str) -> bool {
    let mut lines = block.trim_end().lines();
    let opened = lines.next().is_some();
    let fence =
        |t: &str| t.len() >= 3 && (t.chars().all(|c| c == '`') || t.chars().all(|c| c == '~'));
    opened && lines.last().is_some_and(|l| fence(l.trim()))
}

impl Renderer<'_> {
    /// 代码块：上下两条框线，代码按语言着色，太长的折行。语言写 `math` 的（GitHub 的写法）、收完了的照块级公式写
    /// （2026-10-19 项目主人，`latex`、`tex` 的照代码）；没收完的（回答还在流）照代码写。
    pub(super) fn code_block(&mut self) {
        let Some((lang, body)) = self.code.take() else {
            return;
        };
        if lang.eq_ignore_ascii_case("math") && self.code_closed {
            self.display_math(&body);
        } else {
            self.code_lines(&lang, &body);
        }
    }

    /// 块级公式：先把前面收着的字排出去，自己占一块，写成 Unicode：分式上下摞、环境分行分列（`stack.rs`），
    /// 摞出来比正文宽的照一行写（折开就对不齐了）。
    pub(super) fn display_math(&mut self, tex: &str) {
        self.flush();
        let room = usize::from(self.room());
        let lines = super::stack::stacked(tex, self.math.get(), room);
        self.emit(vec![Piece::new(lines.join("\n"), theme::md_math())], true);
    }

    /// 行内公式：转成 Unicode，公式色。
    pub(super) fn inline_math(&mut self, tex: &str) {
        let text = math::unicode(tex, self.math.get());
        self.push(&text, theme::md_math());
    }

    pub(super) fn code_lines(&mut self, lang: &str, body: &str) {
        let room = usize::from(self.room());
        // 上面那条照主题的 `frame.code`：左上角、横线（出厂 `╭─ code`）。
        let set = crate::sheet::read(|s| s.frame.code.lines());
        let (corner, line) = (set.top_left, set.horizontal);
        let label = if lang.is_empty() {
            format!("{corner}{line} code ")
        } else {
            format!("{corner}{line} code {lang} ")
        };
        let top = format!("{label}{}", line.repeat(room.saturating_sub(label.width())));
        // 列表这一项头一样就是代码块的：记号写在上面那条框线前面（2026-10-23 review：原来写在框里第一行代码前面）。
        self.frame(top, true);
        // 一个代码块一个着色的：块注释、三引号字符串接到下一行（`tui.md`「代码着色」第 2 条）。
        let mut colors = code::Highlighter::new(self.languages.find(lang));
        for line in body.trim_end_matches('\n').split('\n') {
            let pieces = colors.line(line);
            let pieces = if pieces.is_empty() {
                vec![Piece::new(" ", Style::new())]
            } else {
                pieces
            };
            self.emit(pieces, true);
        }
        self.frame(line.repeat(room), false);
    }

    fn frame(&mut self, text: String, first: bool) {
        let lead = self.lead(first);
        let folded = Folded {
            spans: vec![Span::styled(text, theme::dim())],
            links: Vec::new(),
            joined: false,
        };
        self.out.push(MdLine {
            lead,
            folded,
            copy: false,
            details: None,
        });
    }
}
