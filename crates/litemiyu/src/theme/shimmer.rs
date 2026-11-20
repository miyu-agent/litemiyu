//! 运行状态行、压缩那一行的流光和呼吸（Miyu 蓝图 `tui.md`「运行状态行和排队的消息」第 1 条、「正文」第 9 条）：一道亮光
//! 从左往右扫过，没扫到的压暗；进度条最前面那一格明暗来回；压缩久了整行往白里呼吸。搬自 Miyu 终端的 `theme/shimmer.rs`、
//! `theme/motion.rs`。
//!
//! 底色是真彩色的照字节算明暗；是终端调色板的（终端配色），问到了终端 16 色的真颜色就照它算（`seen.rs` 的 `rgb`），问不到的
//! 扫到的加粗、没扫到的变暗。强调色的流光（压缩那一行）和进度条照 Miyu：强调色问不到真字节的退回 tokyonight 的字色
//! （`mod.rs` 的 `accent_rgb`）。

use ratatui::style::{Color, Modifier, Style};

use super::seen::Rgb;
use crate::config::Shimmer;

/// 第 `index` 个字（一共 `len` 个）在 `t` 秒时离亮光多近：0 是远，1 是正中。
fn nearness(index: usize, len: usize, t: f64, look: &Shimmer) -> f64 {
    // 亮光走一趟要多走两个带宽，头尾都扫得干净。
    let span = len as f64 + 2.0 * look.band;
    let head = (t / look.sweep_seconds.max(0.1)).fract() * span - look.band;
    1.0 - ((index as f64 - head).abs() / look.band.max(0.1)).min(1.0)
}

/// 底色是 `base` 的流光：`bytes` 是照它算渐变的真字节，不知道的是 `None`。没扫到的乘 `dim` 压暗，扫到的往白里偏，
/// 正中偏 `lift`。
pub(super) fn sweep(
    base: Color,
    bytes: Option<Rgb>,
    index: usize,
    len: usize,
    t: f64,
    look: &Shimmer,
) -> Style {
    let near = nearness(index, len, t, look);
    let Some((r, g, b)) = bytes else {
        let style = Style::new().fg(base).add_modifier(Modifier::BOLD);
        return if near > 0.5 {
            style
        } else {
            style.add_modifier(Modifier::DIM)
        };
    };
    let scale = look.dim + (1.0 - look.dim) * near;
    let lift = look.lift * near;
    let mix = |c: u8| {
        let v = f64::from(c) * scale;
        (v + (255.0 - v) * lift).round().clamp(0.0, 255.0) as u8
    };
    Style::new()
        .fg(Color::Rgb(mix(r), mix(g), mix(b)))
        .add_modifier(Modifier::BOLD)
}

/// 流光的点和用时：底色压暗（和没扫到的字一样），不跟着流光变。
pub(super) fn rest(base: Color, bytes: Option<Rgb>, look: &Shimmer) -> Style {
    let Some((r, g, b)) = bytes else {
        return Style::new().fg(base).add_modifier(Modifier::DIM);
    };
    let dim = |c: u8| (f64::from(c) * look.dim).round().clamp(0.0, 255.0) as u8;
    Style::new().fg(Color::Rgb(dim(r), dim(g), dim(b)))
}

/// 进度条最前面亮着的那一格：`k` 从 0 到 1，从 `(r, g, b)` 的 `low` 成暗到往白里偏 `lift`。
pub(super) fn frontier((r, g, b): Rgb, k: f64, low: f64, lift: f64) -> Style {
    let k = k.clamp(0.0, 1.0);
    let at = |c: u8| {
        let dark = f64::from(c) * low;
        let light = f64::from(c) + (255.0 - f64::from(c)) * lift;
        (dark + (light - dark) * k).round().clamp(0.0, 255.0) as u8
    };
    Style::new().fg(Color::Rgb(at(r), at(g), at(b)))
}

/// 往白里偏 `amount`（0 到 1）：呼吸用。不是真彩色的前景照旧（色深统一换色在画完以后）。
pub fn lifted(style: Style, amount: f64) -> Style {
    match style.fg {
        Some(Color::Rgb(r, g, b)) if amount > 0.0 => {
            let up = |c: u8| {
                let c = f64::from(c);
                (c + (255.0 - c) * amount.min(1.0)).round() as u8
            };
            style.fg(Color::Rgb(up(r), up(g), up(b)))
        }
        _ => style,
    }
}

#[cfg(test)]
mod tests {
    use ratatui::style::{Color, Modifier, Style};

    use super::{frontier, lifted, rest, sweep};
    use crate::config::Config;

    fn config() -> Config {
        Config::builtin().expect("出厂的配置读得懂")
    }

    #[test]
    fn a_palette_color_sweeps_with_bold_and_dim_instead_of_bytes() {
        // 2026-10-14 Miyu 项目主人报：终端配色时运行状态行的字是写死的蓝。终端调色板的没法按字节算明暗，扫到的加粗、
        // 没扫到的变暗，颜色还是终端的那一个。
        let look = config().layout.shimmer;
        let lit = sweep(Color::Blue, None, 0, 4, look.sweep_seconds * 0.4, &look);
        let far = sweep(Color::Blue, None, 3, 4, 0.0, &look);
        assert_eq!((lit.fg, far.fg), (Some(Color::Blue), Some(Color::Blue)));
        assert!(far.add_modifier.contains(Modifier::DIM), "没扫到的变暗");
        let rest = rest(Color::Blue, None, &look);
        assert_eq!(rest.fg, Some(Color::Blue));
        assert!(rest.add_modifier.contains(Modifier::DIM));
    }

    #[test]
    fn known_bytes_sweep_as_a_real_gradient() {
        // 2026-10-14 Miyu 项目主人：只加粗变暗「怪怪的，是字变颜色，不是真的流光了」。知道真字节的照它算渐变：挨着的字
        // 字节不一样，都是那个颜色压暗、提亮出来的。
        let look = config().layout.shimmer;
        let blue = Some((0x40, 0x80, 0xff));
        let t = look.sweep_seconds * 0.4;
        let colors: Vec<_> = (0..6)
            .map(|i| sweep(Color::Blue, blue, i, 6, t, &look).fg)
            .collect();
        assert!(
            colors.windows(2).any(|w| w[0] != w[1]),
            "挨着的字颜色不一样：{colors:?}"
        );
        for color in colors {
            let Some(Color::Rgb(r, g, b)) = color else {
                panic!("照真字节算：{color:?}");
            };
            assert!(b >= g && g >= r, "还是那个蓝：{r} {g} {b}");
        }
    }

    #[test]
    fn the_frontier_swings_between_dark_and_light_and_lifting_brightens() {
        let fg = |s: Style| match s.fg {
            Some(Color::Rgb(r, g, b)) => u32::from(r) + u32::from(g) + u32::from(b),
            _ => 0,
        };
        let bytes = (0x7a, 0xa2, 0xf7);
        assert!(fg(frontier(bytes, 0.0, 0.45, 0.35)) < fg(frontier(bytes, 1.0, 0.45, 0.35)));
        let base = Style::new().fg(Color::Rgb(100, 100, 100));
        assert!(fg(lifted(base, 0.2)) > fg(base));
        assert_eq!(lifted(base, 0.0), base, "不呼吸不变");
        let named = Style::new().fg(Color::Blue);
        assert_eq!(lifted(named, 0.5), named, "不是真彩色的照旧");
    }
}
