//! L3 四做完的样子（执行计划第六节 L3 四）：会话经真的路由向本机的假服务器发请求，模型一次调真的 `read` 和 `shell`，两件都在
//! 临时目录里跑，结果发回去，模型再说一句，回合结束。工具目录是基础系统的真目录。

use lm_engine::http::testkit::{Piece, Reply, Server};
use lm_kernel::kernel::event::{Body, ToolStatus};

use crate::support::routing::{hellos, served};
use crate::support::*;

/// 照 OpenAI Chat 的流式写法一次调两件：`read` 读 `notes.txt`，`shell` 跑 `echo from-shell`。
fn calls_read_and_shell() -> Reply {
    let events = [
        r#"{"choices":[{"index":0,"delta":{"role":"assistant","tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":"read","arguments":"{\"file_path\":\"notes.txt\"}"}}]},"finish_reason":null}]}"#,
        r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":1,"id":"call_2","type":"function","function":{"name":"shell","arguments":"{\"command\":\"echo from-shell\",\"description\":\"Echo\"}"}}]},"finish_reason":null}]}"#,
        r#"{"choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}"#,
    ];
    let mut text: String = events
        .iter()
        .map(|event| format!("data: {event}\n\n"))
        .collect();
    text.push_str("data: [DONE]\n\n");
    Reply::stream(vec![Piece::Bytes(text.into_bytes())])
}

#[tokio::test]
async fn the_model_runs_the_real_read_and_shell_through_the_real_route() {
    let mut replies = vec![calls_read_and_shell()];
    // 第二份是看了结果说的那一句。
    replies.extend(hellos(1));
    let server = Server::start(replies).await;
    let (routes, configs) = served(&server.base_url, serde_json::json!({}), "");
    let mut home = Home::new();
    home.configs = configs;
    let work = home.scratch.0.join("work");
    std::fs::create_dir_all(&work).expect("建得了工作区");
    std::fs::write(work.join("notes.txt"), "hello from a file\n").expect("写得进");
    let tools = lm_engine::basesystem::catalog().expect("编进程序的字读得出来");
    let opening = Opening {
        cwd: work.to_string_lossy().into_owned(),
        ..Opening::default()
    };
    let handle = home.create_as(&routes, &tools, opening).await;
    let mut pushes = watch(&handle).await;
    ask(&handle, "cmd-1", say("看看笔记，再跑个命令"))
        .await
        .expect("会话在跑");
    until_turn_ends(&mut pushes).await;
    let id = handle.id().clone();
    stop(&handle).await;

    let log = home.log(&id);
    let results: Vec<_> = log
        .iter()
        .filter_map(|event| match &event.body {
            Body::ToolResult(result) => Some(result),
            _ => None,
        })
        .collect();
    assert_eq!(results.len(), 2, "{:?}", kinds(&log));
    assert!(
        results.iter().all(|result| result.status == ToolStatus::Ok),
        "{results:?}"
    );
    // 两件的结果都回到了模型：第二次请求里有它们的回话。
    let received = server.received();
    assert!(received.len() >= 2, "主请求发了两次：{}", received.len());
    let second: serde_json::Value =
        serde_json::from_slice(&received[1].body).expect("请求体是 JSON");
    let messages = second["messages"].as_array().expect("有消息");
    let answer = |call: &str| {
        messages
            .iter()
            .find(|message| message["role"] == "tool" && message["tool_call_id"] == call)
            .map(|message| message["content"].to_string())
            .unwrap_or_default()
    };
    assert!(
        answer("call_1").contains("hello from a file"),
        "{messages:?}"
    );
    assert!(answer("call_2").contains("from-shell"), "{messages:?}");
    let seen = kinds(&log);
    assert_eq!(seen.last(), Some(&"turn.ended"), "{seen:?}");
}
