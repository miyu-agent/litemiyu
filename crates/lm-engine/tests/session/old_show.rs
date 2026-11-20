//! 旧日志里的 `show`（2026-11-14 砍了 `show`，`docs/00-设计.md` 第九节最后一条）：这次的工具目录里没有它，她调过 `show`
//! 的那一步和结果照旧在；结果带的 `file.shown` 读成不认识的效果，原样留着。载入不出错，别的消息照旧在，下一次请求组装得出来。

use std::sync::Arc;

use lm_engine::session::testkit::{Play, Script};
use lm_engine::tool::testkit::{Act, Fake};
use lm_engine::tool::{Catalog, Tool};
use lm_kernel::kernel::block::Block;
use lm_kernel::kernel::event::{Body, Effect, Event};
use lm_kernel::kernel::request::Message;
use lm_kernel::kernel::tool::Access;

use crate::support::*;

/// 那时候 `show` 报的效果（写法照砍之前内核的 `file.shown`）。
const SHOWN: &str = r#"{"kind":"file.shown","path":"/home/me/work/pic.png","blob":"sha256:ca978112ca1bbdcafac231b39a23dc4da786eff8147c4e72b9807785afee48bb","name":"pic.png","media_type":"image/png","size":1}"#;

/// 那时候的 `show` 工具：这里拿假工具顶着，日志里留下调用和结果。
fn then() -> Catalog {
    Catalog::new([Fake::new("show", Access::Read, Act::Echo) as Arc<dyn Tool>]).expect("合写法")
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
async fn an_old_log_with_show_calls_loads_and_goes_on() {
    let home = Home::new();
    let models = Script::new([
        Play::calls(&[("show", r#"{"path":"pic.png"}"#)]),
        Play::Says("打开了。"),
    ]);
    let handle = home.create_with(&models, &then()).await;
    let mut pushes = watch(&handle).await;
    ask(&handle, "m1", say("打开图看看"))
        .await
        .expect("会话在跑");
    until_turn_ends(&mut pushes).await;
    drop(pushes);
    let session = handle.id().clone();
    stop(&handle).await;

    // 那时候的结果带着 `file.shown`：照砍之前的写法补进日志里那条结果。
    let segment = home.root.session(&session).join("000000000001.jsonl");
    let text = std::fs::read_to_string(&segment).expect("读得了第一段");
    let mut results = 0;
    let old: String = text
        .lines()
        .map(|line| {
            let mut event = Event::from_line(line).expect("每一行都是事件");
            if let Body::ToolResult(result) = &mut event.body {
                result
                    .effects
                    .push(serde_json::from_str(SHOWN).expect("读得懂"));
                results += 1;
            }
            event.to_line() + "\n"
        })
        .collect();
    assert_eq!(results, 1, "那一轮调了一次 show");
    std::fs::write(&segment, old).expect("写得回去");

    let again = Script::new([Play::Says("接着来。")]);
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
    assert_eq!(requests.len(), 1, "载入以后照常请求了一次");
    let seen = texts(&requests[0].1.messages);
    let at = |text: &str| {
        seen.iter()
            .position(|seen| seen.contains(text))
            .unwrap_or_else(|| panic!("请求里没有「{text}」：{seen:?}"))
    };
    let order = [
        at("打开图看看"),
        at("call show"),
        // 结果：这次的假工具判不了 `pic.png` 在哪，说的那一句带着它。
        at("pic.png"),
        at("打开了。"),
        at("接着干"),
    ];
    assert!(order.is_sorted(), "照先后：{order:?} {seen:?}");

    let log = home.log(&session);
    let kept: Vec<String> = log
        .iter()
        .filter_map(|event| match &event.body {
            Body::ToolResult(result) => Some(&result.effects),
            _ => None,
        })
        .flatten()
        .map(|effect| match effect {
            Effect::Unknown(raw) => raw.get().to_string(),
            other => format!("{other:?}"),
        })
        .collect();
    assert_eq!(kept, [SHOWN], "旧的效果读成不认识的种类，原样留着");
    let ended = log
        .iter()
        .filter(|event| matches!(event.body, Body::TurnEnded(_)))
        .count();
    assert_eq!(ended, 2, "载入前一轮、载入后一轮");
}
