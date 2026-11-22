//! 两下 `Esc` 只认挨着的两下（LM-38，同 Miyu 终端 11-20 项目主人验收提的：「我发送 /loop，会出现这样的情况，然后什么都没有
//! 发生」）：在回答时按一下 `Esc` 只提示；接下来按了别的键，或者那一下 `Esc` 被命令列表、输入历史列表和它的搜索、各种框、
//! 配置界面、选区用掉了，再按一下 `Esc` 不打断，只重新提示。原来上一下记的时刻一直留着，中间关掉一个列表再按一下就打断了。
//!
//! 每一种各起一个：这一轮停在说了一半（假服务器不往下回），第一下 `Esc` 等提示出来；打开那一样东西、按 `Esc` 关掉，各等到
//! 画面变了；让提示换成别的一句，再按一下 `Esc`：提示换回「再按一次 Esc 中断」、没有打断。打断的那一下会把提示收掉
//! （`app/keys.rs` 的 `esc`），接着写「已中断」，所以分得出来。

use lm_engine::http::testkit::{Piece, Reply};

use crate::cli::serve;
use crate::support::Session;

const ESC: &[u8] = b"\x1b";
const CTRL_R: &[u8] = b"\x12";
const BACKSPACE: &[u8] = b"\x7f";
const SHIFT_LEFT: &[u8] = b"\x1b[1;2D";
const CTRL_C: &[u8] = b"\x03";
/// 清空输入框以后的提示。
const CLEARED: &str = "已清空";
/// 在回答时第一下 `Esc` 的提示。
const HINT: &str = "再按一次 Esc 中断";
/// 打断以后时间线上那一行。
const INTERRUPTED: &str = "已中断";

/// 说了一半停住的一份回答：这一轮一直在跑。
fn half_said() -> Reply {
    let chunk =
        r#"data: {"choices":[{"index":0,"delta":{"content":"写到一半"},"finish_reason":null}]}"#;
    Reply::stream(vec![
        Piece::Bytes(format!("{chunk}\n\n").into_bytes()),
        Piece::Stall,
    ])
}

/// 输入框那一行：只看主列（侧边栏「跳转消息」里当前这一句打头也写 `❯`），列表开着时它在列表下面，取最后一行。
fn input(session: &Session) -> String {
    session
        .columns(0, 160 - 38 - 1)
        .lines()
        .rfind(|l| l.contains('❯'))
        .unwrap_or_default()
        .to_string()
}

/// 起一个、说一句，等这一轮说了一半停住；按第一下 `Esc`，等提示出来。
fn running() -> (crate::cli::Served, Session) {
    let server = serve(vec![half_said()]);
    let session = Session::start(&server.base_url, &[]);
    session.wait_for("❯", "输入框");
    session.type_text("问一句");
    session.press_enter();
    session.wait_for("写到一半", "这一轮在跑");
    session.press(ESC);
    session.wait_for(HINT, "第一下只提示");
    (server, session)
}

/// 再按一下 `Esc`：不打断，只重新提示。`Esc` 后面紧跟的键会和它读成一个组合键，按完要等它的效果：先让提示换成别的一句
/// （敲个 `x` 再 `Ctrl+C` 清掉，提示换成「已清空」），这一下 `Esc` 办完时要么提示换回「再按一次 Esc 中断」，要么打断了
/// （原来：提示收掉，接着时间线上写「已中断」）。
fn esc_only_hints(session: &Session, what: &str) {
    session.type_text("x");
    session.wait_until("x 进了输入框", |s| input(s).contains('x'));
    session.press(CTRL_C);
    session.wait_for(CLEARED, "提示换成清空的那一句");
    final_esc(session, what);
}

/// 提示不是「再按一次 Esc 中断」的时候按一下 `Esc`，等它办完：只提示、不打断。
fn final_esc(session: &Session, what: &str) {
    session.press(ESC);
    session.wait_until("这一下 Esc 办完了", |s| {
        let screen = s.screen();
        screen.contains(HINT) || screen.contains(INTERRUPTED)
    });
    let screen = session.screen();
    assert!(
        screen.contains(HINT) && !screen.contains(INTERRUPTED),
        "{what}以后这一下 Esc 是新的头一下，只提示、不打断：\n{screen}"
    );
}

/// 用 `open` 打开一样东西（等 `opened` 说开了），按 `Esc` 关掉（等它说关了），再按一下 `Esc` 只提示。
fn consumed(what: &str, open: impl Fn(&Session), opened: impl Fn(&Session) -> bool) {
    let (_server, session) = running();
    open(&session);
    session.wait_until(&format!("{what}开了"), |s| opened(s));
    session.press(ESC);
    session.wait_until(&format!("{what}关了"), |s| !opened(s));
    esc_only_hints(&session, what);
}

/// 打 `command` 回车开的框；开着时底下写「Esc 关闭」。
fn panel(what: &str, command: &str) {
    consumed(
        what,
        |s| {
            s.type_text(command);
            s.press_enter();
        },
        |s| s.screen().contains("Esc 关闭"),
    );
}

#[test]
fn another_key_between_two_escs_makes_the_second_a_new_first() {
    let (_server, session) = running();
    session.type_text("a");
    session.wait_until("a 进了输入框", |s| input(s).contains('a'));
    esc_only_hints(&session, "按了别的键");
    // 挨着的两下照旧打断（等上一句提示到点收掉，再按两下，第一下等提示出来）。
    session.wait_until("提示到点收掉", |s| !s.screen().contains(HINT));
    session.press(ESC);
    session.wait_for(HINT, "第一下只提示");
    session.press(ESC);
    session.wait_for(INTERRUPTED, "挨着的两下打断");
}

#[test]
fn closing_the_command_list_with_esc_is_not_the_first_esc() {
    let (_server, session) = running();
    session.type_text("/");
    session.wait_for("撤回上一轮", "命令列表开了");
    session.press(ESC);
    session.wait_until("命令列表关了", |s| !s.screen().contains("撤回上一轮"));
    // 那个 `/` 用 `Ctrl+C` 清掉（再敲字会又把列表筛出来；退格删掉的话，空输入框轮换的提示里也有 `/`，等不出删没删）。
    session.press(CTRL_C);
    session.wait_for(CLEARED, "提示换成清空的那一句");
    final_esc(&session, "关掉命令列表");
}

#[test]
fn closing_the_input_history_with_esc_is_not_the_first_esc() {
    consumed(
        "输入历史列表",
        |s| s.press(CTRL_R),
        |s| s.screen().contains("Enter 填入输入框"),
    );
}

#[test]
fn leaving_the_history_search_with_esc_is_not_the_first_esc() {
    let (_server, session) = running();
    session.press(CTRL_R);
    session.wait_for("Enter 填入输入框", "输入历史列表");
    session.type_text("/");
    session.wait_for("输入以搜索", "进了搜索");
    session.type_text("问");
    session.wait_until("筛成一条", |s| s.screen().contains("1 条"));
    // 第一下退出搜索，第二下关列表：两下都用掉了。
    session.press(ESC);
    session.wait_until("退出搜索", |s| !s.screen().contains("输入以搜索"));
    session.press(ESC);
    session.wait_until("列表关了", |s| !s.screen().contains("Enter 填入输入框"));
    esc_only_hints(&session, "退出搜索、关掉输入历史列表");
}

#[test]
fn closing_the_model_list_with_esc_is_not_the_first_esc() {
    panel("/model 的框", "/model");
}

#[test]
fn closing_the_effort_list_with_esc_is_not_the_first_esc() {
    panel("/effort 的框", "/effort");
}

#[test]
fn closing_the_preset_list_with_esc_is_not_the_first_esc() {
    panel("/preset 的框", "/preset");
}

#[test]
fn closing_the_session_list_with_esc_is_not_the_first_esc() {
    panel("/sessions 的框", "/sessions");
}

#[test]
fn closing_help_with_esc_is_not_the_first_esc() {
    panel("/help 的框", "/help");
}

#[test]
fn closing_status_with_esc_is_not_the_first_esc() {
    panel("/status 的框", "/status");
}

#[test]
fn leaving_settings_with_esc_is_not_the_first_esc() {
    consumed(
        "配置界面",
        |s| {
            s.type_text("/config");
            s.press_enter();
        },
        |s| s.screen().contains("Enter 进入"),
    );
}

#[test]
fn clearing_a_selection_in_the_body_with_esc_is_not_the_first_esc() {
    // 拖选是鼠标：中间一个键都不按，只有那一下 `Esc` 用掉了。松开时复制，提示换成「已复制」。
    let (_server, session) = running();
    let (row, col) = find(&session, "写").expect("说了一半的那一行");
    session.drag((col, row), (col + 6, row));
    session.wait_until("选上了", |s| s.inverse(row, col + 2));
    session.wait_for("已复制", "松开复制，提示换了一句");
    session.press(ESC);
    session.wait_until("选区取消了", |s| !s.inverse(row, col + 2));
    final_esc(&session, "取消正文里的选区");
}

#[test]
fn clearing_a_selection_in_the_input_with_esc_is_not_the_first_esc() {
    let (_server, session) = running();
    session.type_text("abc");
    session.wait_until("abc 进了输入框", |s| input(s).contains("abc"));
    session.press(SHIFT_LEFT);
    let selected = |s: &Session| find(s, "c").is_some_and(|(row, col)| s.inverse(row, col));
    session.wait_until("选上了 c", selected);
    session.press(ESC);
    session.wait_until("选区取消了", |s| !selected(s));
    session.press(BACKSPACE);
    session.wait_until("删掉了 c", |s| !input(s).contains("abc"));
    esc_only_hints(&session, "取消输入框里的选区");
}

/// 主列（侧边栏左边）里头一个写着 `glyph` 的格子：（行，列）。输入框里的 `c` 是 `abc` 的那一个（找 `b` 后面一格）。
fn find(session: &Session, glyph: &str) -> Option<(u16, u16)> {
    let main = 160 - 38 - 1;
    (0..45u16)
        .flat_map(|row| (0..main).map(move |col| (row, col)))
        .find(|&(row, col)| {
            let at = |c: u16| session.cell(row, c).map(|(text, _)| text);
            at(col).as_deref() == Some(glyph)
                && (glyph != "c"
                    || (col >= 2
                        && at(col - 1).as_deref() == Some("b")
                        && at(col - 2).as_deref() == Some("a")))
        })
}
