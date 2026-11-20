//! 旧日志里的 `glob`、`grep`（2026-11-14 砍了这两件，`docs/00-设计.md` 第九节最后一条）：这次的工具目录里没有它们，她调过的
//! 那几步和结果照旧在。载入不出错，别的消息照旧在，下一次请求组装得出来。旧会话的快照里还登记着它们，她照样会调：
//! 说现在用不了（同 `tools.rs` 的 `a_tool_the_catalog_no_longer_has_is_not_available`），会话接着走。

use std::sync::Arc;

use lm_engine::session::testkit::{Play, Script};
use lm_engine::tool::testkit::{Act, Fake};
use lm_engine::tool::{Catalog, Tool};
use lm_kernel::kernel::block::Block;
use lm_kernel::kernel::event::{Body, ToolStatus};
use lm_kernel::kernel::request::Message;
use lm_kernel::kernel::tool::Access;

use crate::support::*;

/// 那时候的 `glob`、`grep`：这里拿假工具顶着，日志里留下调用和结果。
fn then() -> Catalog {
    Catalog::new([
        Fake::new("glob", Access::Read, Act::Echo) as Arc<dyn Tool>,
        Fake::new("grep", Access::Read, Act::Echo),
    ])
    .expect("合写法")
}

/// 一次请求里的字：人说的、她说的、工具结果、工具调用的名字，照先后。
fn texts(messages: &[Message]) -> Vec<String> {
    let block = |block: &Block| match block {
        Block::Text(text) => Some(text.text.clone()),
        Block::ToolCall(call) => Some(format!("call {}", call.name)),
        _ => None,
    };
    messages
        .iter()
        .flat_map(|message| match message {
            Message::User { blocks } | Message::Assistant { blocks } => {
                blocks.iter().filter_map(block).collect::<Vec<_>>()
            }
            Message::Tool { blocks, .. } => blocks.iter().filter_map(block).collect(),
        })
        .collect()
}

#[tokio::test]
async fn an_old_log_with_glob_and_grep_calls_loads_and_goes_on() {
    let home = Home::new();
    let models = Script::new([
        Play::calls(&[
            ("glob", r#"{"pattern":"**/*.rs"}"#),
            ("grep", r#"{"pattern":"TODO"}"#),
        ]),
        Play::Says("找到了。"),
    ]);
    let handle = home.create_with(&models, &then()).await;
    let mut pushes = watch(&handle).await;
    ask(&handle, "m1", say("找找 TODO"))
        .await
        .expect("会话在跑");
    until_turn_ends(&mut pushes).await;
    drop(pushes);
    let session = handle.id().clone();
    stop(&handle).await;
    let results = home
        .log(&session)
        .iter()
        .filter(|event| matches!(event.body, Body::ToolResult(_)))
        .count();
    assert_eq!(results, 2, "那一轮调了 glob、grep 各一次");

    let again = Script::new([
        Play::calls(&[("grep", r#"{"pattern":"FIXME"}"#)]),
        Play::Says("接着来。"),
    ]);
    let tools = lm_engine::basesystem::catalog().expect("编进程序的字读得出来");
    let loaded = home.load_with(&session, &again, &tools).await;
    let mut pushes = watch(&loaded).await;
    ask(&loaded, "m2", say("接着干"))
        .await
        .expect("载入的会话在跑");
    until_turn_ends(&mut pushes).await;
    drop(pushes);
    stop(&loaded).await;

    let requests = again.requests();
    assert_eq!(requests.len(), 2, "载入以后照常请求：调一次工具、再说一句");
    let seen = texts(&requests[0].1.messages);
    let at = |text: &str| {
        seen.iter()
            .position(|seen| seen.contains(text))
            .unwrap_or_else(|| panic!("请求里没有「{text}」：{seen:?}"))
    };
    let order = [
        at("找找 TODO"),
        at("call glob"),
        at("call grep"),
        // 结果：那时候的假工具照原样回了参数。
        at("**/*.rs"),
        at(r#"{"pattern":"TODO"}"#),
        at("找到了。"),
        at("接着干"),
    ];
    assert!(order.is_sorted(), "照先后：{order:?} {seen:?}");

    let log = home.log(&session);
    let last = log
        .iter()
        .rev()
        .find_map(|event| match &event.body {
            Body::ToolResult(result) => Some(result),
            _ => None,
        })
        .expect("载入以后那一次调用有结果");
    assert_eq!(last.status, ToolStatus::Error);
    let said: Vec<&str> = last
        .blocks
        .iter()
        .filter_map(|block| match block {
            Block::Text(text) => Some(text.text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(said, ["The tool \"grep\" is not available right now.\n"]);
    let ended = log
        .iter()
        .filter(|event| matches!(event.body, Body::TurnEnded(_)))
        .count();
    assert_eq!(ended, 2, "载入前一轮、载入后一轮");
}
