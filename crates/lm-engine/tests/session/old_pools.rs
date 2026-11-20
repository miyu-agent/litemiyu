//! 旧日志里按池选模型的会话（LM-8 砍了模型池）：`session.created` 记着 `@池`、`model.called` 发给的是池里的
//! 成员。这样的会话照旧载入、接着聊：`@池` 认不出了，回合开始时退回这一刻的 `models.chat`，内核记一条 `session.policy_changed`
//! （`model` 是退回的、`replaced` 是原来的 `@池`），以后钉在它上面。配置里还写着的 `[pools.*]` 是退役的键，读得进、不起作用。

use std::time::Duration;

use crate::support::routing::{called, configs, hellos, routes};
use crate::support::{Home, Opening, ask, say, stop, until_turn_ends, watch};
use lm_engine::http::testkit::Server;
use lm_engine::session::{Handle, Models};
use lm_engine::tool::Catalog;
use lm_kernel::kernel::event::{Body, Event};

/// 两家：`a` 在 `first`、`b` 在 `second`，都不带 key；`models.chat` 是 `a/m`；还留着那时候的池 `p`（成员 `b/y`，钉住）。
fn config(first: &Server, second: &Server) -> String {
    format!(
        "[providers.a]\ndriver = \"openai-chat\"\nbase_url = \"{}\"\n\n[providers.b]\ndriver = \"openai-chat\"\nbase_url = \"{}\"\n\n\
         [models]\nchat = \"a/m\"\n\n[pools.p]\nmodels = [\"b/y\"]\nstrategy = \"pin\"\n\n[pools.flagship]\nsubagent = false\n",
        first.base_url, second.base_url
    )
}

/// 说一句，等这一轮了结。
async fn turn(handle: &Handle, command: &str) {
    let mut pushes = watch(handle).await;
    ask(handle, command, say("hi")).await.expect("会话在跑");
    until_turn_ends(&mut pushes).await;
}

/// 主请求发给了哪一家的哪个模型（`<供应商>/<模型>`），照先后。
fn sent(home: &Home, handle: &Handle) -> Vec<String> {
    called(home, handle)
        .iter()
        .filter(|call| call.purpose.is_none())
        .filter_map(|call| {
            Some(format!(
                "{}/{}",
                call.endpoint.as_ref()?,
                call.model.as_ref()?
            ))
        })
        .collect()
}

#[tokio::test]
async fn an_old_session_made_on_a_pool_loads_and_goes_on_with_the_chat_model() {
    let (first, second) = (
        Server::start(hellos(4)).await,
        Server::start(hellos(4)).await,
    );
    let mut home = Home::new();
    home.configs = configs(&config(&first, &second), &[]);
    let routes = routes(serde_json::json!({}), Duration::from_secs(60));
    // 那时候：会话造在池上，钉着的成员是 `b/y`，说了一轮。先照 `b/y` 造、说一轮，再把 `session.created` 记的改成 `@p`（那时候
    // 写的就是这样：造会话记引用，`model.called` 记真发给的成员）。
    let opening = Opening {
        model: Some("b/y".to_string()),
        ..Opening::default()
    };
    let handle = home.create_as(&routes, &Catalog::default(), opening).await;
    turn(&handle, "cmd-1").await;
    let session = handle.id().clone();
    stop(&handle).await;
    let segment = home.root.session(&session).join("000000000001.jsonl");
    let text = std::fs::read_to_string(&segment).expect("读得了第一段");
    let old: String = text
        .lines()
        .map(|line| {
            let mut event = Event::from_line(line).expect("每一行都是事件");
            if let Body::SessionCreated(created) = &mut event.body {
                created.model = Some("@p".to_string());
            }
            event.to_line() + "\n"
        })
        .collect();
    assert!(old.contains(r#""model":"@p""#), "{old}");
    std::fs::write(&segment, old).expect("写得回去");

    // 现在：载入得了，接着说一句，发给 `models.chat`；退回默认的那一次记下。
    let loaded: Handle = home.load(&session, &routes as &dyn Models).await;
    turn(&loaded, "cmd-2").await;
    assert_eq!(
        sent(&home, &loaded),
        ["b/y", "a/m"],
        "旧的那一轮照旧在；`@p` 认不出，退回 models.chat"
    );
    let changed: Vec<(Option<String>, Option<String>)> = home
        .log(&session)
        .iter()
        .filter_map(|event| match &event.body {
            Body::PolicyChanged(changed) if changed.model.is_some() => {
                Some((changed.model.clone(), changed.replaced.clone()))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        changed,
        [(Some("a/m".to_string()), Some("@p".to_string()))],
        "内核记一条退回默认的"
    );
    turn(&loaded, "cmd-3").await;
    assert_eq!(
        sent(&home, &loaded),
        ["b/y", "a/m", "a/m"],
        "以后钉在它上面"
    );
    stop(&loaded).await;
    assert!(second.received().len() == 1, "池里的成员只在那时候收过一次");
}
