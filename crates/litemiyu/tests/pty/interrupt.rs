//! 刚发出去、她还没开始做事就两下 `Esc` 打断的，撤掉这一轮、那一句放回输入框（照 miyu-tui `tests/interrupt.rs`；蓝图 `tui.md`
//! 「按键」`Esc`，2026-10-07 项目主人定；`app/takeback.rs`）。

use lm_engine::http::testkit::{Piece, Reply};

use crate::cli::{serve, stream};
use crate::replies::says;
use crate::support::Session;

/// 输入框里那一行。
fn input(session: &Session) -> String {
    session
        .screen()
        .lines()
        .find(|l| l.contains('❯'))
        .unwrap_or_default()
        .to_string()
}

#[test]
fn an_interrupt_before_she_starts_puts_the_message_back_in_the_box() {
    // 第一份一个字都不出：她还没开始做事，打断以后撤掉这一轮，那一句放回输入框；放回来的照常能再发。
    let server = serve(vec![
        Reply::stream(vec![Piece::Stall]),
        stream(says(&["收到。"])),
    ]);
    let session = Session::start(&server.base_url, &[]);
    session.wait_for("❯", "输入框");
    session.type_text("等一下再问");
    session.press_enter();
    // 请求发出去了：这一轮开始跑了（只按固定的时间，忙的时候两下 Esc 都落在开轮以前；开轮以前的照「开轮就打断」走同一条路）。
    session.wait_until("这一轮的请求发出去了", |_| {
        !server.talks().is_empty()
    });
    session.press(b"\x1b");
    session.wait_for("再按一次 Esc", "第一下只提示");
    session.press(b"\x1b");
    session.wait_until("那一句放回输入框", |s| {
        input(s).contains("等一下再问")
    });
    session.wait_until("这一轮撤掉了", |s| {
        !s.screen().contains("┃ 等一下再问")
    });
    session.press_enter();
    session.wait_for("收到。", "放回来的照常能再发");
}
