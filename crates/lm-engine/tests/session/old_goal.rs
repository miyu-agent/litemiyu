//! 旧日志里的长期目标（2026-11-14 砍了 `goal`，`docs/00-设计.md` 第九节最后一条）：日志里的 `goal.*` 事件读成不认识的种类，
//! 快照工具面上的 `goal` 这次的工具目录里没有，她调过 `goal` 的那一步和结果照旧在。载入不出错，别的消息照旧在，下一次请求
//! 组装得出来，`goal.*` 那几条原样留在日志里。

use std::sync::Arc;

use lm_engine::session::testkit::{Play, Script};
use lm_engine::store::log::{SEGMENT_LIMIT, SessionLog};
use lm_engine::tool::testkit::{Act, Fake};
use lm_engine::tool::{Catalog, Tool};
use lm_kernel::kernel::block::Block;
use lm_kernel::kernel::event::{Body, Event};
use lm_kernel::kernel::request::Message;
use lm_kernel::kernel::tool::Access;

use crate::support::*;

/// 那时候的 `goal` 工具：这里拿假工具顶着，日志里留下调用和结果。
fn then() -> Catalog {
    Catalog::new([Fake::new("goal", Access::Read, Act::Echo) as Arc<dyn Tool>]).expect("合写法")
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
async fn an_old_log_with_goal_events_and_calls_loads_and_goes_on() {
    let home = Home::new();
    let models = Script::new([
        Play::calls(&[("goal", r#"{"action":"get"}"#)]),
        Play::Says("没有目标。"),
    ]);
    let handle = home.create_with(&models, &then()).await;
    let mut pushes = watch(&handle).await;
    ask(&handle, "m1", say("看看目标")).await.expect("会话在跑");
    until_turn_ends(&mut pushes).await;
    drop(pushes);
    let session = handle.id().clone();
    stop(&handle).await;

    // 那时候人用 `/goal` 设过、改过、停过、接着跑过、结束过（写法照砍之前事件样本里的那几条）。
    let dir = home.root.session(&session);
    let (mut log, _) = SessionLog::open(&dir, SEGMENT_LIMIT).expect("打得开");
    let next = log.next_seq().get();
    let line = |seq: u64, kind: &str, body: &str| {
        Event::from_line(&format!(
            r#"{{"seq":{seq},"at":"2026-09-25T22:05:00.000Z","kind":"{kind}","by":{{"kind":"person"}},"cause":"cmd-9f{seq}","body":{body}}}"#
        ))
        .expect("读得懂")
    };
    log.append(&[
        line(next, "goal.set", r#"{"text":"把测试修绿","limit":30}"#),
        line(
            next + 1,
            "goal.changed",
            r#"{"text":"把测试修绿，再写 CHANGES.md"}"#,
        ),
        line(next + 2, "goal.paused", "{}"),
        line(next + 3, "goal.resumed", "{}"),
        line(
            next + 4,
            "goal.ended",
            r#"{"outcome":"blocked","reason":"要连外网"}"#,
        ),
    ])
    .expect("写得进");
    drop(log);

    let again = Script::new([Play::Says("接着来。")]);
    let loaded = home.load(&session, &again).await;
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
        at("看看目标"),
        at("call goal"),
        at(r#"args={"action":"get"}"#),
        at("没有目标。"),
        at("接着干"),
    ];
    assert!(order.is_sorted(), "照先后：{order:?} {seen:?}");
    assert!(
        !seen.iter().any(|text| text.contains("把测试修绿")),
        "目标的事件不进她的上下文：{seen:?}"
    );

    let log = home.log(&session);
    let old: Vec<&str> = log
        .iter()
        .filter(|event| matches!(event.body, Body::Unknown { .. }))
        .map(|event| event.body.kind())
        .collect();
    assert_eq!(
        old,
        [
            "goal.set",
            "goal.changed",
            "goal.paused",
            "goal.resumed",
            "goal.ended"
        ],
        "旧的几条读成不认识的种类，原样留着"
    );
    let ended = log
        .iter()
        .filter(|event| matches!(event.body, Body::TurnEnded(_)))
        .count();
    assert_eq!(ended, 2, "载入前一轮、载入后一轮");
}
