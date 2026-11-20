//! kitty 键盘协议（同 miyu-tui `main.rs` 的 `enter`、`leave`：终端认得就打开，Shift+Enter 才分得出来）：LiteMiyu 启动不等终端
//! 回话，问颜色的回话到了才问认不认（`reader.rs`）；认得的打开、退出时关掉，不认得、不回话的一直不开。

use crate::support::{Options, Session, contains, served};

/// 打开键盘协议（只要分得出转义码那一档，`DISAMBIGUATE_ESCAPE_CODES`）。
const PUSH: &[u8] = b"\x1b[>1u";
/// 关掉。
const POP: &[u8] = b"\x1b[<1u";
/// 问认不认（接着问 DA1）。
const ASK: &[u8] = b"\x1b[?u\x1b[c";

fn options(answer_colors: bool, keyboard: Option<bool>) -> Options {
    Options {
        answer: answer_colors.then_some(("c0/ca/f5", "1a/1b/26")),
        keyboard,
        ..Options::default()
    }
}

/// 打几个字、等它上了屏：读按键的线程问键盘协议时不读按键，字上了屏就是问完、主线程也照回话办完了。
fn typed_and_shown(session: &Session, text: &str) {
    session.type_text(text);
    session.wait_for(text, "打的字上了屏");
}

#[test]
fn a_terminal_that_knows_the_protocol_gets_it_on_and_off_again_at_exit() {
    let server = served(Vec::new());
    let session = Session::start_with(&server.base_url, &[], &options(true, Some(true)));
    session.wait_for("❯", "输入框");
    session.wait_for_raw(PUSH, "认得的终端打开键盘协议");
    // 分得出 Shift+Enter 了：kitty 写法的 Shift+Enter 换行，不发出去。
    session.type_text("第一行");
    session.press(b"\x1b[13;2u");
    session.type_text("第二行");
    let screen = session.wait_for("第二行", "换了行接着打");
    assert!(screen.contains("第一行"), "{screen}");
    assert!(server.received().is_empty(), "Shift+Enter 没把话发出去");
    // 开着协议时 Ctrl+C 照 kitty 的写法来：清掉框里的字。
    session.press(b"\x1b[99;5u");
    session.wait_until("框里的字清掉了", |s| !s.screen().contains("第二行"));
    session.type_text("/exit");
    session.press_enter();
    assert!(session.wait_exit(), "退出码 0");
    let raw = session.raw();
    let pushed = raw
        .windows(PUSH.len())
        .position(|w| w == PUSH)
        .expect("开过");
    assert!(
        contains(&raw[pushed..], POP),
        "退出时关掉键盘协议，终端还回原样"
    );
}

#[test]
fn a_terminal_that_answers_only_the_device_attributes_never_gets_it() {
    let server = served(Vec::new());
    let session = Session::start_with(&server.base_url, &[], &options(true, Some(false)));
    session.wait_for("❯", "输入框");
    session.wait_for_raw(ASK, "答了颜色以后问认不认");
    typed_and_shown(&session, "照常打字");
    assert!(!contains(&session.raw(), PUSH), "不认得的不开");
}

#[test]
fn a_terminal_that_answers_nothing_is_never_asked_and_never_waited_for() {
    let server = served(Vec::new());
    let session = Session::start_with(&server.base_url, &[], &options(false, None));
    session.wait_for("❯", "输入框");
    typed_and_shown(&session, "照常打字");
    let raw = session.raw();
    assert!(!contains(&raw, ASK), "颜色都不答的终端不问键盘协议");
    assert!(!contains(&raw, PUSH), "也不开");
}
