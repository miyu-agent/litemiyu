//! 照问到的挑哪一套（`mod.rs` 的 `chosen`，Miyu `choice.rs` 的 `pick_in` 终端配色那一支）。

use ratatui::style::Color;

use super::builtin::{DAY, NIGHT, TERMINAL, TERMINAL_LIGHT};
use super::seen::{Colors, Ground, Seen};
use super::{chosen, derived, md3};

/// Tokyo Night 的终端配色（kitty 出厂那一份）：16 色、字色、底色。
const TOKYO: Colors = [
    (0x15, 0x16, 0x1e),
    (0xf7, 0x76, 0x8e),
    (0x9e, 0xce, 0x6a),
    (0xe0, 0xaf, 0x68),
    (0x7a, 0xa2, 0xf7),
    (0xbb, 0x9a, 0xf7),
    (0x7d, 0xcf, 0xff),
    (0xa9, 0xb1, 0xd6),
    (0x41, 0x48, 0x68),
    (0xf7, 0x76, 0x8e),
    (0x9e, 0xce, 0x6a),
    (0xe0, 0xaf, 0x68),
    (0x7a, 0xa2, 0xf7),
    (0xbb, 0x9a, 0xf7),
    (0x7d, 0xcf, 0xff),
    (0xc0, 0xca, 0xf5),
];

fn ground() -> Ground {
    Ground::of((0xc0, 0xca, 0xf5), (0x1a, 0x1b, 0x26))
}

#[test]
fn a_truecolor_terminal_that_answered_everything_gets_the_md3_set() {
    let seen = Seen {
        sixteen: Some(TOKYO),
        ground: Some(ground()),
        truecolor: true,
    };
    let seed = derived::seeds(seen)[0];
    let want = derived::seeded(md3::overlay(&NIGHT, seed, true), seen);
    assert_eq!(chosen(seen), want);
    // 色值照 Miyu 终端自己的代码（`persona.rs`、`derived.rs`、`miyu-seeds`）同样的输入算出来的：主色挑的是 1 号的红。
    assert_eq!(seed, 0xf7_768e);
    let got = chosen(seen);
    assert_eq!(got.dim.0, Color::Rgb(160, 135, 138));
    assert_eq!(got.accent.0, Color::Rgb(252, 122, 146));
    assert_eq!(got.full.0, Color::Rgb(252, 122, 146));
    assert_eq!(got.shade.0, Color::Rgb(41, 41, 51));
}

#[test]
fn without_truecolor_or_sixteen_the_terminal_set_only_takes_the_ground() {
    for seen in [
        Seen {
            sixteen: Some(TOKYO),
            ground: Some(ground()),
            truecolor: false,
        },
        Seen {
            sixteen: None,
            ground: Some(ground()),
            truecolor: true,
        },
    ] {
        let got = chosen(seen);
        assert_eq!(got.accent, TERMINAL.accent, "字是 16 色的名字");
        assert_eq!(got, derived::terminal(&TERMINAL, seen));
        assert!(matches!(got.shade.0, Color::Rgb(..)), "底色照终端的底推");
    }
    assert_eq!(
        chosen(Seen::NONE),
        TERMINAL,
        "什么都没问到：出厂的 16 色那一套"
    );
    let paper = Seen {
        sixteen: None,
        ground: Some(Ground::of((0x30, 0x30, 0x30), (0xf8, 0xf8, 0xf8))),
        truecolor: true,
    };
    assert_eq!(chosen(paper).accent, TERMINAL_LIGHT.accent);
    assert_eq!(
        chosen(paper).hover,
        TERMINAL_LIGHT.hover,
        "浅色的终端照浅色那一套"
    );
}

#[test]
fn the_open_level_is_the_accent_not_red() {
    // Miyu `0380b05d`（2026-11-10 项目主人：「把开放权限的颜色从红色改成之前workspace的颜色」）：提示符、你说的话的竖线和强调色
    // 同色。出厂四套逐套比；照终端生成的那一套也一样（`like(primary, …)` 输入相同）。
    for palette in [TERMINAL, TERMINAL_LIGHT, NIGHT, DAY] {
        assert_eq!(palette.full, palette.accent);
    }
    let made = md3::overlay(&NIGHT, 0x7a_a2f7, true);
    assert_eq!(made.full, made.accent);
}

/// 第二十六趟（验收时报的）：深色的终端里选了浅色、铺着底色（开头「明暗」挪到「浅色」预览、配置页外观页一样）：整屏
/// 铺成浅的底，悬浮窗（选择窗）的底、点开的一步、小块的底也照这一层浅的推，不照被盖住的终端的深底推；字照这一套浅色的。明暗
/// 和终端一样的照旧照终端的底推（上面那一条的数不变），不铺底色的也照终端的底推（看得见的就是终端的底）。
#[test]
fn a_light_choice_over_a_dark_terminal_lays_its_floats_on_the_light_ground() {
    use super::ground::{Choice, Mode};
    let seen = Seen {
        sixteen: Some(TOKYO),
        ground: Some(ground()),
        truecolor: true,
    };
    let light = |ground| Choice {
        mode: Mode::Light,
        ground,
    };
    let lum = |color: Color| match color {
        Color::Rgb(r, g, b) => (u32::from(r) + u32::from(g) + u32::from(b)) / 3,
        other => panic!("真颜色：{other:?}"),
    };
    let got = super::chosen_in(seen, light(true));
    let page = lum(got.background.0);
    assert!(page > 160, "整屏铺浅的：{page}");
    for (what, tone) in [
        ("选择窗的底", got.panel_bg),
        ("点开的一步", got.shade),
        ("小块", got.chip_bg),
    ] {
        let face = lum(tone.0);
        assert!(face > 140, "{what}照浅的底推：{face}");
        assert!(
            page.abs_diff(face) < 40,
            "{what}和整屏差一档：{page} {face}"
        );
    }
    assert!(lum(got.text.0) < 120, "字是浅色那一套的深字");
    // 不铺底色：看得见的是终端的深底，悬浮窗照它推（原来的样子）。
    let bare = super::chosen_in(seen, light(false));
    assert!(lum(bare.panel_bg.0) < 80, "不铺照终端的底");
    // 明暗和终端一样：照旧照终端的底推。
    let seed = derived::seeds(seen)[0];
    let dark = Choice {
        mode: Mode::Dark,
        ground: true,
    };
    assert_eq!(
        super::chosen_in(seen, dark),
        derived::seeded(md3::overlay(&NIGHT, seed, true), seen)
    );
}
