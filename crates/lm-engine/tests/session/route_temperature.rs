//! 温度（`docs/blueprint/models.md`「怎么走」第十四条，施工 8-22）：一次请求照真发的那个模型配置的默认带，没有就不带；
//! 换模型以后用新模型自己的；轮换的池里每个成员用自己的；改了配置下一轮生效。
//!
//! 两台假服务器，档案是空的、没有目录：没有目录说不收，温度都当能调。

use std::sync::Arc;
use std::time::Duration;

use tokio::sync::watch as channel;

use crate::support::routing::{configs, hellos, items, routes};
use crate::support::{Home, Opening, ask, say, until_turn_ends, watch};
use lm_engine::http::testkit::Server;
use lm_engine::session::{ConfigSource, Handle, Models, fixed_with};
use lm_engine::tool::Catalog;
use lm_kernel::config::Layer;
use lm_kernel::config::merge::{Layers, Resolved, merge};
use lm_kernel::config::parse::parse;
use lm_kernel::kernel::session::Command;
use serde_json::Value;

/// 两家 `a`、`b`，都不带 key。`a` 的 `m` 默认 0.7；`b` 的 `n` 没写。`models.chat` 是 `a/m`。
fn config(first: &Server, second: &Server, extra: &str) -> String {
    format!(
        "[providers.a]\ndriver = \"openai-chat\"\nbase_url = \"{}\"\n\n[providers.a.models.m]\ntemperature = 0.7\n\n\
         [providers.b]\ndriver = \"openai-chat\"\nbase_url = \"{}\"\n\n[providers.b.models.n]\n{extra}\n\
         [models]\nchat = \"a/m\"\n",
        first.base_url, second.base_url
    )
}

/// 造一个会话，记着的模型是 `model`。
async fn create(home: &Home, models: &dyn Models, model: &str) -> Handle {
    let opening = Opening {
        model: Some(model.to_string()),
        ..Opening::default()
    };
    home.create_as(models, &Catalog::default(), opening).await
}

/// 说一句、等这一轮说完。
async fn turn(handle: &Handle, command: &str) {
    let mut pushes = watch(handle).await;
    ask(handle, command, say("hi")).await.expect("会话在跑");
    until_turn_ends(&mut pushes).await;
}

/// 一台服务器收到的每一份请求里的 `temperature`，照先后；没带的是 `null`。
fn sent(server: &Server) -> Vec<Value> {
    server
        .received()
        .iter()
        .map(|received| {
            let body: Value = serde_json::from_slice(&received.body).expect("请求是 JSON");
            body.get("temperature").cloned().unwrap_or(Value::Null)
        })
        .collect()
}

#[tokio::test]
async fn a_request_takes_the_configured_default_or_nothing() {
    let (first, second) = (
        Server::start(hellos(12)).await,
        Server::start(hellos(12)).await,
    );
    let mut home = Home::new();
    home.configs = configs(&config(&first, &second, ""), &[]);
    let routes = routes(serde_json::json!({}), Duration::from_secs(60));
    let handle = create(&home, &routes, "a/m").await;
    turn(&handle, "cmd-1").await;
    let a = sent(&first);
    assert!(!a.is_empty(), "发过");
    assert!(
        a.iter().all(|value| *value == 0.7),
        "主请求、辅助请求都带配置的默认：{a:?}"
    );
    // 换模型以后用新模型自己的：`b/n` 没写，什么都不带。
    let configure = Command::Configure {
        model: "b/n".to_string(),
    };
    ask(&handle, "cmd-2", configure).await.expect("会话在跑");
    turn(&handle, "cmd-3").await;
    let b = sent(&second);
    assert!(
        !b.is_empty() && b.iter().all(Value::is_null),
        "什么都不带：{b:?}"
    );
}

/// 系统配置 `system` 合出来的最终值，当成一份不变的配置源（施工 SU-3 上：只剩这一层）。
fn one_layer(system: &str) -> Arc<dyn ConfigSource> {
    let item_list = items();
    let system = parse(&item_list, Layer::System, system).expect("写法对");
    let layers = Layers {
        system: Some(&system),
        ..Layers::default()
    };
    let resolved: Resolved = merge(&item_list, &layers, &|_| None);
    Arc::clone(&*fixed_with(resolved, Vec::new()).borrow())
}

/// 配置改了下一轮生效（`next_turn`；SU-3 上起只剩系统配置这一层）。
#[tokio::test]
async fn a_changed_setting_applies_on_the_next_turn() {
    let (first, second) = (
        Server::start(hellos(12)).await,
        Server::start(hellos(12)).await,
    );
    let system = config(&first, &second, "");
    let (switching, receiving) = channel::channel(one_layer(&system));
    let mut home = Home::new();
    home.configs = receiving;
    let routes = routes(serde_json::json!({}), Duration::from_secs(60));
    let handle = create(&home, &routes, "a/m").await;
    turn(&handle, "cmd-1").await;
    assert_eq!(
        sent(&first).first(),
        Some(&Value::from(0.7)),
        "先是系统配置"
    );
    let changed_config = system.replace("temperature = 0.7", "temperature = 0");
    assert_ne!(changed_config, system, "锚点对得上");
    switching.send_replace(one_layer(&changed_config));
    turn(&handle, "cmd-2").await;
    assert_eq!(
        sent(&first).last(),
        Some(&Value::from(0)),
        "改成 0 也是写了的"
    );
}
