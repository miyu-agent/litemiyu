//! 终端配色照终端的真颜色生成（Miyu 终端 `theme/derived.rs`，蓝图 `tui.md`「终端配色」）。
//!
//! - 底色（2026-10-14 Miyu 项目主人定）：点开的一步、小块这些底色照终端的底色往字色掺一点，差异的底往终端的绿、红掺
//!   一点。终端没答底色的照出厂那一套写的；没答 16 色的，差异的底照出厂那一套。
//! - 字（2026-10-19 Miyu 项目主人定）：真彩色、16 色和字色底色都问到了，拿 16 色里最鲜艳的那个当主色生成 MD3（[`seeds`]、
//!   `md3.rs`、[`seeded`]），底照旧从终端的底推、不铺。不是真彩色、问不到的照旧写 16 色的名字，字直接用终端的 16 色。
//!
//! Miyu 这里还推配置页的面板、选中行和 mermaid 图的几格底色：LiteMiyu 没有那几格（`palette.rs`），不推。

use material_colors::color::Rgb as Argb;
use material_colors::quantize::{Quantizer, QuantizerCelebi};
use material_colors::score::Score;
use ratatui::style::Color;

use super::palette::{Palette, Tone};
use super::seen::{Ground, Rgb, Seen};

/// 底色往字色掺多少：点开的一步（Miyu 和面板同一档）、标签、悬停的标签。
const PANEL: f64 = 0.08;
const CHIP: f64 = 0.12;
const CHIP_HOVER: f64 = 0.16;
/// 选中的一行（配置界面）：底色往字色掺多少（Miyu `derived.rs` 的 `ROW`）。
const ROW: f64 = 0.18;
/// 没焦点那一栏选中的一行（配置界面）：底色往字色掺多少（Miyu `derived.rs` 的 `ROW_DIM`）。
const ROW_DIM: f64 = 0.04;
/// 差异的底往绿、红掺多少。
const DIFF: f64 = 0.15;

/// 16 色里带颜色的几号：1 到 6、9 到 14（黑、白、灰不算）。
const HUES: [usize; 12] = [1, 2, 3, 4, 5, 6, 9, 10, 11, 12, 13, 14];

/// 照问到的终端颜色推底色：`base` 是出厂那一份（终端配色深色或浅色）。没问到字色、底色的照出厂的。
pub fn terminal(base: &Palette, seen: Seen) -> Palette {
    let mut out = *base;
    if let Some(ground) = seen.ground {
        let diff = seen.sixteen.map(|s| (s[2], s[1]));
        grounds(&mut out, ground.bg, ground.fg, diff);
    }
    out
}

/// 终端的主色候选（Miyu「主色」第 4a 条）：真彩色、16 色和字色底色都问到了，带颜色的那 12 号照跟随壁纸同一套打分挑
/// （Miyu `miyu-seeds` 的 `from_rgba`：Celebi 量化成最多 128 种、MD3 的默认打分挑最多 4 个），好的在前；别的是空的，照
/// [`terminal`] 回退。Miyu 每一帧都看一次、照 16 色记着上次挑的；这里只在换色时算一次，不用记。
pub fn seeds(seen: Seen) -> Vec<u32> {
    let Some(sixteen) = seen
        .sixteen
        .filter(|_| seen.truecolor && seen.ground.is_some())
    else {
        return Vec::new();
    };
    let pixels: Vec<Argb> = HUES
        .iter()
        .map(|&n| {
            let (r, g, b) = sixteen[n];
            Argb::new(r, g, b)
        })
        .collect();
    // 只认终端自己的颜色：一个都不合格时打分给的 Google 蓝不是终端的，没有候选，照 16 色那一套。
    let own: Vec<u32> = HUES.iter().map(|&n| hex(sixteen[n])).collect();
    let quantized = QuantizerCelebi::quantize(&pixels, QUANTIZE);
    Score::score(&quantized.color_to_count, Some(MAX), None, None)
        .into_iter()
        .map(|c| hex((c.red, c.green, c.blue)))
        .filter(|s| own.contains(s))
        .collect()
}

/// 最多挑几个候选（Miyu `miyu-seeds` 的 `MAX`）。
const MAX: usize = 4;
/// 量化成最多几种颜色（Miyu `miyu-seeds` 的 `QUANTIZE`）。
const QUANTIZE: usize = 128;

/// 照主色生成好的一套（`md3`，盖在 tokyonight 上的）换上终端的底：点开的一步这些从终端的底往生成的正文色掺，差异的底
/// 往生成的增、删字色掺。没问到底色的原样。
pub fn seeded(md3: Palette, seen: Seen) -> Palette {
    let Some(Ground { bg, .. }) = seen.ground else {
        return md3;
    };
    let mut out = md3;
    let text = rgb_of(out.text).unwrap_or(bg);
    let diff = rgb_of(out.diff_added_fg).zip(rgb_of(out.diff_removed_fg));
    grounds(&mut out, bg, text, diff);
    out
}

/// 同 [`seeded`]，底照这一套自己的底（`background`）推：铺了底色、挑的明暗又和终端的底不一样时，终端的底整个被盖住了，
/// 悬浮窗、点开的一步这些照终端的底推出来是另一种明暗（深色终端选浅色：整屏铺成浅的，选择窗还是深底暗字，第二十六趟主会话
/// 亲手验收报的）。这一套的底不是真颜色的（没生成过）原样。
pub fn painted(md3: Palette) -> Palette {
    let Some(bg) = rgb_of(md3.background) else {
        return md3;
    };
    let mut out = md3;
    let text = rgb_of(out.text).unwrap_or(bg);
    let diff = rgb_of(out.diff_added_fg).zip(rgb_of(out.diff_removed_fg));
    grounds(&mut out, bg, text, diff);
    out
}

fn hex((r, g, b): Rgb) -> u32 {
    u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b)
}

fn rgb_of(tone: Tone) -> Option<Rgb> {
    match tone.0 {
        Color::Rgb(r, g, b) => Some((r, g, b)),
        _ => None,
    }
}

/// 底色照 `bg` 往 `fg` 掺，差异的底往 `diff` 的绿、红掺（有才掺）。
fn grounds(out: &mut Palette, bg: Rgb, fg: Rgb, diff: Option<(Rgb, Rgb)>) {
    let toward = |to: Rgb, t: f64| Tone::of(rgb(mix(bg, to, t)));
    out.panel_bg = toward(fg, PANEL);
    out.shade = toward(fg, PANEL);
    out.chip_bg = toward(fg, CHIP);
    out.chip_hover_bg = toward(fg, CHIP_HOVER);
    out.row_bg = toward(fg, ROW);
    out.row_dim_bg = toward(fg, ROW_DIM);
    if let Some((green, red)) = diff {
        out.diff_added_bg = toward(green, DIFF);
        out.diff_removed_bg = toward(red, DIFF);
    }
}

/// 从 `from` 往 `to` 走 `t`（0 到 1）。
fn mix(from: Rgb, to: Rgb, t: f64) -> Rgb {
    let step = |a: u8, b: u8| {
        let v = f64::from(a) + (f64::from(b) - f64::from(a)) * t;
        v.round().clamp(0.0, 255.0) as u8
    };
    (step(from.0, to.0), step(from.1, to.1), step(from.2, to.2))
}

fn rgb((r, g, b): Rgb) -> Color {
    Color::Rgb(r, g, b)
}

#[cfg(test)]
mod tests;
