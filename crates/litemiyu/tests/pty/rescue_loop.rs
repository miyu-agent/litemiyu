//! 应急模式下的循环和定时触发（设计第五节第 16 条）：循环只是会话里的一个定时器，不写盘。`/loop 5m <话>` 当场交一次、
//! 回执和输入框上面那一行的标记照平常，和上边框右边的「应急模式」不在同一行、互不挤掉（窄窗口也是）；`/loop off` 停掉；
//! 模型没开循环时调 `schedule_wakeup` 照平常挂一个到点继续。工具面是六件。整棵树一个字节没变。

use std::ffi::OsString;

use super::replies::{calls, says};
use super::rescue::{Root, given, writable_files};
use super::support::{Options, Session, served};

/// 应急模式带的工具（`core/engine/setup/rescue.rs` 的 `TOOLS`）。
const TOOLS: [&str; 6] = ["edit", "jobs", "read", "schedule_wakeup", "shell", "write"];

/// 在 `root` 的工作目录里起 `litemiyu --rescue`，窗口 `cols` 列。
fn launch(root: &Root, env: Vec<(String, OsString)>, cols: u16) -> Session {
    let options = Options {
        only_env: env,
        cols: Some(cols),
        ..Options::default()
    };
    let dirs = (
        root.at("unused-data"),
        root.at("unused-home"),
        root.at("work"),
    );
    Session::spawn_at(dirs, &["--rescue"], &options)
}

/// 请求里的工具名，排好。
fn tools(body: &[u8]) -> Vec<String> {
    let json: serde_json::Value = serde_json::from_slice(body).expect("请求是 JSON");
    let mut names: Vec<String> = json["tools"]
        .as_array()
        .expect("有工具")
        .iter()
        .filter_map(|tool| tool["function"]["name"].as_str().map(str::to_string))
        .collect();
    names.sort_unstable();
    names
}

/// 屏幕上有输入框上面那一行的循环标记：记号后面跟着时刻。
fn tagged(screen: &str) -> bool {
    screen.match_indices("\u{f017} ").any(|(at, mark)| {
        screen[at + mark.len()..]
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_digit())
    })
}

/// 开一个循环、它当场跑一轮、停掉，`cols` 列宽的窗口里。
fn a_loop_runs_and_stops(name: &str, cols: u16) {
    let root = Root::new(name);
    let server = served(vec![says(&["磁盘还有空。"])]);
    let env = given(&server, vec![("HOME", root.at("nohome").into())]);
    let before = root.bytes();
    let session = launch(&root, env, cols);
    session.wait_for("应急模式", "输入框上边框的标记");
    session.type_text("/loop 5m 看一下磁盘");
    session.press_enter();
    session.wait_for("已开始循环：每 5m · 看一下磁盘", "开了的回执");
    session.wait_for("磁盘还有空。", "交进去的那一句回了");
    session.wait_until("循环的标记和应急模式的标记都在", |s| {
        let screen = s.screen();
        tagged(&screen) && screen.contains("应急模式")
    });
    let requests = server.requests();
    assert_eq!(tools(&requests[0].body), TOOLS, "应急模式的工具面");
    let pid = session.pid().expect("有进程号");
    assert_eq!(writable_files(pid), Vec::<String>::new(), "开着能写的文件");
    assert_eq!(root.bytes(), before, "开着循环时一个字节没变");
    session.type_text("/loop off");
    session.press_enter();
    session.wait_for("已停止此会话的循环和定时触发。", "停了的回执");
    session.wait_until("回执收了、标记没了", |s| {
        let screen = s.screen();
        !screen.contains("已停止此会话的循环和定时触发。") && !tagged(&screen)
    });
    assert!(session.screen().contains("应急模式"), "应急模式的标记照旧");
    session.press(b"\x04");
    assert!(session.wait_exit(), "Ctrl+D 好好退出");
    assert_eq!(root.bytes(), before, "退出以后也没变");
}

#[test]
fn a_loop_runs_once_at_once_shows_its_tag_beside_the_rescue_tag_and_stops() {
    a_loop_runs_and_stops("loop-wide", 160);
}

#[test]
fn in_a_narrow_window_the_loop_tag_and_the_rescue_tag_both_show() {
    a_loop_runs_and_stops("loop-narrow", 48);
}

/// 模型没开循环时调 `schedule_wakeup`：照平常挂一个到点继续，标记写时刻；`/loop off` 停得掉。什么都不写。
#[test]
fn a_wakeup_the_model_schedules_is_kept_in_memory_and_stops() {
    let root = Root::new("wakeup");
    let server = served(vec![
        calls(
            "schedule_wakeup",
            r#"{"delay_seconds":600,"prompt":"再看一下磁盘","reason":"清理大约十分钟"}"#,
        ),
        says(&["十分钟以后我再看。"]),
    ]);
    let env = given(&server, vec![("HOME", root.at("nohome").into())]);
    let before = root.bytes();
    let session = launch(&root, env, 160);
    session.wait_for("应急模式", "输入框上边框的标记");
    session.type_text("清理完告诉我");
    session.press_enter();
    session.wait_for("十分钟以后我再看。", "回了");
    session.wait_until("输入框上面那一行的记号和时刻", |s| {
        tagged(&s.screen())
    });
    let second = String::from_utf8_lossy(&server.received()[1]).into_owned();
    assert!(
        !second.contains("\"is_error\":true") && !second.contains("not available"),
        "定时触发成了：{second}"
    );
    assert_eq!(root.bytes(), before, "挂着定时触发时一个字节没变");
    session.type_text("/loop off");
    session.press_enter();
    session.wait_for("已停止此会话的循环和定时触发。", "停了的回执");
    session.wait_until("标记没了", |s| !tagged(&s.screen()));
    session.press(b"\x04");
    assert!(session.wait_exit(), "Ctrl+D 好好退出");
    assert_eq!(root.bytes(), before, "退出以后也没变");
}
