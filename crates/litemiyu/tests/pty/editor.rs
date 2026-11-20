//! 外部编辑器（Miyu 蓝图「按键」`Ctrl+G`，第二趟三）：读按键的线程停下、让出终端给编辑器，退出了收回、整屏重画，接着读。
//! 编辑器换成替身：`$VISUAL` 是一句往文件里写字的 `printf`，不开窗口、不读终端。

use crate::support::{Options, Session, served};

#[test]
fn ctrl_g_hands_the_terminal_to_the_editor_and_reads_its_text_back() {
    let server = served(Vec::new());
    let options = Options {
        env: vec![("VISUAL", "printf 'written in the editor' >")],
        ..Options::default()
    };
    let session = Session::start_with(&server.base_url, &[], &options);
    session.wait_for("❯", "输入框");
    session.type_text("draft");
    session.wait_for("❯ draft", "打的字");
    session.press(b"\x07");
    session.wait_for("❯ written in the editor", "编辑器写的字放回了输入框");
    // 读按键的接着读：再打的字照常进框。
    session.type_text("!");
    session.wait_for("❯ written in the editor!", "收回终端以后照常打字");
}

#[test]
fn the_prompt_row_opens_prompt_md_in_the_editor() {
    // 配置线留的（设计第五节第 15 条）：通用页最后一行「提示词」，Enter 用外部编辑器开配置目录的 prompt.md。
    let server = served(Vec::new());
    let options = Options {
        env: vec![("VISUAL", "printf 'my own prompt' >")],
        ..Options::default()
    };
    let session = Session::start_with(&server.base_url, &[], &options);
    session.wait_for("❯", "输入框");
    session.type_text("/config");
    session.press_enter();
    session.wait_for("Enter 进入", "主菜单");
    session.press_enter();
    session.wait_for("◉ 通用", "通用页");
    session.wait_for("内置提示词", "还没有 prompt.md：写着用出厂的");
    // 「提示词」在「新会话」那一组最后：界面语言、新会话默认往下两行。
    for _ in 0..2 {
        session.press(b"\x1b[B");
    }
    session.press_enter();
    session.wait_for("提示词已保存", "编辑器退出以后状态行");
    session.wait_for("自定义提示词", "这一行跟着变");
    let prompt = lm_engine::store::paths::Paths::under(session.data())
        .config
        .join("prompt.md");
    assert_eq!(
        std::fs::read_to_string(prompt).expect("写进去了"),
        "my own prompt"
    );
}
