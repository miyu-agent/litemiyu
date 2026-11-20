//! 2xx 但不是 API 应答（施工 Q-4 中，设计 43 第三节第 5 条第 3 款）：中转站、网关常回一页 HTML（登录页、维护页）、空的响应体，
//! 状态却是 200。驱动拿不到一个可解析的应答，报可重试（内核照退避再来）。流里已经给过内容再断的照原来的断流办，不在这里。
//! （原来这里还看池里照样记冷却、换下一个成员：LM-8 砍了模型池，只留「分类是可重试」这一半。）

use std::time::Duration;

use crate::support::routing::{configs, routes};
use crate::support::{Home, ask, say, stop, until_logged};
use lm_engine::http::testkit::{Reply, Server};
use lm_kernel::kernel::event::{Body, ErrorClass};

/// 一家 `a`，驱动是 `driver`，`models.chat` 是 `a/x`。
fn single(server: &Server, driver: &str) -> String {
    format!(
        "[providers.a]\ndriver = \"{driver}\"\nbase_url = \"{}\"\n\n[models]\nchat = \"a/x\"\n",
        server.base_url
    )
}

/// 第一次请求回 `reply`，三种驱动各看一遍那一次怎么收场：都该是可重试。看到第一条出错的 `model.called` 就停掉会话（再来要等
/// 退避的一秒，这里不等）。
async fn first_says(reply: Reply) {
    for driver in ["openai-chat", "anthropic", "openai-responses"] {
        let server = Server::start(vec![reply.clone()]).await;
        let mut home = Home::new();
        home.configs = configs(&single(&server, driver), &[]);
        let routes = routes(serde_json::json!({}), Duration::from_secs(60));
        let handle = home.create(&routes).await;
        ask(&handle, "cmd-1", say("hi")).await.expect("会话在跑");
        let log = until_logged(&home, handle.id(), |log| {
            log.iter().any(
                |event| matches!(&event.body, Body::ModelCalled(called) if called.error.is_some()),
            )
        })
        .await;
        let class = log.iter().find_map(|event| match &event.body {
            Body::ModelCalled(called) => called.error.as_ref().map(|error| error.class.clone()),
            _ => None,
        });
        assert_eq!(class, Some(ErrorClass::Retryable), "{driver}");
        stop(&handle).await;
    }
}

#[tokio::test]
async fn a_200_html_page_is_retryable() {
    let page = Reply::error(
        200,
        &[("Content-Type", "text/html; charset=utf-8")],
        "<!doctype html><html><body>Please sign in</body></html>",
    );
    first_says(page).await;
}

#[tokio::test]
async fn a_200_empty_body_is_retryable() {
    let empty = Reply::error(200, &[("Content-Type", "text/event-stream")], "");
    first_says(empty).await;
}

#[tokio::test]
async fn a_200_json_that_is_not_an_answer_is_retryable() {
    let json = Reply::error(
        200,
        &[("Content-Type", "application/json")],
        r#"{"status":"ok"}"#,
    );
    first_says(json).await;
}
