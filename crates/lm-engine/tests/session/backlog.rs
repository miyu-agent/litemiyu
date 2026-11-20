//! 订阅时补发之前的事件（施工 3-8 六补，`docs/blueprint/session/actor.md` 第 6 条）：补发的那一截和订阅在 actor 的同一步
//! 里拿。补到那一步落了盘的最后一条；之后追加的只从订阅推过来，补的时候日志里已经多了也不读进来：不重不漏。

use lm_engine::session::Pushed;
use lm_engine::session::testkit::{Play, Script};
use lm_kernel::kernel::event::Event;

use crate::support::*;

/// 推过来的落了盘的事件，照先后。
fn persisted(pushed: &[std::sync::Arc<Pushed>]) -> Vec<Event> {
    pushed
        .iter()
        .flat_map(|pushed| match &**pushed {
            Pushed::Events(events) => events.clone(),
            Pushed::Transient(_) => Vec::new(),
        })
        .collect()
}

#[tokio::test]
async fn the_backlog_ends_where_the_subscription_begins() {
    let home = Home::new();
    let script = Script::new([Play::Says("一。"), Play::Says("二。")]);
    let handle = home.create(&script).await;
    let mut first = watch(&handle).await;
    ask(&handle, "cmd-1", say("hi")).await.expect("会话在跑");
    until_turn_ends(&mut first).await;
    let landed = home.log(handle.id());

    let (mut subscription, backlog) = within("订阅", handle.subscribe_after(0))
        .await
        .expect("会话在跑");
    assert_eq!(
        backlog.upto(),
        landed.len() as u64,
        "补到订阅那一刻的最后一条"
    );
    // 订阅以后、读之前又说一句：落了盘才回应，日志里已经多了。
    ask(&handle, "cmd-2", say("again")).await.expect("会话在跑");
    let read = within("读", backlog.read()).await.expect("读得了");
    assert_eq!(read, landed, "只补到 upto，后来的不读进来");
    let pushed = persisted(&until_turn_ends(&mut subscription).await);
    let log = home.log(handle.id());
    assert_eq!(
        pushed,
        log[landed.len()..],
        "后来的从订阅推过来，从下一条起"
    );
}

#[tokio::test]
async fn the_backlog_starts_after_the_given_seq() {
    let home = Home::new();
    let handle = home.create(&Script::new([Play::Says("一。")])).await;
    let mut first = watch(&handle).await;
    ask(&handle, "cmd-1", say("hi")).await.expect("会话在跑");
    until_turn_ends(&mut first).await;
    let log = home.log(handle.id());
    let last = log.len() as u64;
    for after in [0, 1, last - 1, last, last + 7] {
        let (_, backlog) = within("订阅", handle.subscribe_after(after))
            .await
            .expect("会话在跑");
        assert_eq!(backlog.upto(), last, "after {after}");
        let read = within("读", backlog.read()).await.expect("读得了");
        let from = usize::try_from(after.min(last)).expect("放得下");
        assert_eq!(read, log[from..], "after {after}：序号大于它的");
    }
}

/// 审计 2026-11-18 #063：订阅掉了队，照 [`lm_engine::session::Rejoin`] 从最后看到的序号重新订阅，落了盘的补上、不重不漏；
/// 拿着它不拖住会话：把手都放下了，会话照停，再重新订阅交回「停了」。
#[tokio::test]
async fn a_rejoin_catches_up_after_the_last_seen_and_does_not_hold_the_session() {
    let home = Home::new();
    let handle = home
        .create(&Script::new([Play::Says("一。"), Play::Says("二。")]))
        .await;
    let mut first = watch(&handle).await;
    ask(&handle, "cmd-1", say("hi")).await.expect("会话在跑");
    until_turn_ends(&mut first).await;
    let seen = home.log(handle.id()).len() as u64;
    ask(&handle, "cmd-2", say("again")).await.expect("会话在跑");
    until_turn_ends(&mut first).await;
    let log = home.log(handle.id());

    let rejoin = handle.rejoin();
    let (_, backlog) = within("重新订阅", rejoin.subscribe_after(seen))
        .await
        .expect("会话在跑");
    assert_eq!(backlog.upto(), log.len() as u64);
    let read = within("读", backlog.read()).await.expect("读得了");
    let from = usize::try_from(seen).expect("放得下");
    assert_eq!(read, log[from..], "从最后看到的下一条补起");

    drop(first);
    drop(handle);
    let mut stopped = false;
    for _ in 0..200 {
        if within("重新订阅", rejoin.subscribe_after(0)).await.is_err() {
            stopped = true;
            break;
        }
        tokio::task::yield_now().await;
    }
    assert!(stopped, "把手都放下了，会话照停，重新订阅不了");
}
