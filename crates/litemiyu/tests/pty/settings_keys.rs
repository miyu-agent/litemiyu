//! 按键模式在配置界面里改了当场交给对话界面（不重启）：对话里 `/help` 框的按键提示照按键模式写「j/k」或「↑/↓」
//! （`ui/hint_keys.rs`）。设置页这一项出厂是 Vim（2026-11-13 项目主人定），选择窗打开时停在它上面：往上一格是方向键，再往下
//! 一格回 Vim。启动时没写这一项的对话照 `layout.json`（归界面线），这里不查。数据在临时目录，不连外网。

use crate::support::{Session, served};

const DOWN: &[u8] = b"\x1b[B";
const UP: &[u8] = b"\x1b[A";
const ESC: &[u8] = b"\x1b";

/// 开 `/help`，等它的按键提示写成 `hint`，关上。
fn help_says(session: &Session, hint: &str) {
    session.type_text("/help");
    session.press_enter();
    session.wait_for(hint, hint);
    session.press(ESC);
    session.wait_until("帮助框关上", |s| !s.screen().contains(hint));
}

/// `/config` → 通用 → 「交互」组的「按键模式」（通用页的最后一项是「预压缩」，从那里往上三格）→ 选择窗里往 `step` 挪一格选上 → 回对话。选择窗底下的按键提示照现在的按键模式写
/// （`keys`：Vim 写「j/k」，方向键写「↑/↓」，第二十六趟）。
fn switch(session: &Session, step: &[u8], keys: &str) {
    session.type_text("/config");
    session.press_enter();
    session.wait_for("j/k 移动   Enter 进入", "主菜单");
    session.press_enter();
    session.wait_for("◉ 通用", "通用页");
    for _ in 0..40 {
        session.press(DOWN);
    }
    // 权限、压缩两组（原来的「隐私与安全」「高级」两页）并进了通用页，排在「交互」后面。
    session.wait_for("┃ 预压缩", "光标到最后一项预压缩");
    for _ in 0..3 {
        session.press(UP);
    }
    session.wait_for("┃ 按键模式", "光标回到按键模式");
    session.press_enter();
    session.wait_for(&format!("{keys} 选择   Enter 确定"), "选择窗");
    session.press(step);
    session.press_enter();
    session.wait_for("已保存", "存上");
    session.press(ESC);
    session.wait_for("j/k 移动   Enter 进入", "回主菜单");
    session.press(ESC);
    session.wait_until("回到对话", |s| !s.screen().contains("● 配置"));
}

#[test]
fn vim_keys_are_the_default_and_a_change_reaches_the_chat_at_once() {
    let server = served(vec![]);
    let session = Session::start(&server.base_url, &[]);
    session.wait_for("❯", "输入框");
    switch(&session, UP, "j/k");
    help_says(&session, "↑/↓ 滚动");
    let config = std::fs::read_to_string(
        lm_engine::store::paths::Paths::under(session.data()).config_file(),
    )
    .expect("读得到配置");
    assert!(config.contains("keys = \"arrows\""), "{config}");
    switch(&session, DOWN, "↑/↓");
    help_says(&session, "j/k 滚动");
}
