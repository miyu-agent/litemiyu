//! 明暗和铺底色（Miyu 终端 `theme/choice.rs` 的 `Mode`、`Choice`，`theme/ground.rs`；蓝图「主题」第 3、4 条）：配置项
//! `tui.theme_mode`（自动、深色、浅色）、`tui.theme_background`（开、关）。铺底色是整个界面没上底色的格子铺这一套的底色，
//! 没上色的字用这一套的字色；终端那两套底色是 `Reset`，不铺。画完一帧、换色深之前做（16 色、`NO_COLOR` 照色深换掉）。

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};

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

/// 悬浮窗先占住它那一块（Miyu `ui/floating.rs`）：格子整个清掉（字、字色、修饰都不留，不然窗里露着后面那页的字和
/// 暗色），再铺悬浮窗的底。铺不铺整屏的底色（[`paint`]）只管没上底色的格子，窗自己的底照样铺，盖得住后面。
pub fn float(buf: &mut Buffer, rect: Rect) {
    let rect = rect.intersection(buf.area);
    for y in rect.top()..rect.bottom() {
        for x in rect.left()..rect.right() {
            buf[(x, y)].reset();
        }
    }
    buf.set_style(rect, panel());
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
