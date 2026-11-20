//! `litemiyu ask` 往坏里：没配模型、配置坏了、数据目录只读、服务器回 401、500、回一半
//! 断开。每种是一句看得懂的话（文本）或者 JSON 里带着错误种类，退出码对。

use lm_engine::http::testkit::{Piece, Reply};

use crate::cli::{Home, Input, json, serve};

#[test]
fn no_model_is_exit_5_with_a_hint() {
    let home = Home::new(None);
    let out = home.run(&["ask", "hi"], Input::Null);
    assert_eq!(out.code, Some(5), "{out:?}");
    assert!(out.stderr.contains("没有可用的模型"), "{}", out.stderr);
    assert!(out.stderr.contains("litemiyu config"), "{}", out.stderr);
    let out = home.run(&["ask", "--format", "jsonl", "hi"], Input::Null);
    assert_eq!(out.code, Some(5));
    assert_eq!(json(out.stdout.trim())["reason"], "no_model");
}

#[test]
fn a_broken_config_is_exit_1_and_says_so() {
    let home = Home::new(None);
    home.write_config("[providers.fake\nthis is not toml");
    let out = home.run(&["ask", "hi"], Input::Null);
    assert_eq!(out.code, Some(1), "{out:?}");
    assert!(out.stderr.starts_with("litemiyu: "), "{}", out.stderr);
    assert!(out.stderr.contains("配置"), "{}", out.stderr);
}

#[test]
fn a_read_only_data_directory_is_exit_1_and_writes_nothing() {
    let server = serve(Vec::new());
    let home = Home::new(Some(&server.base_url));
    let made = std::process::Command::new("chmod")
        .args(["-R", "a-w"])
        .arg(&home.data)
        .status()
        .expect("chmod 跑得了");
    assert!(made.success());
    let out = home.run(&["ask", "--format", "json", "hi"], Input::Null);
    assert_eq!(out.code, Some(1), "{out:?}");
    let value = json(out.stdout.trim());
    assert_eq!(value["exit"], 1);
    assert!(
        out.stderr.starts_with("litemiyu: "),
        "一句话：{}",
        out.stderr
    );
    assert!(server.requests().is_empty(), "什么都没发");
}

/// 每次请求都回 `reply`（几份，够内核重试）。
fn always(reply: &Reply) -> Vec<Reply> {
    vec![reply.clone(); 8]
}

#[test]
fn unauthorized_is_an_auth_error_and_exit_1() {
    let server = serve(always(&Reply::error(
        401,
        &[("Content-Type", "application/json")],
        r#"{"error":{"message":"Invalid API key"}}"#,
    )));
    let home = Home::new(Some(&server.base_url));
    let out = home.run(&["ask", "--format", "json", "hi"], Input::Null);
    assert_eq!(out.code, Some(1), "{out:?}");
    let value = json(out.stdout.trim());
    assert_eq!(value["error"]["class"], "auth", "{value}");
    assert_eq!(value["reason"], "error");
    let out = home.run(&["ask", "hi"], Input::Null);
    assert_eq!(out.code, Some(1));
    assert!(out.stderr.contains("认证失败"), "{}", out.stderr);
    assert!(!out.stderr.contains("sk-test"), "不印 key：{}", out.stderr);
}

#[test]
fn a_server_error_and_a_cut_stream_end_in_an_error_with_exit_1() {
    let server = serve(always(&Reply::error(500, &[], "boom")));
    let home = Home::new(Some(&server.base_url));
    let out = home.run(&["ask", "--format", "json", "hi"], Input::Null);
    assert_eq!(out.code, Some(1), "{out:?}");
    let value = json(out.stdout.trim());
    assert!(value["error"]["class"].is_string(), "{value}");
    let half = Reply::stream(vec![
        Piece::Bytes(br#"data: {"choices":[{"index":0,"delta":{"content":"half"#.to_vec()),
        Piece::Drop,
    ]);
    let server = serve(always(&half));
    let home = Home::new(Some(&server.base_url));
    let out = home.run(&["ask", "--format", "jsonl", "hi"], Input::Null);
    assert_eq!(out.code, Some(1), "{out:?}");
    let ended = out
        .stdout
        .lines()
        .map(json)
        .find(|e| e["type"] == "turn.ended")
        .expect("那一轮结束了");
    assert_eq!(ended["reason"], "error");
    assert!(ended["error"]["class"].is_string(), "{ended}");
}
