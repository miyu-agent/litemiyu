//! `litemiyu ask`：三种输出、标准输入、会话。不进伪终端，标准输入、输出接管道。

use crate::cli::{Home, Input, json, serve, stream};
use crate::replies::{calls, says};

/// 调一次 `read` 读 `notes.txt`，再说一句：几样测试都照它。
fn read_then_say() -> Vec<lm_engine::http::testkit::Reply> {
    vec![
        stream(calls("read", r#"{"file_path":"notes.txt"}"#)),
        stream(says(&["笔记里", "写着 hello。"])),
        stream(says(&["标题"])),
    ]
}

#[test]
fn text_prints_only_the_answer_on_stdout_and_the_step_on_stderr() {
    let server = serve(read_then_say());
    let home = Home::new(Some(&server.base_url));
    std::fs::write(home.work.join("notes.txt"), "hello\n").expect("写得进");
    let out = home.run(&["ask", "看看", "notes.txt"], Input::Null);
    assert_eq!(out.code, Some(0), "{out:?}");
    assert_eq!(out.stdout, "笔记里写着 hello。\n");
    assert!(out.stderr.contains("notes.txt"), "一步一行：{}", out.stderr);
    assert!(out.stderr.contains("· 输入 "), "用量那一行：{}", out.stderr);
    assert!(
        !out.stderr.contains('\x1b'),
        "不是终端不上色：{:?}",
        out.stderr
    );
    let talks = server.talks();
    assert!(talks[0].contains("看看 notes.txt"), "几个词用空格连起来");
    assert_eq!(home.sessions().len(), 1, "新开了一个会话");
}

#[test]
fn json_is_one_object_with_the_session_turns_usage_and_exit() {
    let server = serve(read_then_say());
    let home = Home::new(Some(&server.base_url));
    std::fs::write(home.work.join("notes.txt"), "hello\n").expect("写得进");
    let out = home.run(&["ask", "--format", "json", "看看"], Input::Null);
    assert_eq!(out.code, Some(0), "{out:?}");
    let lines: Vec<&str> = out.stdout.lines().collect();
    assert_eq!(lines.len(), 1, "一行：{}", out.stdout);
    let value = json(lines[0]);
    assert_eq!(value["exit"], 0);
    assert_eq!(
        value["session"].as_str(),
        home.sessions().first().map(String::as_str)
    );
    let turns = value["turns"].as_array().expect("turns 是数组");
    assert_eq!(turns.len(), 1);
    assert_eq!(turns[0]["text"], "笔记里写着 hello。");
    assert_eq!(turns[0]["reason"], "completed");
    assert_eq!(
        turns[0]["usage"]["output"], 12,
        "两次请求的加起来（调工具那一次没报用量）"
    );
    assert_eq!(turns[0]["usage"]["input"], 120);
    assert_eq!(turns[0]["usage"]["cache_read"], 0);
    assert!(
        value.get("error").is_none() && value.get("reason").is_none(),
        "没有的格不写"
    );
    let keys: Vec<&String> = value.as_object().expect("对象").keys().collect();
    let mut sorted = keys.clone();
    sorted.sort();
    assert_eq!(keys, sorted, "格照名字的字母先后排");
    assert!(lines[0].starts_with("{\"exit\""), "{}", lines[0]);
    assert!(out.stderr.is_empty(), "json 不印旁白：{}", out.stderr);
}

#[test]
fn jsonl_streams_events_in_order_and_ends_with_done() {
    let server = serve(read_then_say());
    let home = Home::new(Some(&server.base_url));
    std::fs::write(home.work.join("notes.txt"), "hello\n").expect("写得进");
    let out = home.run(&["ask", "--format", "jsonl", "看看"], Input::Null);
    assert_eq!(out.code, Some(0), "{out:?}");
    let events: Vec<serde_json::Value> = out.stdout.lines().map(json).collect();
    let types: Vec<&str> = events
        .iter()
        .map(|e| e["type"].as_str().unwrap_or("?"))
        .collect();
    assert_eq!(
        types,
        [
            "session",
            "turn.started",
            "step",
            "step.done",
            "text",
            "text",
            "turn.ended",
            "done"
        ],
        "{}",
        out.stdout
    );
    assert_eq!(events[0]["new"], true);
    assert_eq!(events[2]["tool"], "read");
    assert_eq!(events[2]["id"], events[3]["id"]);
    assert_eq!(events[2]["title"], "读取 notes.txt");
    assert_eq!(events[3]["ok"], true);
    assert_eq!(events[1]["turn"], events[6]["turn"]);
    assert_eq!(events[6]["text"], "笔记里写着 hello。");
    assert_eq!(events[7]["exit"], 0);
    assert_eq!(events[7]["session"], events[0]["session"]);
}

#[test]
fn the_message_comes_from_stdin_and_a_dash_appends_it() {
    let server = serve(vec![stream(says(&["好"])), stream(says(&["好"]))]);
    let home = Home::new(Some(&server.base_url));
    let out = home.run(&["ask"], Input::Bytes(b"from the pipe\n".to_vec()));
    assert_eq!(out.code, Some(0), "{out:?}");
    let out = home.run(&["ask", "审一下", "-"], Input::Bytes(b"diff body".to_vec()));
    assert_eq!(out.code, Some(0), "{out:?}");
    let talks = server.talks();
    assert!(
        talks[0].contains("from the pipe\\n"),
        "整段就是消息：{}",
        talks[0]
    );
    assert!(
        talks[1].contains("审一下\\n\\ndiff body"),
        "空一行接在后面：{}",
        talks[1]
    );
}

#[test]
fn a_big_stdin_goes_through_whole() {
    let server = serve(vec![stream(says(&["收到"]))]);
    let home = Home::new(Some(&server.base_url));
    let big: String = "abcdefghij".repeat(300_000);
    let out = home.run(
        &["ask", "数一数", "-"],
        Input::Bytes(big.clone().into_bytes()),
    );
    assert_eq!(out.code, Some(0), "{out:?}");
    assert!(server.talks()[0].contains(&big), "3 MB 整段都发出去了");
}

#[test]
fn words_with_an_open_pipe_do_not_wait_and_no_words_with_closed_stdin_is_usage() {
    let server = serve(vec![stream(says(&["好"]))]);
    let home = Home::new(Some(&server.base_url));
    let out = home.run(&["ask", "只看参数"], Input::Open);
    assert_eq!(out.code, Some(0), "写了消息不读一直开着的管道：{out:?}");
    let out = home.run(&["ask"], Input::Null);
    assert_eq!(out.code, Some(2), "{out:?}");
    assert!(out.stderr.contains("缺少参数"), "{}", out.stderr);
    let out = home.run(&["ask", "-"], Input::Bytes(vec![0xff, 0xfe]));
    assert_eq!(out.code, Some(2), "{out:?}");
    assert!(out.stderr.contains("UTF-8"), "{}", out.stderr);
    let out = home.run(&["ask", "--format", "jsonl"], Input::Null);
    assert_eq!(out.code, Some(2));
    let done = json(out.stdout.trim());
    assert_eq!(
        (done["type"].as_str(), done["reason"].as_str()),
        (Some("done"), Some("usage"))
    );
    assert!(done.get("session").is_none(), "开好会话以前的不带会话");
}

#[test]
fn continue_and_session_talk_on_in_the_same_session() {
    let server = serve(vec![
        stream(says(&["第一句的回答"])),
        stream(says(&["第二句的回答"])),
        stream(says(&["第三句的回答"])),
    ]);
    let home = Home::new(Some(&server.base_url));
    assert_eq!(home.run(&["ask", "第一句"], Input::Null).code, Some(0));
    let first = home.sessions();
    let out = home.run(&["ask", "-c", "--format", "jsonl", "第二句"], Input::Null);
    assert_eq!(out.code, Some(0), "{out:?}");
    let session = json(out.stdout.lines().next().expect("有一行"));
    assert_eq!(session["new"], false);
    assert_eq!(
        session["session"].as_str(),
        first.first().map(String::as_str)
    );
    let out = home.run(&["ask", "-s", &first[0], "第三句"], Input::Null);
    assert_eq!(out.code, Some(0), "{out:?}");
    assert_eq!(home.sessions(), first, "一直是那一个会话");
    let talks = server.talks();
    assert!(
        talks[2].contains("第一句的回答") && talks[2].contains("第二句的回答"),
        "带着前面的"
    );
}

#[test]
fn a_missing_session_and_continue_with_none_say_so_and_exit_1() {
    let server = serve(Vec::new());
    let home = Home::new(Some(&server.base_url));
    let out = home.run(&["ask", "-c", "hi"], Input::Null);
    assert_eq!(out.code, Some(1), "{out:?}");
    assert!(out.stderr.contains("还没有会话"), "{}", out.stderr);
    let out = home.run(
        &["ask", "-s", "nope", "--format", "json", "hi"],
        Input::Null,
    );
    assert_eq!(out.code, Some(1), "{out:?}");
    assert_eq!(json(out.stdout.trim())["reason"], "not_found");
    assert!(server.requests().is_empty(), "什么都没发");
}

#[test]
fn a_closed_stdout_does_not_crash() {
    let server = serve(vec![
        stream(says(&["很长的回答"; 50])),
        stream(says(&["题"])),
    ]);
    let home = Home::new(Some(&server.base_url));
    let mut command = home.command(&["ask", "hi"]);
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = command.spawn().expect("起得来");
    drop(child.stdout.take());
    let out = crate::cli::wait(child);
    assert_eq!(
        out.code,
        Some(0),
        "那头关了照样跑完、不被 SIGPIPE 杀掉：{out:?}"
    );
}

/// 别的进程开着的会话，全屏 `-s` 不硬开（退出码 1）。`ask -s` 从 LM-59 起也不硬接、但不再报 `taken`：它把这一句交给拿着
/// 会话的那个进程、跟着日志等回答（`ask_open.rs` 看的是开在窗口里、答得上来的样子）；这里拿着会话的那一头卡在一次不回话
/// 的请求上，等不到回答，照 `--timeout` 收：退出码 3、`timed_out`，不动那个会话。LM-3 从 `session_cli.rs` 挪来：原来还查
/// `rename -s`、`session delete` 也不动它，这两个子命令删了；界面里改名、删除开着的会话另有测试（`sessions.rs`、`rename.rs`）。
#[test]
fn an_open_session_is_not_taken_by_ask_or_the_full_screen() {
    let server = serve(vec![
        stream(says(&["一"])),
        lm_engine::http::testkit::Reply::stream(vec![lm_engine::http::testkit::Piece::Stall]),
    ]);
    let home = Home::new(Some(&server.base_url));
    assert_eq!(home.run(&["ask", "开个会话"], Input::Null).code, Some(0));
    let id = home.sessions().remove(0);
    let mut command = home.command(&["ask", "-s", &id, "停住"]);
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    let mut holder = command.spawn().expect("起得来");
    crate::cli::until("第二次请求停住了", || server.talks().len() >= 2);
    let out = home.run(
        &["ask", "-s", &id, "--timeout", "1", "--format", "json", "hi"],
        Input::Null,
    );
    assert_eq!(out.code, Some(3), "{out:?}");
    assert_eq!(json(out.stdout.trim())["reason"], "timed_out");
    assert_eq!(json(out.stdout.trim())["session"], id.as_str());
    assert!(!out.stderr.contains("其他窗口"), "{}", out.stderr);
    assert_eq!(home.sessions().len(), 1, "没有另开会话");
    let full = home.run(&["-s", &id], Input::Null);
    assert_eq!(full.code, Some(1), "全屏也不硬开：{full:?}");
    drop(holder.kill());
    drop(holder.wait());
}
