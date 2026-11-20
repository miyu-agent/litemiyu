//! `Ctrl+Z` 挂起（执行计划第六节 L4 二三）：挂起前把终端还原（退出全屏），回来重新进全屏、整屏重画，接着能打字。伪终端里
//! 没有作业控制的 shell，`SIGTSTP` 被丢掉，程序当场「回来」。

use crate::support::{Session, contains, served};

/// 退出、进入备用屏。
const LEAVE: &[u8] = b"\x1b[?1049l";
const ENTER: &[u8] = b"\x1b[?1049h";

#[test]
fn ctrl_z_gives_the_terminal_back_and_comes_back_whole() {
    let server = served(Vec::new());
    let session = Session::start(&server.base_url, &[]);
    session.wait_for("❯", "输入框");
    session.type_text("hi");
    session.wait_for("❯ hi", "打的字");
    let before = session.raw().len();
    session.type_text("\x1a");
    session.wait_for_raw(LEAVE, "挂起前退出全屏");
    let screen = session.wait_for("❯ hi", "回来以后整屏重画，打的字还在");
    let raw = session.raw();
    let after = &raw[before..];
    let left = after.windows(LEAVE.len()).position(|w| w == LEAVE);
    let back = after.windows(ENTER.len()).rposition(|w| w == ENTER);
    assert!(
        matches!((left, back), (Some(l), Some(b)) if l < b),
        "先退出全屏、再回来：\n{screen}"
    );
    assert!(
        contains(&after[back.unwrap_or(0)..], "❯".as_bytes()),
        "回来以后重画了一整屏"
    );
    session.type_text("x");
    session.wait_for("❯ hix", "回来以后接着打字");
}
