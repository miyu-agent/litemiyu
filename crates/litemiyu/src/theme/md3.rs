//! 照主色生成一套（Miyu 终端 `theme/persona.rs` 的 `overlay`：跟随头像、跟随壁纸、终端配色同一套算法，2026-10-14 Miyu
//! 项目主人定）。LiteMiyu 只有终端配色用它，主色从终端的 16 色里挑（`derived.rs` 的 `seeds`）。

use material_colors::blend::harmonize;
use material_colors::color::Rgb;
use material_colors::hct::Hct;
use material_colors::palette::TonalPalette;
use material_colors::scheme::variant::SchemeContent;
use ratatui::style::Color;

use super::palette::{Palette, Tone};

/// 照主色生成一套盖在 `base`（tokyonight 深色或浅色那一份）上：MD3 `SchemeContent`，对比度 0。不带色相的、强调色每一格
/// 照 `base` 那一格的亮度（tone），从主色生成的几条色板里取同一亮度的颜色，明暗层次和 tokyonight 一样，只换色相：正文、
/// 点开的一步、公式、选中那一条上的字从中性色板，暗字、引用、斜体、代码注释和运算符、悬停从中性变体，强调色、链接、
/// 粗体、表头、侧边栏的标题从主色，小块的底、思考从次色，选中、行内代码、图片从第三色。有含义的颜色（出错、增删、警告、
/// 成功、高亮、代码着色其余几格）色相往主色最多转 15°（`harmonize`），认得出是哪一种。
pub fn overlay(base: &Palette, seed: u32, dark: bool) -> Palette {
    let source = Rgb::from_u32(seed);
    let s = SchemeContent::new(Hct::new(source), dark, Some(0.0)).scheme;
    let neutral = &s.neutral_palette;
    let variant = &s.neutral_variant_palette;
    let primary = &s.primary_palette;
    let second = &s.secondary_palette;
    let third = &s.tertiary_palette;
    let like = |from: &TonalPalette, tone: Tone| match tone.0 {
        Color::Rgb(r, g, b) => {
            let lightness = Hct::new(Rgb::new(r, g, b)).get_tone();
            let c = Rgb::from(from.get_hct(lightness));
            Tone::of(Color::Rgb(c.red, c.green, c.blue))
        }
        _ => tone,
    };
    let near = |tone: Tone| match tone.0 {
        Color::Rgb(r, g, b) => {
            let c = harmonize(Rgb::new(r, g, b), source);
            Tone::of(Color::Rgb(c.red, c.green, c.blue))
        }
        _ => tone,
    };
    let b = base;
    Palette {
        dim: like(variant, b.dim),
        accent: like(primary, b.accent),
        // 开放权限和强调色同色（Miyu `0380b05d`，2026-11-10 项目主人：「把开放权限的颜色从红色改成之前workspace的颜色」）。
        full: like(primary, b.full),
        picked: like(third, b.picked),
        // 思考取次色（2026-10-23 Miyu 项目主人看了样板选的：带一点主题的色相、不艳）。
        thought: like(second, b.thought),
        thought_hover: like(second, b.thought_hover),
        shade: like(neutral, b.shade),
        chip_bg: like(second, b.chip_bg),
        chip_hover_bg: like(second, b.chip_hover_bg),
        panel_bg: like(neutral, b.panel_bg),
        added: near(b.added),
        removed: near(b.removed),
        diff_added_fg: near(b.diff_added_fg),
        diff_added_bg: near(b.diff_added_bg),
        diff_removed_fg: near(b.diff_removed_fg),
        diff_removed_bg: near(b.diff_removed_bg),
        error: near(b.error),
        warn: near(b.warn),
        good: near(b.good),
        // 加粗取 primary、文中代码取 tertiary（2026-10-17 Miyu 项目主人：原来都是中性色，和正文一个颜色）。
        md_bold: like(primary, b.md_bold),
        md_italic: like(variant, b.md_italic),
        md_code: like(third, b.md_code),
        md_link: like(primary, b.md_link),
        md_url: like(primary, b.md_url),
        md_image: like(third, b.md_image),
        md_quote: like(variant, b.md_quote),
        md_mark: near(b.md_mark),
        md_table_head: like(primary, b.md_table_head),
        md_math: like(neutral, b.md_math),
        side_title: like(primary, b.side_title),
        hover: like(variant, b.hover),
        faint: like(variant, b.faint),
        on_accent: like(neutral, b.on_accent),
        code_keyword: near(b.code_keyword),
        code_function: near(b.code_function),
        code_string: near(b.code_string),
        code_number: near(b.code_number),
        code_comment: like(variant, b.code_comment),
        code_type: near(b.code_type),
        code_constant: near(b.code_constant),
        code_property: near(b.code_property),
        code_operator: like(variant, b.code_operator),
        code_macro: near(b.code_macro),
        code_parameter: near(b.code_parameter),
        text: like(neutral, b.text),
        row_bg: like(primary, b.row_bg),
        row_dim_bg: like(neutral, b.row_dim_bg),
        md_heading: near(b.md_heading),
        background: like(neutral, b.background),
    }
}

#[cfg(test)]
mod tests {
    use material_colors::color::Rgb;
    use material_colors::hct::Hct;
    use ratatui::style::Color;

    use super::overlay;
    use crate::theme::builtin::{DAY, NIGHT};
    use crate::theme::palette::Tone;

    /// 一格的 HCT：色相、色度、亮度。
    fn hct(tone: Tone) -> (f64, f64, f64) {
        let Color::Rgb(r, g, b) = tone.0 else {
            panic!("{tone:?}")
        };
        let h = Hct::new(Rgb::new(r, g, b));
        (h.get_hue(), h.get_chroma(), h.get_tone())
    }

    #[test]
    fn every_neutral_and_accent_follows_at_tokyonights_lightness_and_meanings_only_lean() {
        // 搬自 Miyu `persona.rs` 的测试（吉祥物、底那几格 LiteMiyu 没有）：照 tokyonight 每一格的亮度取色，有含义的颜色
        // 色相往主色最多转 15°。
        for (base, dark) in [(NIGHT, true), (DAY, false)] {
            let green = overlay(&base, 0x2fab34, dark);
            for (got, was, what) in [
                (green.text, base.text, "正文"),
                (green.thought, base.thought, "思考"),
                (green.accent, base.accent, "强调色"),
            ] {
                assert_ne!(got, was, "{dark} {what}跟着主色");
                assert!(
                    (hct(got).2 - hct(was).2).abs() < 1.0,
                    "{dark} {what}和 tokyonight 一样亮"
                );
            }
            let (thought, dim) = (hct(green.thought), hct(green.dim));
            assert!(
                thought.1 > dim.1,
                "{dark} 思考取次色：比暗字（中性变体）带色"
            );
            assert!(thought.1 < hct(green.accent).1, "{dark} 思考不比强调色艳");
            let (error, before) = (hct(green.error), hct(base.error));
            let turned = (error.0 - before.0)
                .abs()
                .min(360.0 - (error.0 - before.0).abs());
            assert!(turned <= 15.5, "{dark} 出错的红只偏一点：{turned}");
            assert!((error.2 - before.2).abs() < 1.0, "{dark} 出错的红亮度不变");
        }
    }

    #[test]
    fn bold_takes_the_primary_and_inline_code_the_tertiary() {
        // 2026-10-17 Miyu 项目主人：加粗用 primary，文中代码用 tertiary。
        let near = |a: f64, b: f64| (a - b).abs().min(360.0 - (a - b).abs()) < 10.0;
        for (base, dark) in [(NIGHT, true), (DAY, false)] {
            let green = overlay(&base, 0x2fab34, dark);
            let (bold, code, text) = (hct(green.md_bold), hct(green.md_code), hct(green.text));
            assert!(
                bold.1 > text.1 + 10.0,
                "{dark} 加粗有颜色：{bold:?}，正文 {text:?}"
            );
            assert!(
                code.1 > text.1 + 10.0,
                "{dark} 文中代码有颜色：{code:?}，正文 {text:?}"
            );
            assert!(
                near(bold.0, hct(green.accent).0),
                "{dark} 加粗是 primary 的色相"
            );
            assert!(
                near(code.0, hct(green.picked).0),
                "{dark} 文中代码是 tertiary 的色相"
            );
            assert!(!near(bold.0, code.0), "{dark} 两个分得开");
        }
    }
}
