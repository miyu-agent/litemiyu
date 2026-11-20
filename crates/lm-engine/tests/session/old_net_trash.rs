//! 旧日志里的 `web_search`、`web_fetch`、`trash`（LM-2 砍了这三件）：这次的工具目录里没有它们，她调过的
//! 那几步和结果照旧在；`trash` 的结果带的 `file.trashed` 照旧读得懂（内核留着这种效果的读法）。载入不出错，别的消息照旧在，
//! 下一次请求组装得出来；旧会话的快照里还登记着它们，她照样会调：说现在用不了，会话接着走。

use std::sync::Arc;

use lm_engine::session::testkit::{Play, Script};
use lm_engine::tool::testkit::{Act, Fake};
use lm_engine::tool::{Catalog, Tool};
use lm_kernel::kernel::block::Block;
use lm_kernel::kernel::event::{Body, Effect, Event, ToolStatus};
use lm_kernel::kernel::request::Message;
use lm_kernel::kernel::tool::Access;

use crate::support::*;

/// 那时候 `trash` 报的效果（写法照内核的 `file.trashed`）。
const TRASHED: &str = r#"{"kind":"file.trashed","path":"/home/me/work/old.txt","trash":"/home/me/.local/share/Trash/files/old.txt"}"#;

/// 那时候的三件：这里拿假工具顶着，日志里留下调用和结果。
fn then() -> Catalog {
    Catalog::new([
        Fake::new("web_search", Access::Read, Act::Echo) as Arc<dyn Tool>,
        Fake::new("web_fetch", Access::Read, Act::Echo),
        Fake::new("trash", Access::Read, Act::Echo),
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
async fn an_old_log_with_web_and_trash_calls_loads_and_goes_on() {
    let home = Home::new();
    let models = Script::new([
        Play::calls(&[
            ("web_search", r#"{"query":"rust"}"#),
            ("web_fetch", r#"{"url":"https://example.com/"}"#),
            ("trash", r#"{"target":"old.txt"}"#),
        ]),
        Play::Says("查完了，旧文件也删了。"),
    ]);
    let handle = home.create_with(&models, &then()).await;
    let mut pushes = watch(&handle).await;
    ask(&handle, "m1", say("查一下，顺手删掉 old.txt"))
        .await
        .expect("会话在跑");
    until_turn_ends(&mut pushes).await;
    drop(pushes);
    let session = handle.id().clone();
    stop(&handle).await;

    // 那时候 `trash` 的结果带着 `file.trashed`：照那时的写法补进日志里那条结果。
    let segment = home.root.session(&session).join("000000000001.jsonl");
    let text = std::fs::read_to_string(&segment).expect("读得了第一段");
    let mut trashed = 0;
    let old: String = text
        .lines()
        .map(|line| {
            let mut event = Event::from_line(line).expect("每一行都是事件");
            if let Body::ToolResult(result) = &mut event.body
                && result.blocks.iter().any(
                    |block| matches!(block, Block::Text(text) if text.text.contains("old.txt")),
                )
            {
                result
                    .effects
                    .push(serde_json::from_str(TRASHED).expect("读得懂"));
                trashed += 1;
            }
            event.to_line() + "\n"
        })
        .collect();
    assert_eq!(trashed, 1, "那一轮调了一次 trash");
    std::fs::write(&segment, old).expect("写得回去");

    let again = Script::new([
        Play::calls(&[("web_search", r#"{"query":"again"}"#)]),
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
        at("查一下"),
        at("call web_search"),
        at("call web_fetch"),
        at("call trash"),
        // 结果：那时候的假工具照原样回了参数。
        at(r#"{"query":"rust"}"#),
        at("https://example.com/"),
        at(r#"{"target":"old.txt"}"#),
        at("查完了"),
        at("接着干"),
    ];
    assert!(order.is_sorted(), "照先后：{order:?} {seen:?}");

    let log = home.log(&session);
    let kept: Vec<&Effect> = log
        .iter()
        .filter_map(|event| match &event.body {
            Body::ToolResult(result) => Some(&result.effects),
            _ => None,
        })
        .flatten()
        .collect();
    assert!(
        matches!(kept.as_slice(), [Effect::FileTrashed(trashed)] if trashed.path == "/home/me/work/old.txt"),
        "旧的 file.trashed 照旧读得懂：{kept:?}"
    );
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
    assert_eq!(
        said,
        ["The tool \"web_search\" is not available right now.\n"]
    );
    let ended = log
        .iter()
        .filter(|event| matches!(event.body, Body::TurnEnded(_)))
        .count();
    assert_eq!(ended, 2, "载入前一轮、载入后一轮");
}
