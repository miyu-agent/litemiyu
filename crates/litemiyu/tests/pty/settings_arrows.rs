//! 没有颜色时配置界面选中的那一行行首写箭头（配置 七 C19，照 Miyu 终端 `tests/console.rs` 的 `walk`，8a2c7d25）：`NO_COLOR` 下
//! 主菜单、一页里的行、几栏的页、选择窗，选中的都有 `❯`，挪一下箭头跟着走；不再画竖条。

use crate::support::{Options, Session};

const ESC: &[u8] = b"\x1b";

const CONFIG: &str = "[providers.fake]\ndriver = \"openai-chat\"\nbase_url = \"http://127.0.0.1:9/v1\"\n\n[providers.fake.models.\"m1\"]\nwindow = 64000\n\n[models]\nchat = \"fake/m1\"\n";

/// 写着箭头的那一行，箭头后面的字（去掉两头空白）是 `choices` 里的哪一个（窗盖在页上时，箭头前面是露在窗左边的页）。
fn picked(session: &Session, choices: &[&str]) -> Option<String> {
    session.screen().lines().find_map(|l| {
        let (_, rest) = l.rsplit_once('❯')?;
        let rest = rest.trim();
        choices
            .iter()
            .find(|c| rest.starts_with(**c))
            .map(|c| (*c).to_string())
    })
}

/// 等到箭头停在 `want` 上（`choices` 里的一个）。
fn wait_picked(session: &Session, choices: &[&str], want: &str, what: &str) {
    session.wait_until(what, |s| picked(s, choices).as_deref() == Some(want));
}

#[test]
fn without_color_the_settings_point_at_the_chosen_row() {
    let options = Options {
        env: vec![("NO_COLOR", "1")],
        ..Options::default()
    };
    let session = Session::launch_with(Some(CONFIG), &[], &["config"], &options);
    session.wait_for("Enter 进入", "主菜单");
    let menu = ["通用", "外观"];
    wait_picked(&session, &menu, "通用", "主菜单选中的有箭头");
    session.type_text("j");
    wait_picked(&session, &menu, "外观", "主菜单挪了一项，箭头跟着走");
    assert!(
        !session.screen().contains('┃'),
        "没有颜色不画竖条：\n{}",
        session.screen()
    );
    session.type_text("k");
    session.press_enter();
    session.wait_for("◉ 通用", "通用页");
    let rows = ["界面语言", "新会话的功能"];
    wait_picked(&session, &rows, "界面语言", "页里的行");
    session.press_enter();
    session.wait_for("Enter 确定", "界面语言的选择窗");
    let choices = ["跟随系统", "中文", "English"];
    wait_picked(
        &session,
        &choices,
        "跟随系统",
        "选择窗选中的那一行行首有箭头",
    );
    session.type_text("j");
    wait_picked(&session, &choices, "中文", "挪了一行，箭头跟着走");
    session.press(ESC);
    session.wait_until("选择窗关上", |s| !s.screen().contains("Enter 确定"));
    session.press(ESC);
    session.wait_for("Enter 进入", "回主菜单");
    // 几栏的页：供应商那一栏选中的那一家。
    session.type_text("jj");
    session.press_enter();
    session.wait_for("◉ 供应商和模型", "供应商和模型");
    wait_picked(&session, &["fake"], "fake", "几栏的页选中的那一家有箭头");
    assert!(
        !session.screen().contains('┃'),
        "几栏的页也不画竖条：\n{}",
        session.screen()
    );
}
