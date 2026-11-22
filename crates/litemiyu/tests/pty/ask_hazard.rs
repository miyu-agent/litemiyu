//! `litemiyu ask` 没有人的时候：要问人的高危命令不写 `--yes` 就拒、说明、退出码
//! 4，写了才放行；灭顶的写了也拒；标准输入是终端也不问。

use std::io::Read;

use portable_pty::{CommandBuilder, PtySize, native_pty_system};

use crate::cli::{Home, Input, json, serve, stream, wait_child_in_pty};
use crate::replies::{calls, says};

/// 调一次 `shell` 跑 `command`，再说一句。
fn shell_then_say(command: &str) -> Vec<lm_engine::http::testkit::Reply> {
    let args = serde_json::json!({"command": command, "description": "Test"}).to_string();
    vec![
        stream(calls("shell", &args)),
        stream(says(&["好了"])),
        stream(says(&["标题"])),
    ]
}

#[test]
fn a_risky_command_without_yes_is_refused_reported_and_exits_4() {
    let server = serve(shell_then_say("git push --force; touch pushed"));
    let home = Home::new(Some(&server.base_url));
    let out = home.run(&["ask", "推上去"], Input::Null);
    assert_eq!(out.code, Some(4), "{out:?}");
    assert!(!home.work.join("pushed").exists(), "没人确认就没跑");
    assert!(
        out.stderr.contains("1 步需要确认，已跳过"),
        "{}",
        out.stderr
    );
    assert!(out.stderr.contains("--yes"), "说了怎么放行：{}", out.stderr);
    assert_eq!(out.stdout, "好了\n");
    let talks = server.talks();
    assert!(
        talks[1].contains("no one here can give it"),
        "她拿到的是内核现成的「没人能确认」：{}",
        talks[1]
    );
}

#[test]
fn json_lists_the_refused_step_and_jsonl_warns_before_done() {
    let server = serve(shell_then_say("git push --force; touch pushed"));
    let home = Home::new(Some(&server.base_url));
    let out = home.run(&["ask", "--format", "json", "推上去"], Input::Null);
    assert_eq!(out.code, Some(4), "{out:?}");
    let value = json(out.stdout.trim());
    assert_eq!(value["exit"], 4);
    assert_eq!(value["reason"], "unattended");
    assert_eq!(value["denied"][0]["tool"], "shell");
    let server = serve(shell_then_say("git push --force; touch pushed"));
    let home = Home::new(Some(&server.base_url));
    let out = home.run(&["ask", "--format", "jsonl", "推上去"], Input::Null);
    let events: Vec<serde_json::Value> = out.stdout.lines().map(json).collect();
    let n = events.len();
    assert_eq!(events[n - 2]["type"], "warning", "{}", out.stdout);
    assert_eq!(events[n - 1]["type"], "done");
    assert_eq!(events[n - 1]["reason"], "unattended");
    let done = events
        .iter()
        .find(|e| e["type"] == "step.done")
        .expect("有一步");
    assert_eq!(done["ok"], false);
}

#[test]
fn yes_lets_a_risky_command_run_once() {
    let server = serve(shell_then_say(
        "git push --force; echo \"push=$?\"; touch pushed",
    ));
    let home = Home::new(Some(&server.base_url));
    let out = home.run(&["ask", "--yes", "推上去"], Input::Null);
    assert_eq!(out.code, Some(0), "{out:?}");
    assert!(home.work.join("pushed").exists(), "放行了");
    // 放行了它就真的跑：测试的目录不许被 git 认成外面这个真的仓库（`Home::new` 放的那个 `.git` 文件），推一定是失败的。
    let talks = server.talks();
    assert!(
        talks[1].contains("push=128"),
        "git 在测试的目录里认不到仓库，什么都没推：{}",
        &talks[1][talks[1].find("\"role\":\"tool\"").unwrap_or(0)..]
    );
}

#[test]
fn a_catastrophic_command_is_refused_even_with_yes() {
    let server = serve(vec![]);
    let home = Home::new(Some(&server.base_url));
    std::fs::write(home.home.join("keep"), "x").expect("写得进");
    let command = format!("rm -rf '{}'", home.home.display());
    let server = serve(shell_then_say(&command));
    home.write_config(&crate::cli::config(&server.base_url));
    let out = home.run(&["ask", "--yes", "--format", "jsonl", "清掉"], Input::Null);
    assert!(home.home.join("keep").exists(), "家目录还在：{out:?}");
    let done = out
        .stdout
        .lines()
        .map(json)
        .find(|e| e["type"] == "step.done")
        .expect("有一步");
    assert_eq!(done["ok"], false);
    assert_eq!(
        out.code,
        Some(0),
        "灭顶的拒是工具的事，不算没人确认：{out:?}"
    );
}

#[test]
fn a_terminal_on_stdin_is_not_asked_either() {
    let server = serve(shell_then_say("git push --force; touch pushed"));
    let home = Home::new(Some(&server.base_url));
    let pty = native_pty_system()
        .openpty(PtySize {
            rows: 40,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("开得了伪终端");
    let mut command = CommandBuilder::new(env!("CARGO_BIN_EXE_litemiyu"));
    command.args(["ask", "推上去"]);
    command.env_clear();
    command.cwd(&home.work);
    for (key, value) in [
        // `SHELL`、`HOME` 钉住：不给的话 portable-pty 调 `getpwuid` 去查（见 `support.rs`）。
        ("SHELL", "/bin/sh"),
        ("PATH", "/usr/bin:/bin"),
        ("LANG", "zh_CN.UTF-8"),
        ("TERM", "xterm-256color"),
        ("LITEMIYU_TEST_KEY", "sk-test"),
    ] {
        command.env(key, value);
    }
    command.env("LITEMIYU_HOME", &home.data);
    command.env("HOME", &home.home);
    let child = pty.slave.spawn_command(command).expect("起得来");
    drop(pty.slave);
    let mut reader = pty.master.try_clone_reader().expect("读得了");
    let screen = std::thread::spawn(move || {
        let mut all = Vec::new();
        drop(reader.read_to_end(&mut all));
        String::from_utf8_lossy(&all).into_owned()
    });
    let code = wait_child_in_pty(child);
    drop(pty.master);
    let shown = screen.join().expect("读完了");
    assert_eq!(code, Some(4), "不等人敲 y/n：{shown}");
    assert!(!home.work.join("pushed").exists());
    assert!(!shown.contains("\x1b[?1049h"), "没进全屏：{shown:?}");
}
