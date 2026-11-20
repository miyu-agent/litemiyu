//! 另两种驱动接在路由后面整条走通（照 `path.rs` 的 `a_tool_call_goes_through_the_real_route`）：假服务器照 Anthropic、
//! OpenAI Responses 各自的流式写法回话，模型调一次 `echo`，结果照这一家的写法发回去，第二次请求说完回合结束；落盘以后载得回。
//! Miyu 里这两家经路由的测试走一次性入口（没搬），逐字节的编码另有 `route/exchange` 的对照守着；这里守的是整条路。

use std::sync::Arc;

use lm_engine::http::testkit::{Piece, Reply, Server};
use lm_engine::session::{Configs, Routes};
use lm_engine::tool::testkit::{Act, Fake};
use lm_engine::tool::{Catalog, Tool};
use lm_kernel::config::secret::Reference;
use lm_kernel::kernel::event::{Body, ToolStatus};
use lm_kernel::kernel::tool::Access;

use crate::support::routing::{configs, routes};
use crate::support::*;

/// 一家 `p` 在假服务器 `base_url`，驱动 `driver`，key 是 `sk-test`；`models.chat` 指着 `p/m`。
fn provider(base_url: &str, driver: &str) -> (Routes, Configs) {
    let source = format!(
        "[providers.p]\ndriver = \"{driver}\"\nbase_url = \"{base_url}\"\nkey = {{ env = \"P_KEY\" }}\n\n[providers.p.models.m]\n\n[models]\nchat = \"p/m\"\n"
    );
    let configs = configs(&source, &[(Reference::Env("P_KEY".to_string()), "sk-test")]);
    let routes = routes(serde_json::json!({}), std::time::Duration::from_secs(60));
    (routes, configs)
}

/// 一串 SSE 事件 `(名字, JSON)` 写成一份流。
fn stream(events: &[(&str, &str)]) -> Reply {
    let text: String = events
        .iter()
        .map(|(name, data)| format!("event: {name}\ndata: {data}\n\n"))
        .collect();
    Reply::stream(vec![Piece::Bytes(text.into_bytes())])
}

/// Anthropic 的流：调一次 `echo`，参数 `{"x":1}` 分两段到。
fn anthropic_calls_echo() -> Reply {
    stream(&[
        (
            "message_start",
            r#"{"type":"message_start","message":{"id":"msg_1","type":"message","role":"assistant","content":[],"usage":{"input_tokens":10,"output_tokens":1}}}"#,
        ),
        (
            "content_block_start",
            r#"{"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_1","name":"echo","input":{}}}"#,
        ),
        (
            "content_block_delta",
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"x\""}}"#,
        ),
        (
            "content_block_delta",
            r#"{"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":":1}"}}"#,
        ),
        (
            "content_block_stop",
            r#"{"type":"content_block_stop","index":0}"#,
        ),
        (
            "message_delta",
            r#"{"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":20}}"#,
        ),
        ("message_stop", r#"{"type":"message_stop"}"#),
    ])
}

/// Responses 的流：调一次 `echo`，参数 `{"x":1}` 分两段到。
fn responses_calls_echo() -> Reply {
    stream(&[
        (
            "response.output_item.added",
            r#"{"type":"response.output_item.added","output_index":0,"item":{"id":"fc_1","type":"function_call","arguments":"","call_id":"call_1","name":"echo"}}"#,
        ),
        (
            "response.function_call_arguments.delta",
            r#"{"type":"response.function_call_arguments.delta","output_index":0,"delta":"{\"x\""}"#,
        ),
        (
            "response.function_call_arguments.delta",
            r#"{"type":"response.function_call_arguments.delta","output_index":0,"delta":":1}"}"#,
        ),
        (
            "response.completed",
            r#"{"type":"response.completed","response":{"status":"completed","usage":{"input_tokens":10,"output_tokens":20}}}"#,
        ),
    ])
}

/// 这一家的样本流 `docs/designs/samples/drivers/<samples>/streams/text.sse`：说「你好！」。
fn says_hello(samples: &str) -> Reply {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!(
        "../../docs/designs/samples/drivers/{samples}/streams/text.sse"
    ));
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("读不了 {}：{e}", path.display()));
    Reply::stream(vec![Piece::Bytes(bytes)])
}

/// 照驱动 `driver` 走一回合：先回 `call`，再回这一家的样本流。
/// 工具跑了一次、两次主请求都发到 `path`、头 `header` 是 `value`，日志里一次成功的工具结果、回合结束，载得回。交回第二次
/// 请求的请求体。
async fn one_tool_turn(
    driver: &str,
    call: Reply,
    path: &str,
    header: (&str, &str),
) -> serde_json::Value {
    let server = Server::start(vec![call, says_hello(driver)]).await;
    let (routes, configs) = provider(&server.base_url, driver);
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
    for request in &received[..2] {
        assert!(request.path.ends_with(path), "{}", request.path);
        assert_eq!(request.header(header.0), Some(header.1));
    }

    let log = home.log(&id);
    let seen = kinds(&log);
    assert!(seen.contains(&"turn.ended"), "{seen:?}");
    let results = log
        .iter()
        .filter(|event| matches!(&event.body, Body::ToolResult(result) if result.status == ToolStatus::Ok))
        .count();
    assert_eq!(results, 1, "{seen:?}");
    let replied = log
        .iter()
        .any(|event| matches!(&event.body, Body::ModelCalled(called) if called.purpose.is_none()));
    assert!(replied, "{seen:?}");

    let loaded = home.load_with(&id, &routes, &tools).await;
    assert_eq!(home.log(&id)[..log.len()], log[..]);
    stop(&loaded).await;

    serde_json::from_slice(&received[1].body).expect("请求体是 JSON")
}

#[tokio::test]
async fn an_anthropic_tool_call_goes_through_the_real_route() {
    let second = one_tool_turn(
        "anthropic",
        anthropic_calls_echo(),
        "/messages",
        ("x-api-key", "sk-test"),
    )
    .await;
    let messages = second["messages"].as_array().expect("有消息");
    let used = messages.iter().any(|message| {
        message["role"] == "assistant"
            && message["content"].as_array().is_some_and(|blocks| {
                blocks.iter().any(|block| {
                    block["type"] == "tool_use"
                        && block["id"] == "toolu_1"
                        && block["input"] == serde_json::json!({"x": 1})
                })
            })
    });
    assert!(used, "{messages:?}");
    let answered = messages.iter().any(|message| {
        message["role"] == "user"
            && message["content"].as_array().is_some_and(|blocks| {
                blocks.iter().any(|block| {
                    block["type"] == "tool_result"
                        && block["tool_use_id"] == "toolu_1"
                        && block["content"].to_string().contains(r#"args={\"x\":1}"#)
                })
            })
    });
    assert!(answered, "{messages:?}");
}

#[tokio::test]
async fn a_responses_tool_call_goes_through_the_real_route() {
    let second = one_tool_turn(
        "openai-responses",
        responses_calls_echo(),
        "/responses",
        ("authorization", "Bearer sk-test"),
    )
    .await;
    let input = second["input"].as_array().expect("有 input");
    let called = input.iter().any(|item| {
        item["type"] == "function_call" && item["call_id"] == "call_1" && item["name"] == "echo"
    });
    assert!(called, "{input:?}");
    let answered = input.iter().any(|item| {
        item["type"] == "function_call_output"
            && item["call_id"] == "call_1"
            && item["output"].to_string().contains(r#"args={\"x\":1}"#)
    });
    assert!(answered, "{input:?}");
}
