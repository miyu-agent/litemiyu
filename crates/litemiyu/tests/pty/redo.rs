//! `/redo`、`/edit`（Miyu 终端 `app/redo.rs`，内核的 `Redo`）：这一轮原样重来、编辑上一句改完回车重来；重做不留撤销说明，
//! 撤掉的那一轮藏起来，重发的那句落在它原来的位置；编辑时输入框上边框写「编辑上一条 · Esc 取消」，`Esc` 取消；还没开会话的
//! 当场说。数据都在临时目录，供应商是本机的假服务器，不连外网。

use crate::replies::says;
use crate::support::{Session, served};

const ESC: &[u8] = b"\x1b";

/// 第 `n` 次请求（从 0 数）的请求体。
fn request(server: &crate::support::Served, n: usize) -> String {
    let received = server.received();
    String::from_utf8_lossy(received.get(n).map_or(&[][..], Vec::as_slice)).into_owned()
}

#[test]
fn redo_runs_the_last_turn_again_without_an_undo_line() {
    let server = served(vec![says(&["第一次的回答"]), says(&["重来的回答"])]);
    let session = Session::start(&server.base_url, &[]);
    session.wait_for("❯", "输入框");
    // 还没开会话：当场说，不去开一个。
    session.type_text("/redo");
    session.press_enter();
    session.wait_for("无法重做", "还没开会话");
    session.type_text("头一句话");
    session.press_enter();
    session.wait_for("第一次的回答", "第一轮答完");
    session.type_text("/redo");
    session.press_enter();
    session.wait_for("重来的回答", "重来了");
    session.wait_until("撤掉的那一轮藏起来了", |s| {
        !s.screen().contains("第一次的回答")
    });
    // 侧边栏「最近会话」里没起名的这个会话写第一句话（引擎 十，照 Miyu）：列表读到它以后才写，先等它出来。
    session.wait_until("最近会话写第一句话", |s| {
        s.screen().contains("❱ 头一句话")
    });
    let screen = session.screen();
    assert!(!screen.contains("已撤销"), "重做不留撤销说明：\n{screen}");
    assert_eq!(
        screen.matches("头一句话").count(),
        3,
        "正文里一次、侧边栏「跳转消息」一次、「最近会话」一次，不多出一句：\n{screen}"
    );
    // 重来的那一次请求里没有撤掉的回答，那一句照原样再说一次。
    let resent = request(&server, 1);
    assert!(resent.contains("头一句话"), "{resent}");
    assert!(!resent.contains("第一次的回答"), "{resent}");
}

#[test]
fn edit_puts_the_last_message_back_and_sends_it_changed() {
    let server = served(vec![says(&["第一次的回答"]), says(&["改过以后的回答"])]);
    let session = Session::start(&server.base_url, &[]);
    session.wait_for("❯", "输入框");
    session.type_text("头一句话");
    session.press_enter();
    session.wait_for("第一次的回答", "第一轮答完");
    // 放进输入框，上边框写着在编辑；Esc 取消、框里清空。
    session.type_text("/edit");
    session.press_enter();
    // 只看主列：侧边栏「跳转消息」里当前这一句打头也写 `❯`（Miyu `cbb5dc0e`）。
    session.wait_until("那一句放进输入框", |s| {
        let screen = s.columns(0, 160 - 38 - 1);
        screen.contains("编辑上一条 · Esc 取消") && screen.lines().any(|l| l.contains("❯ 头一句话"))
    });
    session.press(ESC);
    session.wait_until("取消了", |s| {
        let screen = s.columns(0, 160 - 38 - 1);
        !screen.contains("编辑上一条") && !screen.lines().any(|l| l.contains("❯ 头一句话"))
    });
    // 再编辑一次，接着打几个字回车：照改过的重来。
    session.type_text("/edit");
    session.press_enter();
    session.wait_for("编辑上一条 · Esc 取消", "又在编辑");
    session.type_text("，改过");
    session.press_enter();
    session.wait_for("改过以后的回答", "照改过的重来了");
    session.wait_until("撤掉的那一轮藏起来了", |s| {
        !s.screen().contains("第一次的回答")
    });
    let screen = session.screen();
    assert!(screen.contains("头一句话，改过"), "{screen}");
    assert!(
        !screen.contains("已撤销") && !screen.contains("编辑上一条"),
        "{screen}"
    );
    let resent = request(&server, 1);
    assert!(resent.contains("头一句话，改过"), "{resent}");
    assert!(!resent.contains("第一次的回答"), "{resent}");
}
