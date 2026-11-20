//! 会话的上下文上限（Miyu 施工 CTX-1，设计 68 第二节；照 `miyu-session/tests/route_context.rs` 搬）：路由交的限额，窗口取模型的
//! 和上限里小的，模型没报窗口的照上限；设了当场生效、不等下一轮；换模型照用；去掉回到模型的；重启以后照日志读回来。另加
//! LiteMiyu 的入口 `Handle::set_context`：太小的拒、交回最少要多少（Miyu 在协议那一层，`context_cap.rs`）。
//!
//! 一台假服务器、没有目录：`a/m` 的窗口 1000000，`b/n` 的 64000，`b/y` 没写窗口。

use std::time::Duration;

use crate::support::routing::{configs, hellos, routes};
use crate::support::{Home, Opening, alice, ask, id, say, stop, until_turn_ends, watch};
use lm_engine::http::testkit::Server;
use lm_engine::session::{ContextRefused, Handle, Models};
use lm_engine::tool::Catalog;
use lm_kernel::kernel::session::Command;

fn config(server: &Server) -> String {
    format!(
        "[providers.a]\ndriver = \"openai-chat\"\nbase_url = \"{0}\"\n\n[providers.a.models.m]\nwindow = 1000000\n\n\
         [providers.b]\ndriver = \"openai-chat\"\nbase_url = \"{0}\"\n\n[providers.b.models.n]\nwindow = 64000\n\n\
         [models]\nchat = \"a/m\"\n",
        server.base_url
    )
}

async fn create(home: &Home, models: &dyn Models, model: &str) -> Handle {
    let opening = Opening {
        model: Some(model.to_string()),
        ..Opening::default()
    };
    home.create_as(models, &Catalog::default(), opening).await
}

/// 说 `words`、等这一轮说完。
async fn turn(handle: &Handle, command: &str, words: &str) {
    let mut pushes = watch(handle).await;
    ask(handle, command, say(words)).await.expect("会话在跑");
    until_turn_ends(&mut pushes).await;
}

async fn send(handle: &Handle, command: &str, sent: Command) {
    ask(handle, command, sent).await.expect("会话在跑");
}

fn cap(tokens: Option<u64>) -> Command {
    Command::SetContext { tokens }
}

fn switch(model: &str) -> Command {
    Command::Configure {
        model: model.to_string(),
    }
}

/// 给头看的窗口、上限。
fn shown(handle: &Handle) -> (Option<u64>, Option<u64>) {
    let limits = handle.limits();
    (limits.window, limits.context)
}

#[tokio::test]
async fn the_cap_follows_the_session_across_models_and_restarts() {
    let server = Server::start(hellos(24)).await;
    let mut home = Home::new();
    home.configs = configs(&config(&server), &[]);
    let routes = routes(serde_json::json!({}), Duration::from_secs(60));
    let a = create(&home, &routes, "a/m").await;
    assert_eq!(shown(&a), (Some(1_000_000), None));
    send(&a, "a-1", cap(Some(128_000))).await;
    assert_eq!(
        shown(&a),
        (Some(128_000), Some(128_000)),
        "当场生效，不等下一轮"
    );
    turn(&a, "a-2", "alpha").await;
    assert_eq!(
        shown(&a),
        (Some(128_000), Some(128_000)),
        "回合开始重算照套"
    );
    send(&a, "a-3", switch("b/n")).await;
    turn(&a, "a-4", "bravo").await;
    assert_eq!(
        shown(&a),
        (Some(64_000), Some(128_000)),
        "模型的小：照模型的"
    );
    send(&a, "a-5", switch("b/y")).await;
    turn(&a, "a-6", "charlie").await;
    assert_eq!(
        shown(&a),
        (Some(128_000), Some(128_000)),
        "模型没报窗口：照上限"
    );
    send(&a, "a-7", switch("a/m")).await;
    turn(&a, "a-8", "delta").await;
    assert_eq!(shown(&a), (Some(128_000), Some(128_000)), "换回来照套");
    stop(&a).await;
    let a = home.load(a.id(), &routes).await;
    assert_eq!(
        shown(&a),
        (Some(128_000), Some(128_000)),
        "载入照日志算的造路由"
    );
    send(&a, "a-9", cap(None)).await;
    assert_eq!(shown(&a), (Some(1_000_000), None), "去掉回到模型的");
    stop(&a).await;
}

/// `Handle::set_context`（Miyu `models/context.rs` 那一关）：比压缩线算得出的最小窗口小的、0 拒，交回最少要多少，什么都不记；
/// 够的设上、当场生效；去掉不查。
#[tokio::test]
async fn too_small_a_cap_is_refused_with_the_least() {
    let server = Server::start(hellos(4)).await;
    let mut home = Home::new();
    home.configs = configs(&config(&server), &[]);
    let routes = routes(serde_json::json!({}), Duration::from_secs(60));
    let a = create(&home, &routes, "a/m").await;
    let least = a
        .current()
        .await
        .expect("会话在跑")
        .least_context
        .expect("策略里有压缩的数");
    assert!(least > 1, "{least}");
    for (n, tokens) in [0, least - 1].into_iter().enumerate() {
        let got = a
            .set_context(id(&format!("c-{n}")), alice(), Some(tokens))
            .await
            .expect("会话在跑");
        assert_eq!(
            got,
            Err(ContextRefused::TooSmall { min: least }),
            "{tokens}"
        );
    }
    assert_eq!(shown(&a), (Some(1_000_000), None), "拒了的什么都不记");
    let got = a.set_context(id("c-ok"), alice(), Some(least)).await;
    assert_eq!(got, Ok(Ok(())));
    assert_eq!(shown(&a), (Some(least), Some(least)));
    let got = a.set_context(id("c-off"), alice(), None).await;
    assert_eq!(got, Ok(Ok(())));
    assert_eq!(shown(&a), (Some(1_000_000), None));
    stop(&a).await;
}
