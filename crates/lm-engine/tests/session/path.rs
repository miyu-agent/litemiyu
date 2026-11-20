//! L3 的目标那条路（执行计划第六节 L3 二下）：照剧本回话的假模型在 `lm-engine` 里跑完一个回合，落盘以后载得回；带一次工具
//! 调用的也一样。L3 三起另有一条走真的路由（`lm_engine::session::Routes`）：会话向本机的假服务器发请求、收流式回复，模型调一件
//! 工具，结果发回去，模型再说一句，回合结束。

use std::sync::Arc;

use lm_engine::session::testkit::{Play, Script};
use lm_engine::tool::testkit::{Act, Fake};
use lm_engine::tool::{Catalog, Tool};
use lm_kernel::kernel::block::{Block, Text};
use lm_kernel::kernel::event::{Body, ToolStatus};
use lm_kernel::kernel::request::Message;
use lm_kernel::kernel::session::Outcome;
use lm_kernel::kernel::tool::Access;

use lm_engine::http::testkit::{Piece, Reply, Server};

use crate::support::routing::{hellos, served};
use crate::support::*;

#[tokio::test]
async fn one_turn_is_stored_and_loads_back() {
    let home = Home::new();
    let script = Script::new([Play::Says("你好。")]);
    let handle = home.create(&script).await;
    let mut pushes = watch(&handle).await;
    let outcome = ask(&handle, "cmd-1", say("hi")).await.expect("会话在跑");
    assert!(matches!(outcome, Outcome::Accepted { .. }), "{outcome:?}");
    until_turn_ends(&mut pushes).await;
    let id = handle.id().clone();
    stop(&handle).await;

    let before = home.log(&id);
    let kinds_before = kinds(&before);
    for kind in [
        "session.created",
        "message.user",
        "turn.started",
        "message.assistant",
        "turn.ended",
    ] {
        assert!(
            kinds_before.contains(&kind),
            "少了 {kind}：{kinds_before:?}"
        );
    }
    assert_eq!(script.requests().len(), 1);

    // 载入：照日志重建，磁盘上的一条不少；再说一句接着跑。
    let again = Script::new([Play::Says("又见面了。")]);
    let loaded = home.load(&id, &again).await;
    let after = home.log(&id);
    assert_eq!(after[..before.len()], before[..]);
    let mut pushes = watch(&loaded).await;
    ask(&loaded, "cmd-2", say("还在吗"))
        .await
        .expect("会话在跑");
    until_turn_ends(&mut pushes).await;
    stop(&loaded).await;
    let requests = again.requests();
    assert_eq!(requests.len(), 1);
    // 载回来的历史进了下一次请求：上一轮的问和答都在。
    let sent = format!("{:?}", requests[0].1.messages);
    assert!(sent.contains("hi") && sent.contains("你好。"), "{sent}");
}

#[tokio::test]
async fn a_tool_call_runs_and_the_turn_ends() {
    let home = Home::new();
    let echo = Fake::new("echo", Access::Read, Act::Echo);
    let tools = Catalog::new([Arc::clone(&echo) as Arc<dyn Tool>]).expect("合写法");
    let script = Script::new([Play::calls(&[("echo", r#"{"x":1}"#)]), Play::Says("好了。")]);
    let handle = home.create_with(&script, &tools).await;
    let mut pushes = watch(&handle).await;
    ask(&handle, "cmd-1", say("调一下"))
        .await
        .expect("会话在跑");
    until_turn_ends(&mut pushes).await;
    let id = handle.id().clone();
    stop(&handle).await;

    assert_eq!(echo.calls().len(), 1, "工具跑了一次");
    let log = home.log(&id);
    let results: Vec<_> = log
        .iter()
        .filter_map(|event| match &event.body {
            Body::ToolResult(result) => Some(result),
            _ => None,
        })
        .collect();
    assert_eq!(results.len(), 1, "{:?}", kinds(&log));
    assert_eq!(results[0].status, ToolStatus::Ok);
    // 结果回到了模型：第二次请求里有工具的回话。
    let requests = script.requests();
    assert_eq!(requests.len(), 2);
    let answered = requests[1].1.messages.iter().any(|message| {
        matches!(message, Message::Tool { error: false, blocks, .. }
            if blocks == &[Block::Text(Text { text: r#"cwd=~/src/miyu args={"x":1}"#.to_string() })])
    });
    assert!(answered, "{:?}", requests[1].1.messages);
    // 模型调完工具又说了一句，回合才结束。
    let seen = kinds(&log);
    assert_eq!(seen.last(), Some(&"turn.ended"));
    let said = seen
        .iter()
        .filter(|kind| **kind == "message.assistant")
        .count();
    assert_eq!(said, 2, "{seen:?}");

    // 落盘的载得回。
    let loaded = home.load_with(&id, &Script::new([]), &tools).await;
    assert_eq!(home.log(&id)[..log.len()], log[..]);
    stop(&loaded).await;
}

/// 照 OpenAI Chat 的流式写法调一次 `echo`，参数 `{"x":1}`，分两段到。
fn calls_echo() -> Reply {
    let events = [
        r#"{"choices":[{"index":0,"delta":{"role":"assistant","tool_calls":[{"index":0,"id":"call_1","type":"function","function":{"name":"echo","arguments":"{\"x\""}}]},"finish_reason":null}]}"#,
        r#"{"choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":":1}"}}]},"finish_reason":null}]}"#,
        r#"{"choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}"#,
    ];
    let mut text: String = events
        .iter()
        .map(|event| format!("data: {event}\n\n"))
        .collect();
    text.push_str("data: [DONE]\n\n");
    Reply::stream(vec![Piece::Bytes(text.into_bytes())])
}

/// 走真的路由：请求发到假服务器，回来的流照驱动解出一次工具调用，跑完把结果发回去，第二次请求说完回合结束；落盘以后载得回。
#[tokio::test]
async fn a_tool_call_goes_through_the_real_route() {
    let mut replies = vec![calls_echo()];
    replies.extend(hellos(1));
    let server = Server::start(replies).await;
    let (routes, configs) = served(&server.base_url, serde_json::json!({}), "");
    let mut home = Home::new();
    home.configs = configs;
    let echo = Fake::new("echo", Access::Read, Act::Echo);
    let tools = Catalog::new([Arc::clone(&echo) as Arc<dyn Tool>]).expect("合写法");
    let handle = home.create_with(&routes, &tools).await;
    let mut pushes = watch(&handle).await;
    ask(&handle, "cmd-1", say("调一下"))
        .await
        .expect("会话在跑");
    until_turn_ends(&mut pushes).await;
    let id = handle.id().clone();
    stop(&handle).await;

    assert_eq!(echo.calls().len(), 1, "工具跑了一次");
    let received = server.received();
    assert!(received.len() >= 2, "主请求发了两次：{}", received.len());
    assert!(
        received[0].path.ends_with("/chat/completions"),
        "{}",
        received[0].path
    );
    assert_eq!(received[0].header("authorization"), Some("Bearer sk-test"));
    let second: serde_json::Value =
        serde_json::from_slice(&received[1].body).expect("请求体是 JSON");
    let messages = second["messages"].as_array().expect("有消息");
    let answered = messages.iter().any(|message| {
        message["role"] == "tool"
            && message["tool_call_id"] == "call_1"
            && message["content"].to_string().contains(r#"args={\"x\":1}"#)
    });
    assert!(answered, "{messages:?}");

    let log = home.log(&id);
    let seen = kinds(&log);
    assert!(seen.contains(&"turn.ended"), "{seen:?}");
    let called: Vec<_> = log
        .iter()
        .filter_map(|event| match &event.body {
            Body::ModelCalled(called) if called.purpose.is_none() => Some(called),
            _ => None,
        })
        .collect();
    assert_eq!(called.len(), 2, "主请求两次：{seen:?}");
    assert!(
        called
            .iter()
            .all(|call| call.endpoint.as_ref().map(|e| e.as_str()) == Some("deepseek")),
        "{called:?}"
    );
    let results = log
        .iter()
        .filter(|event| matches!(&event.body, Body::ToolResult(result) if result.status == ToolStatus::Ok))
        .count();
    assert_eq!(results, 1, "{seen:?}");

    let loaded = home.load_with(&id, &routes, &tools).await;
    assert_eq!(home.log(&id)[..log.len()], log[..]);
    stop(&loaded).await;
}
