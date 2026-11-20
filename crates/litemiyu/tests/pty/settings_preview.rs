//! 「明暗」挑选窗挪光标当场换样子（配置 五，`settings/preview.rs`）：外观页开「明暗」的窗，往下挪一档，没按 `Enter` 整个界面的
//! 底色已经换了、配置文件一个字节没动；`Esc` 换回原来的底色、配置照旧；再开、挪、`Enter`，配置里写上新的那一档。
//! 真彩色、手动选的深色铺底色：底色是 tokyonight 的深色那一份，换成浅色就是浅色那一份（`theme/mod.rs` 的 `chosen_in`）。

use crate::support::{Options, Session};

const DOWN: &[u8] = b"\x1b[B";
const ESC: &[u8] = b"\x1b";

const CONFIG: &str = "[providers.fake]\ndriver = \"openai-chat\"\nbase_url = \"http://127.0.0.1:9/v1\"\n\n[models]\nchat = \"fake/m1\"\n\n[tui]\ntheme_mode = \"dark\"\n";

/// 右上角一格（配置页的字都在左边，这一格只有铺的底色）的底色。
fn ground(session: &Session) -> vt100::Color {
    session.cell(1, 150).map(|(_, bg)| bg).expect("在屏幕里")
}

fn config_file(session: &Session) -> std::path::PathBuf {
    lm_engine::store::paths::Paths::under(session.data()).config_file()
}

/// 外观页的第一行就是「明暗」：开它的窗。
fn open_mode(session: &Session) {
    session.press_enter();
    session.wait_for("Enter 确定", "明暗的选择窗");
}

#[test]
fn moving_in_the_mode_picker_previews_at_once_esc_goes_back_and_enter_saves() {
    let options = Options {
        env: vec![("COLORTERM", "truecolor")],
        ..Options::default()
    };
    let session = Session::launch_with(Some(CONFIG), &[], &["config"], &options);
    session.wait_for("Enter 进入", "主菜单");
    session.press(DOWN);
    session.press_enter();
    session.wait_for("◉ 外观", "外观页");
    let dark = ground(&session);
    assert!(
        matches!(dark, vt100::Color::Rgb(..)),
        "深色铺底色是真彩色：{dark:?}"
    );
    let file = config_file(&session);
    let before = std::fs::read(&file).expect("有配置");

    open_mode(&session);
    session.press(DOWN);
    session.wait_until("挪到「浅色」，底色当场换了", |s| {
        ground(s) != dark
    });
    let light = ground(&session);
    assert!(
        session.screen().contains("Enter 确定"),
        "窗还开着，没按 Enter"
    );
    assert_eq!(std::fs::read(&file).expect("有配置"), before, "预览不写盘");

    session.press(ESC);
    session.wait_until("Esc 以后换回深色", |s| ground(s) == dark);
    assert!(!session.screen().contains("Enter 确定"), "窗关了");
    assert_eq!(std::fs::read(&file).expect("有配置"), before, "Esc 不存");

    open_mode(&session);
    session.press(DOWN);
    session.wait_until("又挪到浅色", |s| ground(s) == light);
    session.press_enter();
    session.wait_until("Enter 存了浅色", |_| {
        std::fs::read_to_string(&file).is_ok_and(|text| text.contains("theme_mode = \"light\""))
    });
    assert_eq!(ground(&session), light, "存了以后照存的那一档");
    let after = std::fs::read_to_string(&file).expect("有配置");
    assert!(
        after.contains("[providers.fake]") && !after.contains("\"dark\""),
        "只换了这一项：\n{after}"
    );
}
