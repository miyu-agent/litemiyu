//! 问到的终端颜色（Miyu 终端 `theme/terminal_colors/mod.rs`）：调色板 16 色（OSC 4）、字色和底色（OSC 10、11）、是不是
//! 真彩色。终端配色照它生成（`derived.rs`），流光照它取调色板颜色的真字节（`shimmer.rs`）。
//!
//! Miyu 起来时停下读按键、等终端答（最多 1 秒，`terminal-colorsaurus` 问字色底色）；这里不等，回话由读按键的线程认出来
//! （`answer.rs`），到一句记一句（`Asked`），问的最后一句（底色）到了才照攒下的换色（`mod.rs` 的 `answered`）。

use ratatui::style::Color;

/// 一个颜色的红、绿、蓝。
pub type Rgb = (u8, u8, u8);

/// 0 到 15 号的真颜色。
pub type Colors = [Rgb; 16];

/// 终端自己的字色、底色。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ground {
    /// 字色。
    pub fg: Rgb,
    /// 底色。
    pub bg: Rgb,
    /// 底比字暗（照 `terminal-colorsaurus` 的看法，[`dark`]）。
    pub dark: bool,
}

impl Ground {
    /// 照字色、底色定深浅。
    pub fn of(fg: Rgb, bg: Rgb) -> Ground {
        Ground {
            fg,
            bg,
            dark: dark(fg, bg),
        }
    }
}

/// 问到的。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Seen {
    /// 16 色，16 个都答了才有。
    pub sixteen: Option<Colors>,
    /// 字色、底色。
    pub ground: Option<Ground>,
    /// 显示得了真彩色。
    pub truecolor: bool,
}

impl Seen {
    /// 什么都没问到。
    pub const NONE: Seen = Seen {
        sixteen: None,
        ground: None,
        truecolor: false,
    };
}

/// 这一轮回话攒到的：16 色一号一号到，字色、底色各一句。
#[derive(Debug, Default)]
pub struct Asked {
    sixteen: [Option<Rgb>; 16],
    fg: Option<Rgb>,
}

impl Asked {
    /// 还什么都没到。
    pub const fn new() -> Asked {
        Asked {
            sixteen: [None; 16],
            fg: None,
        }
    }

    /// 调色板的第 `n` 号到了（不在 0 到 15 的不要）。
    pub fn palette(&mut self, n: usize, rgb: Rgb) {
        if let Some(slot) = self.sixteen.get_mut(n) {
            *slot = Some(rgb);
        }
    }

    /// 字色到了。
    pub fn fg(&mut self, rgb: Rgb) {
        self.fg = Some(rgb);
    }

    /// 底色到了（问的最后一句）：交回这一轮问到的，攒着的清掉。16 个都答了才有 16 色；字色没答的没有字色底色。
    pub fn bg(&mut self, bg: Rgb) -> (Option<Colors>, Option<Ground>) {
        let taken = std::mem::take(self);
        let mut colors = [(0, 0, 0); 16];
        let mut whole = true;
        for (slot, got) in colors.iter_mut().zip(taken.sixteen) {
            match got {
                Some(rgb) => *slot = rgb,
                None => whole = false,
            }
        }
        let ground = taken.fg.map(|fg| Ground::of(fg, bg));
        (whole.then_some(colors), ground)
    }
}

/// 一个颜色的真字节，流光照它算渐变：真彩色的原样；调色板 16 色的照问到的；别的（256 色 16 号以后、终端自己的前景色、
/// 没问到的）是 `None`。不是真彩色的也是 `None`：别的色深要把渐变换成最近的 256 色、16 色，一道光扫过去会跳成别的颜色。
pub fn rgb(color: Color, seen: &Seen) -> Option<Rgb> {
    rgb_in(color, seen.sixteen.as_ref().filter(|_| seen.truecolor))
}

fn rgb_in(color: Color, real: Option<&Colors>) -> Option<Rgb> {
    if let Color::Rgb(r, g, b) = color {
        return Some((r, g, b));
    }
    let n = index(color)?;
    real.map(|colors| colors[n])
}

/// 调色板 16 色的序号。
fn index(color: Color) -> Option<usize> {
    Some(match color {
        Color::Black => 0,
        Color::Red => 1,
        Color::Green => 2,
        Color::Yellow => 3,
        Color::Blue => 4,
        Color::Magenta => 5,
        Color::Cyan => 6,
        Color::Gray => 7,
        Color::DarkGray => 8,
        Color::LightRed => 9,
        Color::LightGreen => 10,
        Color::LightYellow => 11,
        Color::LightBlue => 12,
        Color::LightMagenta => 13,
        Color::LightCyan => 14,
        Color::White => 15,
        Color::Indexed(n) if n < 16 => usize::from(n),
        _ => return None,
    })
}

/// 底比字暗：照 `terminal-colorsaurus` 的 `ColorPalette::theme_mode`（Miyu `answer.rs` 的 `dark`）。那个库照 `xterm-color`
/// 的 `perceived_lightness` 比（CIE 的 L*）；这里不带库，照它的算法写。
fn dark(fg: Rgb, bg: Rgb) -> bool {
    let (fg, bg) = (lightness(fg), lightness(bg));
    if bg < fg {
        true
    } else {
        !(bg > fg || bg > 0.5)
    }
}

/// 一个颜色的 L*（0 到 1）：`xterm-color` 1.0 的 `perceived_lightness`，通道照 16 位算（Miyu 把 8 位乘 257 交给它）。
fn lightness((r, g, b): Rgb) -> f32 {
    let linear = |c: u8| {
        let v = f32::from(u16::from(c) * 257) / f32::from(u16::MAX);
        if v <= 0.0 {
            v
        } else if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    let y = 0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b);
    let l = if y <= 216. / 24389. {
        y * (24389. / 27.)
    } else {
        y.cbrt() * 116. - 16.
    };
    l / 100.
}

#[cfg(test)]
mod tests {
    use ratatui::style::Color;

    use super::{Asked, Ground, rgb_in};

    /// 16 色：第 4 号（蓝）是 `#4080ff`，别的是灰。
    fn reply() -> super::Colors {
        let mut colors = [(0, 0, 0); 16];
        for (n, slot) in (0u8..).zip(colors.iter_mut()) {
            *slot = (n * 8, n * 8, n * 8);
        }
        colors[4] = (0x40, 0x80, 0xff);
        colors
    }

    #[test]
    fn palette_colors_take_the_answered_bytes_and_others_stay_unknown() {
        let colors = reply();
        assert_eq!(rgb_in(Color::Blue, Some(&colors)), Some((0x40, 0x80, 0xff)));
        assert_eq!(
            rgb_in(Color::Indexed(4), Some(&colors)),
            Some((0x40, 0x80, 0xff))
        );
        assert_eq!(rgb_in(Color::Blue, None), None, "没问到的");
        assert_eq!(rgb_in(Color::Indexed(73), Some(&colors)), None);
        assert_eq!(rgb_in(Color::Rgb(1, 2, 3), None), Some((1, 2, 3)));
    }

    #[test]
    fn dark_and_light_follow_colorsaurus() {
        assert!(Ground::of((0xc0, 0xca, 0xf5), (0x1a, 0x1b, 0x26)).dark);
        assert!(!Ground::of((0x37, 0x60, 0xbf), (0xe1, 0xe2, 0xe7)).dark);
        // 字和底一样：底亮过一半的是浅色，暗的是深色（照那个库）。
        assert!(!Ground::of((200, 200, 200), (200, 200, 200)).dark);
        assert!(Ground::of((20, 20, 20), (20, 20, 20)).dark);
    }

    #[test]
    fn a_round_of_answers_is_whole_only_when_all_sixteen_came() {
        let mut asked = Asked::default();
        for (n, &rgb) in reply().iter().enumerate() {
            asked.palette(n, rgb);
        }
        asked.palette(99, (1, 1, 1));
        asked.fg((0xc0, 0xca, 0xf5));
        let (sixteen, ground) = asked.bg((0x1a, 0x1b, 0x26));
        assert_eq!(sixteen, Some(reply()));
        assert_eq!(
            ground,
            Some(Ground::of((0xc0, 0xca, 0xf5), (0x1a, 0x1b, 0x26)))
        );
        // 攒着的清掉了：下一轮只答了底色和一号，什么都凑不齐。
        asked.palette(1, (1, 2, 3));
        assert_eq!(asked.bg((0, 0, 0)), (None, None));
    }
}
