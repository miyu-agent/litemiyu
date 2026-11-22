//! 循环和撤回（LM-34，2026-11-22 项目主人；同 Miyu 核心 FX-13、FX-14）：
//!
//! - 人亲手敲 `/loop`（`Handle::start_loop`）是人的一次操作：有一次撤回还能恢复也马上交进去、开一轮，那一句带着人敲的那一行，
//!   她看到的照旧是循环的标签和那句话；开了这一轮，`/restore` 被拒。
//! - 之后到点（固定间隔的、她自己定节奏的、到点继续的）遇到撤回还能恢复：这一次不交、日志里什么都不写，循环搁着、欠一次
//!   （`Looping::held`）；撤回处理了（人说了一句、那一轮结束了，或者 `/restore` 恢复完）补交一次，中间到点几次都只补一次。
//!
//! 同步点：到点交给 actor 以后，「搁着」照把手读得到（`Handle::looping` 的 `held`、固定间隔的下一次换了），读到了再查日志里
//! 没多东西；不靠「等够久」。不真等：同 `looping.rs`，到点前把 tokio 的钟拨过去。

use lm_engine::session::testkit::Play;
use lm_engine::session::{Handle, Looping, Pushed, Subscription};
use lm_kernel::kernel::event::{Body, Event};
use lm_kernel::kernel::origin::{By, ScheduleLoop};
use lm_kernel::kernel::session::{Command, Outcome, Reason};

use super::{catalog, fired, script, skip, turn, typed};
use crate::support::*;

/// 撤回最后一轮。
async fn undo(handle: &Handle, id: &str) {
    let outcome = ask(handle, id, Command::Revert { turn: None })
        .await
        .expect("会话在跑");
    assert!(
        !matches!(outcome, Outcome::Rejected { .. }),
        "撤得了：{outcome:?}"
    );
}

/// 恢复撤回：交回回应。
async fn restore(handle: &Handle, id: &str) -> Outcome {
    ask(handle, id, Command::Unrevert).await.expect("会话在跑")
}

/// 到点了、搁着：拨过 `seconds` 秒，等到 actor 办完这一次到点。同步点是搁着时推来的那一批空的（没交东西就没有别的推送，
/// actor 推它叫头看一眼循环）。交回那时的循环。
async fn due_while_undone(
    handle: &Handle,
    subscription: &mut Subscription,
    seconds: u64,
) -> Looping {
    skip(seconds).await;
    within("到点了搁着", async {
        loop {
            let next = subscription.next().await.expect("订阅没断");
            // 撤回那一批可能还没读走：照先后读过去，日志里多没多由调的一方查。
            if matches!(&*next, Pushed::Events(events) if events.is_empty()) {
                return;
            }
        }
    })
    .await;
    let looping = handle.looping().expect("循环开着");
    assert!(looping.held, "搁着：{looping:?}");
    looping
}

/// 撤回怎么处理。
#[derive(Debug, Clone, Copy)]
enum Settle {
    /// 人说一句，等那一轮结束。
    Say,
    /// `/restore`。
    Restore,
}

/// 照 `how` 处理撤回，等补交的那一轮结束。
async fn settle(handle: &Handle, subscription: &mut Subscription, how: Settle) {
    match how {
        Settle::Say => {
            ask(handle, "m9", say("在吗")).await.expect("会话在跑");
            turn(subscription).await;
        }
        Settle::Restore => {
            let outcome = restore(handle, "r9").await;
            assert!(
                !matches!(outcome, Outcome::Rejected { .. }),
                "恢复得了：{outcome:?}"
            );
        }
    }
    turn(subscription).await;
}

/// 最后一句到点交的排在处理撤回以后：人说的那一句开的那一轮结束了以后，或者恢复以后。
fn owed_after(log: &[Event], how: Settle) -> bool {
    let owed = log.iter().rposition(|event| {
        matches!(event.by, By::Schedule(_)) && matches!(event.body, Body::MessageUser(_))
    });
    let settled = match how {
        Settle::Say => log
            .iter()
            .rposition(|event| event.by == alice() && matches!(event.body, Body::MessageUser(_)))
            .and_then(|said| {
                log[said..]
                    .iter()
                    .position(|event| matches!(event.body, Body::TurnEnded(_)))
                    .map(|ended| said + ended)
            }),
        Settle::Restore => log
            .iter()
            .position(|event| matches!(event.body, Body::TurnUnreverted(_))),
    };
    matches!((settled, owed), (Some(settled), Some(owed)) if settled < owed)
}

#[tokio::test]
async fn a_loop_the_person_starts_after_an_undo_runs_at_once_and_ends_the_undo() {
    let home = Home::new();
    let models = script([Play::Says("你好。"), Play::Says("CI 还在跑。")]);
    let handle = home.create_with(&models, &catalog()).await;
    let mut subscription = watch(&handle).await;
    ask(&handle, "m1", say("hi")).await.expect("会话在跑");
    turn(&mut subscription).await;
    undo(&handle, "u1").await;
    within(
        "开循环",
        handle.start_loop(Some(300), "看看 CI".to_string(), typed("5m 看看 CI")),
    )
    .await
    .expect("会话在跑");
    let events = turn(&mut subscription).await;
    assert!(
        events
            .iter()
            .any(|event| matches!(event.body, Body::TurnStarted(_))),
        "马上开了一轮：{:?}",
        kinds(&events)
    );
    let requests = models.requests();
    assert_eq!(requests.len(), 2, "第一句的请求发出去了");
    let request = format!("{:?}", requests[1].1);
    assert!(
        request.contains("看看 CI") && request.contains("<scheduled-task id="),
        "她看到的是循环的标签和那句话：{request}"
    );
    assert!(
        !request.contains("/loop 5m"),
        "人敲的那一行不进请求：{request}"
    );
    let log = home.log(handle.id());
    let first = log
        .iter()
        .find_map(|event| match &event.body {
            Body::MessageUser(message) if matches!(event.by, By::Schedule(_)) => {
                Some(message.clone())
            }
            _ => None,
        })
        .expect("第一句记下了");
    assert_eq!(first.typed(), Some("/loop 5m 看看 CI"), "带着人敲的那一行");
    assert_eq!(
        restore(&handle, "r1").await,
        Outcome::Rejected {
            reason: Reason::NothingToUnrevert,
        },
        "开了一轮：撤回恢复不了"
    );
    stop(&handle).await;
}

/// 固定间隔的：撤回期间到点两次，日志里一条都没多；处理了补交一次、只补一次；补交以后下一次到点照常。
async fn fixed(how: Settle) {
    let home = Home::new();
    let models = script([
        Play::Says("第一次。"),
        Play::Says("在。"),
        Play::Says("补交的。"),
        Play::Says("下一次。"),
    ]);
    let handle = home.create_with(&models, &catalog()).await;
    let mut subscription = watch(&handle).await;
    within(
        "开循环",
        handle.start_loop(Some(60), "ping".to_string(), typed("1m ping")),
    )
    .await
    .expect("会话在跑");
    turn(&mut subscription).await;
    undo(&handle, "u1").await;
    let undone = home.log(handle.id()).len();
    due_while_undone(&handle, &mut subscription, 61).await;
    due_while_undone(&handle, &mut subscription, 61).await;
    assert_eq!(
        home.log(handle.id()).len(),
        undone,
        "撤回期间到点两次，日志里一条都没多：{:?}",
        kinds(&home.log(handle.id())[undone..])
    );
    settle(&handle, &mut subscription, how).await;
    let log = home.log(handle.id());
    assert!(owed_after(&log, how), "处理了撤回才补交：{:?}", kinds(&log));
    assert_eq!(
        fired(&log),
        [
            (Some(ScheduleLoop::Fixed), "ping".to_string()),
            (Some(ScheduleLoop::Fixed), "ping".to_string()),
        ],
        "到点两次只补交一次"
    );
    let looping = handle.looping().expect("循环开着");
    assert!(!looping.held, "不再搁着");
    skip(61).await;
    turn(&mut subscription).await;
    assert_eq!(fired(&home.log(handle.id())).len(), 3, "下一次到点照常");
    stop(&handle).await;
}

#[tokio::test]
async fn a_fixed_loop_owes_one_run_until_the_person_speaks() {
    fixed(Settle::Say).await;
}

#[tokio::test]
async fn a_fixed_loop_owes_one_run_until_the_undo_is_restored() {
    fixed(Settle::Restore).await;
}

/// 她自己定节奏的：撤回期间到点不写；处理了补交一次。
async fn dynamic(how: Settle) {
    let home = Home::new();
    let wakeup = r#"{"delay_seconds":600,"prompt":"盯 CI","reason":"CI 大约十分钟"}"#;
    let models = script([
        Play::calls(&[("schedule_wakeup", wakeup)]),
        Play::Says("十分钟以后再看。"),
        Play::Says("在。"),
        Play::Says("CI 绿了。"),
    ]);
    let handle = home.create_with(&models, &catalog()).await;
    let mut subscription = watch(&handle).await;
    within(
        "开循环",
        handle.start_loop(None, "盯 CI".to_string(), typed("盯 CI")),
    )
    .await
    .expect("会话在跑");
    turn(&mut subscription).await;
    undo(&handle, "u1").await;
    let undone = home.log(handle.id()).len();
    let held = due_while_undone(&handle, &mut subscription, 601).await;
    assert_eq!(held.next, None, "搁着，没有下一次");
    assert_eq!(home.log(handle.id()).len(), undone, "到点不写");
    settle(&handle, &mut subscription, how).await;
    let log = home.log(handle.id());
    assert!(owed_after(&log, how), "处理了撤回才补交：{:?}", kinds(&log));
    assert_eq!(
        fired(&log),
        [
            (Some(ScheduleLoop::Dynamic), "盯 CI".to_string()),
            (Some(ScheduleLoop::Dynamic), "盯 CI".to_string()),
        ]
    );
    // 补交的那一轮她没再定下一次：循环结束（LM-44），不搁着、也不挂着。
    within("循环结束", async {
        while handle.looping().is_some() {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await;
    stop(&handle).await;
}

#[tokio::test]
async fn a_self_paced_loop_owes_one_run_until_the_person_speaks() {
    dynamic(Settle::Say).await;
}

#[tokio::test]
async fn a_self_paced_loop_owes_one_run_until_the_undo_is_restored() {
    dynamic(Settle::Restore).await;
}

/// 到点继续的：撤回期间到点不写、不丢；处理了补交一次，交过就没了。
async fn once(how: Settle) {
    let home = Home::new();
    let later = r#"{"delay_seconds":600,"prompt":"看看 CI 过了没有","reason":"CI 大约十分钟"}"#;
    let models = script([
        Play::calls(&[("schedule_wakeup", later)]),
        Play::Says("十分钟以后我再看。"),
        Play::Says("在。"),
        Play::Says("CI 过了。"),
    ]);
    let handle = home.create_with(&models, &catalog()).await;
    let mut subscription = watch(&handle).await;
    ask(&handle, "m1", say("CI 好了告诉我"))
        .await
        .expect("会话在跑");
    turn(&mut subscription).await;
    undo(&handle, "u1").await;
    let undone = home.log(handle.id()).len();
    due_while_undone(&handle, &mut subscription, 601).await;
    assert_eq!(home.log(handle.id()).len(), undone, "到点不写");
    settle(&handle, &mut subscription, how).await;
    let log = home.log(handle.id());
    assert!(owed_after(&log, how), "处理了撤回才补交：{:?}", kinds(&log));
    assert_eq!(fired(&log), [(None, "看看 CI 过了没有".to_string())]);
    assert_eq!(handle.looping(), None, "交过就没了");
    stop(&handle).await;
}

#[tokio::test]
async fn a_wakeup_owes_its_run_until_the_person_speaks() {
    once(Settle::Say).await;
}

#[tokio::test]
async fn a_wakeup_owes_its_run_until_the_undo_is_restored() {
    once(Settle::Restore).await;
}
