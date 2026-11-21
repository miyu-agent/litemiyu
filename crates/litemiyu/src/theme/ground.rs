//! 明暗和铺底色（Miyu 终端 `theme/choice.rs` 的 `Mode`、`Choice`，`theme/ground.rs`；蓝图「主题」第 3、4 条）：配置项
//! `tui.theme_mode`（自动、深色、浅色）、`tui.theme_background`（开、关）。铺底色是整个界面没上底色的格子铺这一套的底色，
//! 没上色的字用这一套的字色；终端那两套底色是 `Reset`，不铺。画完一帧、换色深之前做（16 色、`NO_COLOR` 照色深换掉）。

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use unicode_width::UnicodeWidthStr;

/// kitty 的图占着的格子写这个字，前景色是图的编号（同 `depth.rs`）：只铺底色，前景色不碰。
const KITTY_PLACEHOLDER: char = '\u{10EEEE}';

/// 明暗。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// 跟随终端：照终端的底色挑，问不到的当深色。
    Auto,
    /// 深色。
    Dark,
    /// 浅色。
    Light,
}

impl Mode {
    /// 读配置里的写法；认不得的是 `None`。
    pub fn read(text: &str) -> Option<Self> {
        match text {
            "auto" => Some(Self::Auto),
            "dark" => Some(Self::Dark),
            "light" => Some(Self::Light),
            _ => None,
        }
    }
}

/// 明暗、铺不铺底色。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Choice {
    /// 明暗。
    pub mode: Mode,
    /// 铺底色（出厂开）。
    pub ground: bool,
}

impl Choice {
    /// 出厂的：明暗自动、铺底色。
    pub const DEFAULT: Choice = Choice {
        mode: Mode::Auto,
        ground: true,
    };

    /// 用深色那一份：手动选的照选的，跟随终端的照终端的底色（`terminal_dark`），问不到的当深色。
    pub fn dark(self, terminal_dark: Option<bool>) -> bool {
        match self.mode {
            Mode::Dark => true,
            Mode::Light => false,
            Mode::Auto => terminal_dark.unwrap_or(true),
        }
    }
}

/// 没上底色的格子铺 `bg`，没上色的字用 `fg`。kitty 的图占着的格子也铺底色、不动前景色：图透明的地方露的是格子的底色。
pub fn paint(buf: &mut Buffer, bg: Color, fg: Color) {
    for cell in &mut buf.content {
        if cell.bg == Color::Reset {
            cell.bg = bg;
        }
        if cell.fg == Color::Reset && !cell.symbol().starts_with(KITTY_PLACEHOLDER) {
            cell.fg = fg;
        }
    }
}

/// 悬浮窗的底：比页面浅一档（Miyu `theme::panel`）；正文顶上钉着的那一行也铺它（`ui/pinned.rs`）。
pub fn panel() -> Style {
    Style::new().bg(super::pick(|p| p.panel_bg))
}

/// 悬浮窗先占住它那一块（Miyu `ui/floating.rs`）：格子整个清掉（[`claim`]），再铺悬浮窗的底。铺不铺整屏的底色（[`paint`]）
/// 只管没上底色的格子，窗自己的底照样铺，盖得住后面。
pub fn float(buf: &mut Buffer, rect: Rect) {
    claim(buf, rect);
    buf.set_style(rect.intersection(buf.area), panel());
}

/// 浮在别的东西上面的框、列表、提示先占住它那一块（LM-38，同 Miyu 终端 11-20 项目主人验收：「确认弹窗的背景，后面列表里的
/// 中文字会把弹窗底色顶掉」）：格子整个清掉（字、字色、修饰都不留，不然框里露着后面的字和暗色）；左右两边压在框边上的宽字
/// 截掉，框外的那半格换成带它原来样子的空格。不截的话，框左边外面那个宽字的后半占着框的第一列，画完一帧清宽字后面那格时
/// （`ui/wide.rs`）把框的第一列清成后面那一行的样子，终端上也只画宽字、不画框的那一格；框右边里面那个宽字的后半留在框外。
pub fn claim(buf: &mut Buffer, rect: Rect) {
    let area = buf.area;
    let rect = rect.intersection(area);
    if rect.is_empty() {
        return;
    }
    for y in rect.top()..rect.bottom() {
        // 左边：框外紧挨着的那一格是伸进框里的宽字（占两格的前半）。
        if rect.left() > area.left() {
            let cell = &mut buf[(rect.left() - 1, y)];
            if cell.symbol().width() > 1 {
                cell.set_symbol(" ");
            }
        }
        // 右边：框里最后一格是伸出框外的宽字，框外那几格换成带它样子的空格。
        let last = &buf[(rect.right() - 1, y)];
        let (wide, style) = (last.symbol().width(), last.style());
        let end = (rect.right() - 1).saturating_add(u16::try_from(wide).unwrap_or(1));
        for x in rect.right()..end.min(area.right()) {
            let cell = &mut buf[(x, y)];
            cell.reset();
            cell.set_style(style);
        }
        for x in rect.left()..rect.right() {
            buf[(x, y)].reset();
        }
    }
}

#[cfg(test)]
mod tests {
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::style::Color;

    use super::{Choice, Mode, float, paint};

    #[test]
    fn a_painted_frame_fills_only_what_has_no_color_and_keeps_the_picture_ids() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 3, 1));
        buf[(1, 0)].set_fg(Color::Red).set_bg(Color::Blue);
        buf[(2, 0)].set_symbol("\u{10EEEE}");
        let (bg, fg) = (Color::Rgb(0xe1, 0xe2, 0xe7), Color::Rgb(0x37, 0x60, 0xbf));
        paint(&mut buf, bg, fg);
        assert_eq!((buf[(0, 0)].bg, buf[(0, 0)].fg), (bg, fg), "没上色的铺上");
        assert_eq!(
            (buf[(1, 0)].bg, buf[(1, 0)].fg),
            (Color::Blue, Color::Red),
            "上了色的不动"
        );
        assert_eq!(
            (buf[(2, 0)].bg, buf[(2, 0)].fg),
            (bg, Color::Reset),
            "图占着的格子铺底色，前景色是图的编号、不碰"
        );
    }

    #[test]
    fn a_floating_window_clears_what_is_behind_and_lays_its_own_face() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 4, 2));
        buf.set_string(0, 0, "abcd", ratatui::style::Style::new().fg(Color::Red));
        buf.set_string(0, 1, "efgh", ratatui::style::Style::new());
        float(&mut buf, Rect::new(1, 0, 2, 9));
        // 哪一套都有悬浮窗的底（别的测试可能同时换了配色，这里不比是哪一格）。
        let face = buf[(1, 0)].bg;
        assert_ne!(face, Color::Reset, "悬浮窗总有自己的底");
        for x in 1..3 {
            for y in 0..2 {
                let cell = &buf[(x, y)];
                assert_eq!((cell.symbol(), cell.bg, cell.fg), (" ", face, Color::Reset));
            }
        }
        assert_eq!(buf[(0, 0)].symbol(), "a", "窗外的不动");
        assert_eq!(buf[(3, 1)].symbol(), "h");
        // 铺整屏底色在后面：窗的底已经上了色，不被盖掉。
        paint(&mut buf, Color::Rgb(1, 2, 3), Color::Rgb(4, 5, 6));
        assert_eq!(buf[(1, 0)].bg, face);
    }

    /// LM-38（同 Miyu 终端 11-20 项目主人验收：确认弹窗的背景被后面列表里的中文字顶掉）：后面铺满中文（宽字从双数列起），
    /// 框放在单数列（左右两边都压在宽字中间）、双数列（只有右边压着）各一次，铺整屏底色的和不铺的（16 色、终端默认的底）各一次。
    /// 照一帧的做法画完（清宽字后面那格、铺底色）以后，框里每一格都是框的底；压在框边上的宽字截掉，框外那半格是带着后面那一行
    /// 样子的空格；从上一帧（只有列表）换到这一帧时，框的第一列每一行都写给终端（不被宽字跳过）。
    #[test]
    fn a_box_over_wide_characters_keeps_its_whole_face_lm38() {
        use ratatui::style::Style;
        use unicode_width::UnicodeWidthStr;
        let area = Rect::new(0, 0, 20, 3);
        let list = Style::new().bg(Color::Blue);
        let listed = || {
            let mut buf = Buffer::empty(area);
            for y in 0..3 {
                buf.set_string(0, y, "中".repeat(10), list);
            }
            crate::ui::wide::tidy(&mut buf);
            buf
        };
        for (x, width) in [(3u16, 6u16), (4, 7)] {
            for ground in [Some(Color::Rgb(1, 2, 3)), None] {
                let frame = |buf: &mut Buffer| {
                    crate::ui::wide::tidy(buf);
                    if let Some(bg) = ground {
                        paint(buf, bg, Color::Rgb(4, 5, 6));
                    }
                };
                let mut before = listed();
                frame(&mut before);
                let mut buf = listed();
                let rect = Rect::new(x, 0, width, 3);
                float(&mut buf, rect);
                let face = buf[(x, 0)].bg;
                frame(&mut buf);
                let case = format!("框在第 {x} 列、宽 {width}，铺底色 {ground:?}");
                assert_ne!(face, Color::Blue, "{case}：框有自己的底");
                for y in 0..3 {
                    for col in rect.left()..rect.right() {
                        let cell = &buf[(col, y)];
                        assert_eq!(
                            (cell.symbol(), cell.bg),
                            (" ", face),
                            "{case}：框里第 {col} 列第 {y} 行是框的底"
                        );
                    }
                    // 左边：单数列的框压着「中」的后半，「中」截成空格；双数列的框外整个「中」留着。
                    let (left, right) = (rect.left(), rect.right());
                    if left % 2 == 1 {
                        let cut = &buf[(left - 1, y)];
                        assert_eq!(
                            (cut.symbol(), cut.bg),
                            (" ", Color::Blue),
                            "{case}：左边截掉"
                        );
                    } else {
                        assert_eq!(buf[(left - 2, y)].symbol(), "中", "{case}：左边整个的留着");
                    }
                    // 右边：两种都压着框里最后一个「中」，框外那半格是空格、后面那一行的底。
                    let cut = &buf[(right, y)];
                    assert_eq!(
                        (cut.symbol(), cut.bg),
                        (" ", Color::Blue),
                        "{case}：右边截掉"
                    );
                    assert_eq!(buf[(right + 1, y)].symbol(), "中", "{case}：再往右的留着");
                    // 框外没有哪个宽字伸进框里。
                    for col in 0..left {
                        let wide = u16::try_from(buf[(col, y)].symbol().width()).unwrap_or(1);
                        assert!(
                            col + wide.max(1) <= left,
                            "{case}：第 {col} 列的宽字伸进了框"
                        );
                    }
                }
                let written: Vec<(u16, u16)> =
                    before.diff(&buf).iter().map(|(x, y, _)| (*x, *y)).collect();
                for y in 0..3 {
                    assert!(
                        written.contains(&(x, y)),
                        "{case}：框的第一列第 {y} 行写给了终端：{written:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn the_mode_follows_the_terminal_unless_chosen() {
        let auto = Choice::DEFAULT;
        assert!(auto.dark(None), "问不到的当深色");
        assert!(!auto.dark(Some(false)));
        let light = Choice {
            mode: Mode::Light,
            ground: true,
        };
        assert!(!light.dark(Some(true)));
        assert_eq!(Mode::read("dark"), Some(Mode::Dark));
        assert_eq!(Mode::read("x"), None);
    }
}
