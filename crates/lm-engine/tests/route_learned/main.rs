//! 用出来的窗口（施工 8-7，`docs/blueprint/models.md`「怎么走」第二条第 9 条）：请求报上下文超长、说了上限、比手头的
//! 窗口小的，记进 `state/models/learned.json`，新造的会话用上；比手头的大的不记。（原来还查运行日志
//! `INFO learned window` 一行：LM-1 #058 去掉了没人订阅的运行日志，那一半删了。）

#[path = "../session/support/mod.rs"]
mod support;

use std::sync::Arc;
use std::time::Duration;

use lm_engine::http::testkit::{Piece, Reply, Server};
use lm_engine::session::{ModelData, Observed, read_observed};
use lm_kernel::models::matching::Vendors;
use lm_kernel::models::observed::Learned;
use lm_kernel::models::profile::Profiles;
use support::routing::{configs, routes_with};
use support::{Home, ask, say, until_turn_ends, watch};

/// 超长的 400：上限 `limit`。
fn too_long(limit: u64) -> Reply {
    let body = format!(
        r#"{{"error":{{"message":"This model's maximum context length is {limit} tokens. However, you requested {} tokens.","code":"context_length_exceeded"}}}}"#,
        limit + 5000
    );
    Reply::error(400, &[], &body)
}

/// 说「你好！」。
fn hello() -> Reply {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/designs/samples/drivers/openai-chat/streams/openai-text.sse");
    Reply::stream(vec![Piece::Bytes(
        std::fs::read(&path).expect("样本读得到"),
    )])
}

/// 一家 `dev` 在 `base_url`，模型 `m`，手写的窗口另写 `window` 那一行。
fn config(base_url: &str, window: &str) -> String {
    format!(
        "[providers.dev]\ndriver = \"openai-chat\"\nbase_url = \"{base_url}\"\n\n[providers.dev.models.m]\n{window}\n[models]\nchat = \"dev/m\"\n"
    )
}

/// 说一句、等这一轮了结（可能以出错了结），交回新造的会话。
async fn overflow(home: &mut Home, data: &Arc<ModelData>, limit: u64, window: &str) {
    let mut replies = vec![too_long(limit)];
    replies.extend((0..6).map(|_| hello()));
    let server = Server::start(replies).await;
    home.configs = configs(&config(&server.base_url, window), &[]);
    let routes = routes_with(Arc::clone(data), Duration::from_secs(60));
    let handle = home.create(&routes).await;
    let mut pushes = watch(&handle).await;
    ask(&handle, "cmd-1", say("hi")).await.expect("会话在跑");
    tokio::time::timeout(Duration::from_secs(20), until_turn_ends(&mut pushes))
        .await
        .expect("这一轮了结");
}

#[tokio::test]
async fn a_stated_limit_is_learned_and_used_by_the_next_session() {
    let mut home = Home::new();
    let dir = home.root.model_state();
    let data = Arc::new(ModelData::new(
        Profiles::default(),
        Vendors::default(),
        Some(lm_engine::session::ModelDirs::in_one(dir.clone())),
    ));
    data.loaded(None, Observed::default());
    overflow(&mut home, &data, 65_536, "").await;
    let learned = Learned::parse(&std::fs::read_to_string(dir.join("learned.json")).expect("写了"))
        .expect("读得进");
    assert_eq!(
        learned.window("dev", "m").map(|stamped| stamped.value),
        Some(65_536)
    );
    assert_eq!(
        read_observed(&lm_engine::session::ModelDirs::in_one(dir.clone())).learned,
        learned,
        "核心下次起来读回同一份"
    );
    // 新造的会话用上。
    let routes = routes_with(Arc::clone(&data), Duration::from_secs(60));
    assert_eq!(home.create(&routes).await.limits().window, Some(65_536));
    // 再报一个更大的上限：不记。
    overflow(&mut home, &data, 100_000, "").await;
    assert_eq!(
        data.with(|knowledge| knowledge
            .learned
            .window("dev", "m")
            .map(|stamped| stamped.value)),
        Some(65_536)
    );
    // 手写的窗口比报的小：不记；手写的盖过用出来的。
    overflow(&mut home, &data, 60_000, "window = 50000").await;
    let routes = routes_with(Arc::clone(&data), Duration::from_secs(60));
    assert_eq!(
        data.with(|knowledge| knowledge
            .learned
            .window("dev", "m")
            .map(|stamped| stamped.value)),
        Some(65_536)
    );
    assert_eq!(home.create(&routes).await.limits().window, Some(50_000));
}

/// 流式回一段话 `text`（OpenAI 兼容的 chat completions）。
fn says(text: &str) -> Reply {
    let chunk = |delta: serde_json::Value, finish: Option<&str>| {
        let chunk = serde_json::json!({
            "id": "c1",
            "object": "chat.completion.chunk",
            "created": 1_790_000_000,
            "model": "m",
            "choices": [{"index": 0, "delta": delta, "finish_reason": finish}],
        });
        format!("data: {chunk}\n\n")
    };
    let body = chunk(
        serde_json::json!({"role": "assistant", "content": text}),
        None,
    ) + &chunk(serde_json::json!({}), Some("stop"))
        + "data: [DONE]\n\n";
    Reply::stream(vec![Piece::Bytes(body.into_bytes())])
}

/// 不知道窗口的模型（LM-9，照 Miyu：没有目录、配置没写、也没学到时不设默认值、不主动压缩）：说了两轮以后供应商回「上下文
/// 超长」（没说上限，学不到窗口），会话先压一次（摘要请求），再把这一步重发，这一轮照常说完。窗口未知时压缩留的尾巴是策略的
/// `tail`（16000 token，`lm_kernel::policy::snapshot::TAIL`）：一轮说约四万字节（约一万 token），第一轮落在尾巴外面、压得掉；
/// 比尾巴还短的历史报超长没有能压的，照出错结束（同窗口已知时）。
#[tokio::test]
async fn an_unknown_window_compacts_after_the_provider_says_too_long_and_sends_again() {
    let mut home = Home::new();
    let dir = home.root.model_state();
    let data = Arc::new(ModelData::new(
        Profiles::default(),
        Vendors::default(),
        Some(lm_engine::session::ModelDirs::in_one(dir.clone())),
    ));
    data.loaded(None, Observed::default());
    let too_long = Reply::error(
        400,
        &[],
        r#"{"error":{"message":"prompt is too long","type":"invalid_request_error","code":"context_length_exceeded"}}"#,
    );
    let server = Server::start(vec![
        says("一。"),
        says("二。"),
        too_long,
        says("<summary>用户说了两轮。</summary>"),
        says("三。"),
    ])
    .await;
    home.configs = configs(&config(&server.base_url, ""), &[]);
    let routes = routes_with(Arc::clone(&data), Duration::from_secs(60));
    let handle = home.create(&routes).await;
    assert_eq!(handle.limits().window, None, "窗口未知：不设默认值");
    for n in 1..=3 {
        let mut pushes = watch(&handle).await;
        let text = format!("第{n}轮。{}", "lorem ipsum dolor sit amet, ".repeat(1500));
        ask(&handle, &format!("cmd-{n}"), say(&text))
            .await
            .expect("会话在跑");
        tokio::time::timeout(Duration::from_secs(20), until_turn_ends(&mut pushes))
            .await
            .expect("这一轮了结");
    }
    let log = home.log(handle.id());
    let compacted: Vec<_> = log
        .iter()
        .filter_map(|event| match &event.body {
            lm_kernel::kernel::event::Body::ContextCompacted(compacted) => {
                Some(compacted.trigger.clone())
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        compacted,
        [Some(lm_kernel::kernel::event::CompactTrigger::Overflow)],
        "被动压了一次"
    );
    let ended: Vec<_> = log
        .iter()
        .filter_map(|event| match &event.body {
            lm_kernel::kernel::event::Body::TurnEnded(ended) => Some(ended.reason.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        ended,
        vec![lm_kernel::kernel::event::EndReason::Completed; 3],
        "三轮都照常说完，第三轮没有照出错结束"
    );
    let received = server.received();
    assert_eq!(received.len(), 5, "两轮、超长、摘要、重发");
    let body = |at: usize| String::from_utf8_lossy(&received[at].body).into_owned();
    assert!(
        body(3).contains("Respond with text only."),
        "第四次是摘要请求"
    );
    assert!(body(4).contains("用户说了两轮。"), "重发的带着摘要");
    assert!(!body(4).contains("第1轮"), "第一轮压掉了");
    assert!(
        data.with(|knowledge| knowledge.learned.window("dev", "m"))
            .is_none(),
        "没说上限：学不到窗口，照旧未知"
    );
}

/// 同上，只是历史比压缩的尾巴（16000 token）短：报超长时没有能压的，不发摘要请求，这一轮照出错结束（同 Miyu）。
#[tokio::test]
async fn an_unknown_window_with_a_short_history_ends_the_turn_with_the_error() {
    let mut home = Home::new();
    let data = Arc::new(ModelData::new(
        Profiles::default(),
        Vendors::default(),
        None,
    ));
    data.loaded(None, Observed::default());
    let too_long = Reply::error(
        400,
        &[],
        r#"{"error":{"message":"prompt is too long","code":"context_length_exceeded"}}"#,
    );
    let server = Server::start(vec![says("一。"), too_long, says("不该发到这里。")]).await;
    home.configs = configs(&config(&server.base_url, ""), &[]);
    let routes = routes_with(Arc::clone(&data), Duration::from_secs(60));
    let handle = home.create(&routes).await;
    for n in 1..=2 {
        let mut pushes = watch(&handle).await;
        ask(&handle, &format!("cmd-{n}"), say("你好"))
            .await
            .expect("会话在跑");
        tokio::time::timeout(Duration::from_secs(20), until_turn_ends(&mut pushes))
            .await
            .expect("这一轮了结");
    }
    let ended: Vec<_> = home
        .log(handle.id())
        .iter()
        .filter_map(|event| match &event.body {
            lm_kernel::kernel::event::Body::TurnEnded(ended) => Some(ended.reason.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        ended,
        [
            lm_kernel::kernel::event::EndReason::Completed,
            lm_kernel::kernel::event::EndReason::Error
        ]
    );
    assert_eq!(server.received().len(), 2, "没有摘要请求、没有重发");
}
