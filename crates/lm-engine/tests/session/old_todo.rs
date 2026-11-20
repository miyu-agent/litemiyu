//! 旧日志里的待办（2026-11-14 砍了 `todowrite`，`docs/00-设计.md` 第九节最后一条）：她调过 `todowrite` 的那一步和结果照旧在，
//! 结果带的 `todo.written` 读成不认识的效果、人清掉待办的 `todos.cleared` 读成不认识的种类，都原样留着。载入不出错；这时
//! 压缩，检查点里代码写的几段和从来没有待办时一样，只有取回指路；以前压缩时写进检查点的待办那一段照原样进请求。

use std::sync::Arc;

use lm_engine::session::testkit::{Play, Script};
use lm_engine::store::log::{SEGMENT_LIMIT, SessionLog};
use lm_engine::tool::testkit::{Act, Fake};
use lm_engine::tool::{Catalog, Tool};
use lm_kernel::kernel::block::Block;
use lm_kernel::kernel::event::{Body, ContextCompacted, Effect, Event};
use lm_kernel::kernel::request::Message;
use lm_kernel::kernel::request::Request;
use lm_kernel::kernel::session::Command;
use lm_kernel::kernel::tool::Access;

use crate::support::*;

/// 那时候 `todowrite` 报的效果（写法照砍之前内核的 `todo.written`）。
const WRITTEN: &str = r#"{"kind":"todo.written","todos":[{"content":"读代码","status":"completed"},{"content":"写测试","status":"in_progress"}]}"#;

/// 那时候压缩写进检查点的待办那一段（`notes-todos.txt` 的头一行，下面一项一行）。
const OLD_TODOS: &str =
    "Todo list before this checkpoint:\n- [completed] 读代码\n- [in_progress] 写测试\n";

/// 那时候的 `todowrite` 工具：这里拿假工具顶着，日志里留下调用和结果。
fn then() -> Catalog {
    Catalog::new([Fake::new("todowrite", Access::Read, Act::Echo) as Arc<dyn Tool>])
        .expect("合写法")
}

/// 请求里的全部字，连成一段：人说的、她说的、工具结果，工具调用写成 `call <名字>`。
fn words(request: &Request) -> String {
    request
        .messages
        .iter()
        .flat_map(|message| match message {
            Message::User { blocks }
            | Message::Assistant { blocks }
            | Message::Tool { blocks, .. } => blocks.iter(),
        })
        .filter_map(|block| match block {
            Block::Text(text) => Some(text.text.clone()),
            Block::ToolCall(call) => Some(format!("call {}", call.name)),
            _ => None,
        })
        .collect()
}

/// 日志里的检查点。
fn compacted(log: &[Event]) -> ContextCompacted {
    log.iter()
        .find_map(|event| match &event.body {
            Body::ContextCompacted(compacted) => Some((**compacted).clone()),
            _ => None,
        })
        .expect("压了")
}

/// 照 `edit` 改写这个会话日志里的每一条，写回原处。
fn rewrite(home: &Home, session: &lm_kernel::kernel::id::SessionId, edit: impl Fn(&mut Event)) {
    let dir = home.root.session(session);
    let mut segments: Vec<_> = std::fs::read_dir(&dir)
        .expect("读得了会话目录")
        .map(|entry| entry.expect("读得了").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "jsonl"))
        .collect();
    segments.sort();
    for segment in segments {
        let text = std::fs::read_to_string(&segment).expect("读得了");
        let new: String = text
            .lines()
            .map(|line| {
                let mut event = Event::from_line(line).expect("每一行都是事件");
                edit(&mut event);
                event.to_line() + "\n"
            })
            .collect();
        std::fs::write(&segment, new).expect("写得回去");
    }
}

#[tokio::test]
async fn an_old_log_with_todos_loads_compacts_and_goes_on() {
    let home = Home::new();
    // 窗口大到不会自动压。第一句说七万个字，一组就超了出厂 16000 token 的尾巴，手动压得掉它。
    let models = Script::new([
        Play::calls(&[(
            "todowrite",
            r#"{"todos":[{"content":"读代码","status":"completed"},{"content":"写测试","status":"in_progress"}]}"#,
        )]),
        Play::Says("记下了。"),
    ])
    .window(1_000_000);
    let handle = home.create_with(&models, &then()).await;
    let mut pushes = watch(&handle).await;
    let first = format!("列个待办{}", "x".repeat(70_000));
    ask(&handle, "m1", say(&first)).await.expect("会话在跑");
    until_turn_ends(&mut pushes).await;
    drop(pushes);
    let session = handle.id().clone();
    stop(&handle).await;

    // 那时候的结果带着 `todo.written`；后来人用 `/todo clear` 清掉过（写法照砍之前事件样本里的那一条）。
    rewrite(&home, &session, |event| {
        if let Body::ToolResult(result) = &mut event.body {
            result
                .effects
                .push(serde_json::from_str(WRITTEN).expect("读得懂"));
        }
    });
    let results = home
        .log(&session)
        .iter()
        .filter(|event| matches!(event.body, Body::ToolResult(_)))
        .count();
    assert_eq!(results, 1, "那一轮调了一次 todowrite");
    let dir = home.root.session(&session);
    let (mut log, _) = SessionLog::open(&dir, SEGMENT_LIMIT).expect("打得开");
    let next = log.next_seq().get();
    log.append(&[Event::from_line(&format!(
        r#"{{"seq":{next},"at":"2026-09-25T22:08:00.000Z","kind":"todos.cleared","by":{{"kind":"person"}},"cause":"cmd-8a21","body":{{}}}}"#
    ))
    .expect("读得懂")])
    .expect("写得进");
    drop(log);

    // 载入、手动压缩：代码写的几段只有取回指路，同从来没有待办时。
    let tools = lm_engine::basesystem::catalog().expect("编进程序的字读得出来");
    let again = Script::new([
        Play::Says("<summary>她列了一份待办。</summary>"),
        Play::Says("接着来。"),
    ])
    .window(1_000_000);
    let loaded = home.load_with(&session, &again, &tools).await;
    let mut pushes = watch(&loaded).await;
    let compact = Command::Compact { instructions: None };
    ask(&loaded, "m2", compact).await.expect("载入的会话在跑");
    until_turn_ends(&mut pushes).await;
    ask(&loaded, "m3", say("接着干"))
        .await
        .expect("载入的会话在跑");
    until_turn_ends(&mut pushes).await;
    drop(pushes);
    stop(&loaded).await;

    let log = home.log(&session);
    let checkpoint = compacted(&log);
    let retrieve = include_str!("../../../../resources/core/compaction/notes-retrieve.txt")
        .replace("{upto}", &checkpoint.upto.to_string());
    assert_eq!(checkpoint.notes, retrieve, "代码写的几段只有取回指路");
    let requests = again.requests();
    assert_eq!(requests.len(), 2, "摘要请求、接着说的一次");
    let summary = words(&requests[0].1);
    assert!(summary.contains("列个待办"), "摘要请求里有第一句");
    let after = words(&requests[1].1);
    // 那一轮调的 `todowrite` 和她的回答：压掉了的在摘要请求里，留作尾巴的照原样在后一次请求里。
    let both = format!("{summary}{after}");
    for text in ["todowrite", "记下了。"] {
        assert!(both.contains(text), "两次请求里都没有「{text}」");
    }
    assert!(after.contains("她列了一份待办。"), "{after}");
    assert!(after.contains(&retrieve), "{after}");
    assert!(!after.contains("Todo list"), "{after}");
    assert!(after.contains("接着干"), "{after}");
    // 旧的几条原样留着：效果读成不认识的，`todos.cleared` 读成不认识的种类。
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
    assert_eq!(kept, [WRITTEN], "旧的效果原样留着");
    let old: Vec<&str> = log
        .iter()
        .filter(|event| matches!(event.body, Body::Unknown { .. }))
        .map(|event| event.body.kind())
        .collect();
    assert_eq!(old, ["todos.cleared"], "旧的那一条原样留着");

    // 以前压缩时写进检查点的待办那一段：照原样进请求。
    rewrite(&home, &session, |event| {
        if let Body::ContextCompacted(compacted) = &mut event.body {
            compacted.notes = format!("{}{OLD_TODOS}", compacted.notes);
        }
    });
    let third = Script::new([Play::Says("还在。")]).window(1_000_000);
    let reloaded = home.load_with(&session, &third, &tools).await;
    let mut pushes = watch(&reloaded).await;
    ask(&reloaded, "m4", say("再看看"))
        .await
        .expect("又载入的会话在跑");
    until_turn_ends(&mut pushes).await;
    drop(pushes);
    stop(&reloaded).await;
    let requests = third.requests();
    assert_eq!(requests.len(), 1, "载入以后照常请求了一次");
    let seen = words(&requests[0].1);
    assert!(seen.contains(&format!("{retrieve}{OLD_TODOS}")), "{seen}");
    assert!(seen.contains("再看看"), "{seen}");
}
