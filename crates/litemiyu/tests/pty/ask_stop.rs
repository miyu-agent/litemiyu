//! `litemiyu ask` 被打断：退出码 3，后台命令一个不留，日志能再载入接着说；
//! `--timeout`、`--max-steps`。

use std::process::Stdio;

use lm_engine::http::testkit::{Piece, Reply};

use crate::cli::{Home, Input, json, processes_with, serve, stream, until, wait};
use crate::replies::{calls, says};

/// 放一条后台命令（`marker` 是它本身，例如 `sleep 3011`：外面的 `sh` 和里面的 `sleep` 命令行里都有），下一次请求停住不回。
fn background_then_stall(marker: &str) -> Vec<Reply> {
    let args = serde_json::json!({
        "command": format!("{marker}; true"),
        "description": "Wait",
        "run_in_background": true,
    })
    .to_string();
    vec![
        stream(calls("shell", &args)),
        Reply::stream(vec![Piece::Stall]),
    ]
}

/// 发 `signal` 给停在第二次请求上的 `litemiyu ask`：退出码 3，后台命令没了，日志接着说得下去。
fn stopped_by(signal: &str, marker: &str) {
    let server = serve(background_then_stall(marker));
    let home = Home::new(Some(&server.base_url));
    let mut command = home.command(&["ask", "--format", "jsonl", "跑一下"]);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let child = command.spawn().expect("起得来");
    let pid = child.id();
    until("停在第二次请求上", || server.talks().len() >= 2);
    until("后台命令跑起来了", || {
        !processes_with(marker).is_empty()
    });
    let sent = std::process::Command::new("kill")
        .args([signal, &pid.to_string()])
        .status()
        .expect("kill 跑得了");
    assert!(sent.success());
    let out = wait(child);
    assert_eq!(out.code, Some(3), "{signal}：{out:?}");
    let done = json(out.stdout.lines().last().expect("有收尾那一行"));
    assert_eq!(
        (done["type"].as_str(), done["exit"].as_i64()),
        (Some("done"), Some(3))
    );
    assert_eq!(done["reason"], "interrupted");
    until("后台命令一个不留", || {
        processes_with(marker).is_empty()
    });
    // 日志是完整的：换一个服务器接着说，带着前面那一轮。
    let next = serve(vec![stream(says(&["接着说"])), stream(says(&["题"]))]);
    home.write_config(&crate::cli::config(&next.base_url));
    let again = home.run(&["ask", "-c", "还在吗"], Input::Null);
    assert_eq!(again.code, Some(0), "{again:?}");
    assert!(next.talks()[0].contains("跑一下"), "载入了上一次的日志");
}

#[test]
fn sigint_interrupts_cleans_up_and_leaves_a_loadable_log() {
    stopped_by("-INT", "sleep 3011");
}

#[test]
fn sigterm_interrupts_cleans_up_and_leaves_a_loadable_log() {
    stopped_by("-TERM", "sleep 3012");
}

#[test]
fn timeout_stops_a_reply_that_never_comes() {
    let server = serve(vec![Reply::stream(vec![Piece::Stall])]);
    let home = Home::new(Some(&server.base_url));
    let started = std::time::Instant::now();
    let out = home.run(
        &["ask", "--timeout", "2s", "--format", "json", "hi"],
        Input::Null,
    );
    assert_eq!(out.code, Some(3), "{out:?}");
    assert_eq!(json(out.stdout.trim())["reason"], "timed_out");
    assert!(
        started.elapsed() < std::time::Duration::from_secs(30),
        "到点就收"
    );
    let out = home.run(&["ask", "--timeout", "1.5", "hi"], Input::Null);
    assert_eq!(out.code, Some(2), "时长写错是参数不对：{out:?}");
}

#[test]
fn max_steps_stops_after_that_many_requests() {
    let args = serde_json::json!({"command": "true", "description": "Run"}).to_string();
    let server = serve(vec![
        stream(calls("shell", &args)),
        stream(calls("shell", &args)),
        stream(calls("shell", &args)),
        stream(says(&["完"])),
    ]);
    let home = Home::new(Some(&server.base_url));
    let out = home.run(
        &["ask", "--max-steps", "1", "--format", "json", "跑"],
        Input::Null,
    );
    assert_eq!(out.code, Some(3), "{out:?}");
    assert_eq!(json(out.stdout.trim())["reason"], "max_steps");
    assert!(
        server.talks().len() <= 2,
        "到了上限就停：{}",
        server.talks().len()
    );
}
