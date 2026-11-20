//! 切进子会话（Miyu 蓝图 `tui.md`「切进子会话」，界面线第七趟）：点子代理状态行里的子代理，正文换成它的时间线（交代的活
//! 写「从主会话收到消息」），输入框上边框写「子代理 · 名字」；它还在跑时接着出字；它在跑时按 `Esc` 只提示，它报完了按
//! `Esc` 回主会话，主会话那一份照旧在、报完了叫醒的那一句也在。
//!
//! 子会话等的是工作目录里一个文件（数着最多等 30 秒，测试中途失败也自己退出，不留进程；不写 GNU 的 `timeout`，macOS 上没有，
//! LM-7）：测试切进去以后才放这个文件，它接着做完，看得到它接着出字；不靠睡几毫秒碰运气。

use crate::replies::{calls, says};
use crate::support::{Options, Session, served_by};

/// 子会话的 system 末尾接的那一段（`resources/core/jobs/subagent-venue.txt`）的开头。
const CHILD: &str = "You are a subagent, started by another session";

/// 子会话先跑一条等 `go` 文件的命令，等到了说一句；主会话派它，派出去说一句，看到回报说一句。
fn waiting(body: &str) -> String {
    let tool = body.contains(r#""role":"tool""#);
    if body.contains(CHILD) {
        return if tool {
            says(&["child: ", "等到了 go。"])
        } else {
            calls(
                "shell",
                r#"{"command":"sh -c 'i=0; until [ -e go ] || [ $i -ge 600 ]; do sleep 0.05; i=$((i+1)); done'","description":"Wait for go"}"#,
            )
        };
    }
    if body.contains("<subagent-report") {
        says(&["看到回报了。"])
    } else if tool {
        says(&["派出去了。"])
    } else {
        calls(
            "subagent",
            r#"{"description":"Wait it","prompt":"Wait for the go file."}"#,
        )
    }
}

#[test]
fn clicking_a_subagent_shows_its_timeline_live_and_esc_goes_back() {
    let base_url = served_by(waiting);
    let session = Session::start(&base_url, &[]);
    session.wait_for("❯", "输入框");
    session.type_text("派个子代理等文件");
    session.press_enter();
    session.wait_for("○ Wait it", "子代理状态行里它在跑");
    session.wait_for("派出去了。", "主会话派出去以后说的");
    let row = session
        .row_of(0, "○ Wait it")
        .expect("状态行里它那一行在屏幕上");
    session.click(10, row);
    session.wait_for("子代理 · Wait it", "输入框上边框写它的名字");
    let screen = session.wait_for("从主会话收到消息", "交代的活照主会话发来的话画");
    assert!(
        !screen.contains("派出去了。"),
        "正文换成了它的时间线：\n{screen}"
    );
    session.wait_for("Wait for go", "它正在跑的那一步");
    // 它还在跑：第一下 Esc 只提示，不回去。
    session.press(b"\x1b");
    session.wait_for("再按一次 Esc", "跑着的按 Esc 只提示");
    assert!(session.screen().contains("子代理 · Wait it"), "还看着它");
    std::fs::write(session.work().join("go"), "").expect("写得进");
    session.wait_for("等到了 go。", "切进去以后它接着出的字");
    session.wait_for(
        "● Wait it  完成",
        "它报完了：状态行里看着的这一行写怎么结束的",
    );
    session.wait_until("Esc 的提示收掉", |s| {
        !s.screen().contains("再按一次 Esc")
    });
    session.shot("child");
    // 它报完了：输入框空着按 Esc 回主会话，主会话那一份照旧在，回报叫醒的那一句也在。
    session.press(b"\x1b");
    let screen = session.wait_for("看到回报了。", "回到主会话");
    assert!(
        screen.contains("派出去了。"),
        "主会话的正文还在：\n{screen}"
    );
    assert!(
        !screen.contains("子代理 · Wait it"),
        "上边框不写子代理了：\n{screen}"
    );
}

#[test]
fn a_narrow_window_goes_into_a_subagent_and_back_by_keyboard_alone() {
    // 窄窗口没有侧边栏（界面线第九趟）：输入框空着按 `↓` 到子代理状态行，`↓` 到它那一行、`Enter` 切进去；`↑` 到主会话那一行、
    // `Enter` 回来（同 Miyu `jobs_key`）。不碰鼠标。
    let base_url = served_by(waiting);
    let options = Options {
        cols: Some(70),
        ..Options::default()
    };
    let session = Session::start_with(&base_url, &[], &options);
    session.wait_for("❯", "输入框");
    session.type_text("派个子代理等文件");
    session.press_enter();
    session.wait_for("派出去了。", "主会话派出去以后说的");
    let screen = session.wait_for("● 主会话", "输入框下面的子代理状态行");
    assert!(
        screen.contains("○ Wait it"),
        "它那一行在状态行里：\n{screen}"
    );
    session.press(b"\x1b[B");
    session.wait_for("❯ ● 主会话", "焦点到主会话那一行");
    session.press(b"\x1b[B");
    session.wait_for("❯ ○ Wait it", "焦点到它那一行");
    session.press_enter();
    session.wait_for("子代理 · Wait it", "切进去了：输入框上边框写它的名字");
    let screen = session.wait_for("从主会话收到消息", "交代的活照主会话发来的话画");
    assert!(
        !screen.contains("派出去了。"),
        "正文换成了它的时间线：\n{screen}"
    );
    assert!(
        screen.contains("❯ ● Wait it"),
        "焦点留在这一行、它成了实心：\n{screen}"
    );
    std::fs::write(session.work().join("go"), "").expect("写得进");
    session.wait_for("等到了 go。", "切进去以后它接着出的字");
    session.wait_for("● Wait it  完成", "它报完了还列着，写怎么结束的");
    session.shot("agents-row");
    session.press(b"\x1b[A");
    session.wait_for("❯ ○ 主会话", "焦点回到主会话那一行");
    session.press_enter();
    let screen = session.wait_for("看到回报了。", "回到主会话");
    assert!(
        screen.contains("派出去了。"),
        "主会话的正文还在：\n{screen}"
    );
    assert!(
        !screen.contains("子代理 · Wait it"),
        "上边框不写子代理了：\n{screen}"
    );
}
