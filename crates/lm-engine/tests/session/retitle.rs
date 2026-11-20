//! 人要求重起标题（Miyu 施工 TT-1～TT-3，`Handle::retitle`）：真 actor、假模型。Miyu 这几条在协议那一层
//! （`miyu-endpoint/tests/retitle.rs`、`retitle_failed.rs`）；LiteMiyu 没有协议，照引擎交给界面、命令行的入口测：起好了交回
//! 新标题、记成人起的；没起成交回错、标题不动；总时限到了照出错；没东西可起的当场拒。

use std::time::Duration;

use lm_engine::session::Retitled;
use lm_engine::session::testkit::{Play, Script};
use lm_kernel::kernel::event::{Body, ErrorClass, Event};
use lm_kernel::kernel::origin::By;
use lm_kernel::kernel::session::{Command, Reason};

use crate::support::*;

/// 日志里的标题变化，照先后：是不是人起的、标题。
fn titles(log: &[Event]) -> Vec<(bool, String)> {
    log.iter()
        .filter_map(|event| match &event.body {
            Body::MetaChanged(changed) => changed
                .title
                .clone()
                .map(|title| (matches!(event.by, By::Person(_)), title)),
            _ => None,
        })
        .collect()
}

/// 说一轮，人起名「人起的」，等它落盘。
async fn one_turn_named(home: &Home, script: &Script) -> lm_engine::session::Handle {
    let handle = home.create(script).await;
    let mut pushes = watch(&handle).await;
    ask(&handle, "m1", say("帮我看看这个仓库"))
        .await
        .expect("会话在跑");
    until_turn_ends(&mut pushes).await;
    let named = Command::SetMeta {
        title: Some("人起的".to_string()),
        pinned: None,
    };
    ask(&handle, "n1", named).await.expect("会话在跑");
    until_logged(home, handle.id(), |log| !titles(log).is_empty()).await;
    handle
}

/// 要了交回结果。
async fn retitle(handle: &lm_engine::session::Handle, text: &str) -> Result<Retitled, Reason> {
    let asked = within("重起标题的回应", handle.retitle(id(text), alice()))
        .await
        .expect("会话在跑")?;
    Ok(within("重起标题的结果", asked).await.expect("结果交得回来"))
}

#[tokio::test]
async fn a_retitle_names_it_from_recent_turns_as_the_person() {
    let home = Home::new();
    let script = Script::new([Play::Says("看过了。")]).titles([Play::Says("重起的")]);
    let handle = one_turn_named(&home, &script).await;
    let got = retitle(&handle, "r1").await;
    assert_eq!(got, Ok(Retitled::Titled("重起的".to_string())));
    let log = until_logged(&home, handle.id(), |log| titles(log).len() == 2).await;
    stop(&handle).await;
    assert_eq!(
        titles(&log),
        [(true, "人起的".to_string()), (true, "重起的".to_string())],
        "重起的盖掉人起的，也算人起的（TT-2）"
    );
    assert_eq!(script.titled().len(), 1, "发一次起标题的请求");
}

#[tokio::test]
async fn a_failed_retitle_is_told_and_keeps_the_title() {
    let home = Home::new();
    let script = Script::new([Play::Says("看过了。")]).titles([Play::Fails {
        class: ErrorClass::Retryable,
        wait_ms: None,
    }]);
    let handle = one_turn_named(&home, &script).await;
    let Ok(Retitled::Failed(failed)) = retitle(&handle, "r1").await else {
        panic!("没起成要交回错");
    };
    assert_eq!(failed.error.class, ErrorClass::Retryable);
    assert_eq!(failed.human.key, "core/sessions/retitle-failed");
    stop(&handle).await;
    assert_eq!(
        titles(&home.log(handle.id())),
        [(true, "人起的".to_string())],
        "标题不动"
    );
}

/// 辅助请求的总时限（TT-3）：剧本设成 100 毫秒，重起的那一次 5 秒才说完，照出错交回，「总超时」。
#[tokio::test]
async fn a_retitle_past_the_aside_limit_fails() {
    let home = Home::new();
    let script = Script::new([Play::Says("看过了。")])
        .titles([Play::Late {
            ms: 5_000,
            text: "太晚了",
        }])
        .aside_limit(Duration::from_millis(100));
    let handle = one_turn_named(&home, &script).await;
    let Ok(Retitled::Failed(failed)) = retitle(&handle, "r1").await else {
        panic!("过了总时限要交回错");
    };
    assert_eq!(failed.error.class, ErrorClass::Retryable);
    assert!(
        failed.error.message.starts_with("总超时："),
        "{}",
        failed.error.message
    );
    stop(&handle).await;
    assert_eq!(
        titles(&home.log(handle.id())),
        [(true, "人起的".to_string())]
    );
}

/// 一个回答都没有、也没有回合在进行：当场拒，`nothing_to_title`。
#[tokio::test]
async fn nothing_to_title_is_refused() {
    let home = Home::new();
    let script = Script::new([]);
    let handle = home.create(&script).await;
    assert_eq!(retitle(&handle, "r1").await, Err(Reason::NothingToTitle));
    stop(&handle).await;
    assert!(script.titled().is_empty());
}
