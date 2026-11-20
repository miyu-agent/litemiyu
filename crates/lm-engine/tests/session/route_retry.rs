//! 单个模型出错再来（`docs/blueprint/models.md`「怎么走」第四条、第五条，施工 8-9；原来叫 `route_failover.rs`，LM-8 砍了
//! 模型池以后只留单个模型的那几条）：限速的照供应商说的等、在它自己上面再来；说到一半断了的接着发给它；请求本身有错的
//! 不再来；窗口放不下这份请求的不发、照上下文超长报。池里换人、冷却、全在冷却交 `cooling`、换端点数进 5 次那几条跟着池删了。

use std::sync::Arc;
use std::time::Duration;

use crate::support::routing::{called, configs, cut_after, hellos, routes};
use crate::support::{Home, ask, say, stop, until_logged, until_turn_ends, watch};
use lm_engine::http::testkit::{Reply, Server};
use lm_engine::session::{Handle, Pushed};
use lm_kernel::kernel::event::{Body, ErrorClass, Status, TransientBody};

/// 一家 `a`，模型 `m` 的窗口 `window`，`models.chat` 是 `a/m`，不带 key。
fn single(server: &Server, window: u64) -> String {
    format!(
        "[providers.a]\ndriver = \"openai-chat\"\nbase_url = \"{}\"\n\n[providers.a.models.m]\nwindow = {window}\n\n[models]\nchat = \"a/m\"\n",
        server.base_url
    )
}

/// 说一句 `text`，等这一轮了结，交回推来的。
async fn turn(handle: &Handle, command: &str, text: &str) -> Vec<Arc<Pushed>> {
    let mut pushes = watch(handle).await;
    ask(handle, command, say(text)).await.expect("会话在跑");
    until_turn_ends(&mut pushes).await
}

/// 推来的「等着再来」。
fn statuses(pushed: &[Arc<Pushed>]) -> Vec<Status> {
    pushed
        .iter()
        .filter_map(|pushed| match &**pushed {
            Pushed::Transient(transient) => match &transient.body {
                TransientBody::Status(status) => Some(status.clone()),
                _ => None,
            },
            _ => None,
        })
        .collect()
}

/// 主请求的收场：`<供应商>/<模型>:<分类或 ok>`，照先后。
fn sent(home: &Home, handle: &Handle) -> Vec<String> {
    called(home, handle)
        .iter()
        .filter(|call| call.purpose.is_none())
        .map(|call| {
            format!(
                "{}/{}:{}",
                call.endpoint.as_ref().map_or("?", |id| id.as_str()),
                call.model.as_ref().map_or("?", |name| name.as_str()),
                call.error
                    .as_ref()
                    .map_or("ok", |error| error.class.as_str())
            )
        })
        .collect()
}

#[tokio::test]
async fn a_rate_limited_model_is_asked_again_on_itself() {
    let mut replies = vec![Reply::error(429, &[("retry-after", "1")], "{}")];
    replies.extend(hellos(4));
    let server = Server::start(replies).await;
    let mut home = Home::new();
    home.configs = configs(&single(&server, 64_000), &[]);
    let routes = routes(serde_json::json!({}), Duration::from_secs(60));
    let handle = home.create(&routes).await;
    let pushed = turn(&handle, "cmd-1", "hi").await;
    let status = statuses(&pushed);
    assert_eq!(status.len(), 1, "再来一次");
    assert_eq!(status[0].retry.wait_ms, 1000, "照供应商说的等");
    assert_eq!(sent(&home, &handle), ["a/m:rate_limited", "a/m:ok"]);
}

#[tokio::test]
async fn a_reply_cut_midway_is_asked_again_on_the_same_model() {
    let mut replies = vec![cut_after(2)];
    replies.extend(hellos(6));
    let server = Server::start(replies).await;
    let mut home = Home::new();
    home.configs = configs(&single(&server, 64_000), &[]);
    let routes = routes(serde_json::json!({}), Duration::from_secs(60));
    let handle = home.create(&routes).await;
    turn(&handle, "cmd-1", "hi").await;
    assert_eq!(
        sent(&home, &handle),
        ["a/m:retryable", "a/m:ok"],
        "说到一半断了：还发给它，接着说"
    );
}

#[tokio::test]
async fn a_bad_request_is_not_asked_again() {
    let mut replies = vec![Reply::error(
        400,
        &[],
        r#"{"error":{"message":"bad request"}}"#,
    )];
    replies.extend(hellos(4));
    let server = Server::start(replies).await;
    let mut home = Home::new();
    home.configs = configs(&single(&server, 64_000), &[]);
    let routes = routes(serde_json::json!({}), Duration::from_secs(60));
    let handle = home.create(&routes).await;
    let pushed = turn(&handle, "cmd-1", "hi").await;
    assert!(statuses(&pushed).is_empty(), "请求本身有错：不再来");
    assert_eq!(sent(&home, &handle), ["a/m:other"]);
    turn(&handle, "cmd-2", "again").await;
    assert_eq!(
        sent(&home, &handle),
        ["a/m:other", "a/m:ok"],
        "下一轮照发它"
    );
}

/// 窗口放不下这份请求（施工 Q-4 上：原来在池里是「放不下的成员不挑、出错也不换过去」，单个模型也照它）：不发，照上下文超长报，
/// 原话写用量和窗口（内核接着先压再来）。
#[tokio::test]
async fn a_request_the_window_cannot_hold_is_not_sent() {
    let server = Server::start(hellos(8)).await;
    let mut home = Home::new();
    home.configs = configs(&single(&server, 32_000), &[]);
    let routes = routes(serde_json::json!({}), Duration::from_secs(60));
    let handle = home.create(&routes).await;
    // 约四万个 token：放不进 32000。
    let long = "word ".repeat(32_000);
    ask(&handle, "cmd-1", say(&long)).await.expect("会话在跑");
    let log = until_logged(&home, handle.id(), |log| {
        log.iter()
            .any(|event| matches!(&event.body, Body::ModelCalled(called) if called.error.is_some()))
    })
    .await;
    let first = log
        .iter()
        .find_map(|event| match &event.body {
            Body::ModelCalled(called) if called.purpose.is_none() => Some(called.clone()),
            _ => None,
        })
        .expect("记了一次");
    let error = first.error.as_ref().expect("出错");
    assert_eq!(error.class, ErrorClass::ContextTooLong);
    assert!(
        error.message.starts_with("request of about ")
            && error
                .message
                .ends_with("fits no candidate's window: a/m 32000"),
        "{}",
        error.message
    );
    assert_eq!(first.endpoint, None, "没发出去");
    stop(&handle).await;
}
