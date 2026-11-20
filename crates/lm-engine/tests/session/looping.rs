//! 循环（L5 四，`docs/00-设计.md` 第五节第 9 条）：真 actor、假模型、真的 `schedule_wakeup`。人开一个她自己定节奏的循环，当场交一次，
//! 她调 `schedule_wakeup` 定下一次，到点会话被叫醒开一轮，那一轮没再调就停着；固定间隔的照间隔交，停掉以后不再叫醒；重开时
//! 关掉之前还开着的写一行 `command.ran`，只写一次；没开着循环的她调了说没有；无功能的一档不认 `/loop`。
//!
//! 不真等：到点前把 tokio 的钟拨过去（[`skip`]），拨完马上放开，别的照真的钟走。

use std::sync::Arc;
use std::time::Duration;

use lm_engine::session::testkit::{Play, Script};
use lm_engine::session::{Pace, Subscription};
use lm_engine::tool::{Catalog, Tool};
use lm_kernel::kernel::block::Block;
use lm_kernel::kernel::event::{Body, Event, ToolStatus};
use lm_kernel::kernel::id::SessionId;
use lm_kernel::kernel::origin::{By, ScheduleLoop};

use crate::support::*;

/// 有真的 `schedule_wakeup` 的目录。
fn catalog() -> Catalog {
    let tools: Vec<Arc<dyn Tool>> = lm_engine::schedule::tools().expect("读得出");
    Catalog::new(tools).expect("合写法")
}

/// 照 `plays` 回。
fn script(plays: impl IntoIterator<Item = Play>) -> Script {
    Script::new(plays)
}

/// 把 tokio 的钟往后拨 `seconds` 秒：到点的定时器当场醒，拨完放开，接着照真的钟走。
async fn skip(seconds: u64) {
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(seconds)).await;
    tokio::time::resume();
}

/// 日志里到点交进来的话：循环的种类、原话，照先后。
fn fired(log: &[Event]) -> Vec<(Option<ScheduleLoop>, String)> {
    log.iter()
        .filter_map(|event| match (&event.by, &event.body) {
            (By::Schedule(schedule), Body::MessageUser(message)) => Some((
                schedule.looping,
                message
                    .blocks
                    .iter()
                    .filter_map(|block| match block {
                        Block::Text(text) => Some(text.text.clone()),
                        _ => None,
                    })
                    .collect(),
            )),
            _ => None,
        })
        .collect()
}

/// 日志里重开时记的那一行（`command.ran`，`loop`，内核记的）有几条。
fn stopped_lines(log: &[Event]) -> usize {
    log.iter()
        .filter(|event| {
            matches!(&event.body, Body::CommandRan(ran) if ran.command == "loop")
                && event.by == By::Kernel
        })
        .count()
}

/// 读推送读到回合结束：交回这一轮落了盘的事件。
async fn turn(subscription: &mut Subscription) -> Vec<Event> {
    until_turn_ends(subscription)
        .await
        .into_iter()
        .filter_map(|pushed| match &*pushed {
            lm_engine::session::Pushed::Events(events) => Some(events.clone()),
            lm_engine::session::Pushed::Transient(_) => None,
        })
        .flatten()
        .collect()
}

/// 这一轮的工具结果：成没成、字。
fn results(events: &[Event]) -> Vec<(bool, String)> {
    events
        .iter()
        .filter_map(|event| match &event.body {
            Body::ToolResult(result) => Some((
                result.status == ToolStatus::Ok,
                result
                    .blocks
                    .iter()
                    .filter_map(|block| match block {
                        Block::Text(text) => Some(text.text.clone()),
                        _ => None,
                    })
                    .collect(),
            )),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn a_self_paced_loop_wakes_when_she_says_and_stops_when_she_does_not() {
    let home = Home::new();
    let wakeup = r#"{"delay_seconds":600,"prompt":"盯 CI","reason":"CI 大约十分钟"}"#;
    let models = script([
        // 开的那一刻交的一轮：她定十分钟以后再看。
        Play::calls(&[("schedule_wakeup", wakeup)]),
        Play::Says("十分钟以后再看。"),
        // 到点叫醒的一轮：她不再定，循环停着。
        Play::Says("CI 绿了。"),
    ]);
    let handle = home.create_with(&models, &catalog()).await;
    assert!(handle.loops(), "工具面上有 schedule_wakeup 的认 /loop");
    let mut subscription = watch(&handle).await;
    let replaced = within("开循环", handle.start_loop(None, "盯 CI".to_string()))
        .await
        .expect("会话在跑");
    assert!(!replaced, "原来没有循环");
    let first = results(&turn(&mut subscription).await);
    assert!(
        first.len() == 1 && first[0].0 && first[0].1.starts_with("Next iteration at "),
        "她定下一次成了：{first:?}"
    );
    let looping = handle.looping().expect("循环开着");
    assert_eq!((looping.id.as_str(), looping.pace), ("t1", Pace::Dynamic));
    assert!(looping.next.is_some(), "她定了下一次");
    skip(601).await;
    turn(&mut subscription).await;
    let log = home.log(handle.id());
    assert_eq!(
        fired(&log),
        [
            (Some(ScheduleLoop::Dynamic), "盯 CI".to_string()),
            (Some(ScheduleLoop::Dynamic), "盯 CI".to_string()),
        ]
    );
    let requests = models.requests();
    let woken = format!("{:?}", requests.last().expect("请求过").1);
    assert!(
        woken.contains("loop-wakeup id=")
            && woken.contains("Call schedule_wakeup to continue the loop."),
        "到点交进来的话包着循环的标签：{woken}"
    );
    assert_eq!(
        handle.looping().and_then(|looping| looping.next),
        None,
        "那一轮没再定，停着等"
    );
}

#[tokio::test]
async fn a_fixed_loop_fires_every_interval_until_stopped() {
    let home = Home::new();
    let models = script([
        Play::Says("第一次。"),
        Play::Says("第二次。"),
        Play::Says("你说的我听到了。"),
    ]);
    let handle = home.create_with(&models, &catalog()).await;
    let mut subscription = watch(&handle).await;
    within("开循环", handle.start_loop(Some(60), "ping".to_string()))
        .await
        .expect("会话在跑");
    turn(&mut subscription).await;
    assert_eq!(
        handle.looping().map(|looping| looping.pace),
        Some(Pace::Fixed { every: 60 })
    );
    skip(61).await;
    turn(&mut subscription).await;
    let stopped = within("停循环", handle.stop_loop())
        .await
        .expect("会话在跑");
    assert!(stopped, "停掉了一个");
    assert_eq!(handle.looping(), None);
    skip(600).await;
    // 停了以后拨过十分钟也不叫醒：人接着说的一句开的是下一轮。
    ask(&handle, "m1", say("还在吗")).await.expect("会话在跑");
    let next = turn(&mut subscription).await;
    assert!(
        next.iter()
            .any(|event| matches!(event.body, Body::MessageUser(_)) && event.by == alice()),
        "下一轮是人说的那一句：{:?}",
        kinds(&next)
    );
    let log = home.log(handle.id());
    assert_eq!(
        fired(&log),
        [
            (Some(ScheduleLoop::Fixed), "ping".to_string()),
            (Some(ScheduleLoop::Fixed), "ping".to_string()),
        ]
    );
    assert!(
        !within("再停", handle.stop_loop()).await.expect("会话在跑"),
        "没有的停不了"
    );
}

/// 时钟停在同一毫秒（系统时间往回拨了）：固定间隔的照样每次都叫醒，不因为两次的时刻一样被当成同一次。
#[tokio::test]
async fn a_fixed_loop_keeps_firing_while_the_clock_stands_still() {
    let home = Home::new();
    let models = script([Play::Says("第一次。"), Play::Says("第二次。")]);
    let session = home
        .stuck(home.create_with(&models, &catalog()).await)
        .await;
    let handle = home.load_with(&session, &models, &catalog()).await;
    let mut subscription = watch(&handle).await;
    within("开循环", handle.start_loop(Some(60), "ping".to_string()))
        .await
        .expect("会话在跑");
    turn(&mut subscription).await;
    skip(61).await;
    turn(&mut subscription).await;
    let log = home.log(&session);
    assert_eq!(
        fired(&log),
        [
            (Some(ScheduleLoop::Fixed), "ping".to_string()),
            (Some(ScheduleLoop::Fixed), "ping".to_string()),
        ]
    );
    assert_eq!(models.requests().len(), 2, "两次各开了一轮");
}

/// 时钟停在同一毫秒：每次重开时循环还开着的，那一行照样每次都写（上一次写的那一行不挡这一次的）。
#[tokio::test]
async fn reopening_tells_each_time_while_the_clock_stands_still() {
    let home = Home::new();
    let models = script([Play::Says("一。"), Play::Says("二。")]);
    let session = home
        .stuck(home.create_with(&models, &catalog()).await)
        .await;
    for (round, lines) in [(1, 1), (2, 2)] {
        let handle = home.load_with(&session, &models, &catalog()).await;
        let mut subscription = watch(&handle).await;
        within(
            "开循环",
            handle.start_loop(Some(300), format!("第 {round} 次")),
        )
        .await
        .expect("会话在跑");
        turn(&mut subscription).await;
        stop(&handle).await;
        let handle = home.load_with(&session, &models, &catalog()).await;
        until_logged(&home, &session, |log| stopped_lines(log) == lines).await;
        stop(&handle).await;
    }
}

/// 开一个固定间隔的循环，等它交过一次，有计划地停下：交回会话编号。
async fn left_running(home: &Home, models: &Script) -> SessionId {
    let handle = home.create_with(models, &catalog()).await;
    let mut subscription = watch(&handle).await;
    within(
        "开循环",
        handle.start_loop(Some(300), "看看邮件".to_string()),
    )
    .await
    .expect("会话在跑");
    turn(&mut subscription).await;
    let session = handle.id().clone();
    stop(&handle).await;
    session
}

#[tokio::test]
async fn reopening_tells_once_that_the_loop_stopped() {
    let home = Home::new();
    let models = script([Play::Says("没有新邮件。"), Play::Says("在。")]);
    let session = left_running(&home, &models).await;
    let handle = home.load_with(&session, &models, &catalog()).await;
    let log = until_logged(&home, &session, |log| stopped_lines(log) == 1).await;
    let line = log
        .iter()
        .find_map(|event| match &event.body {
            Body::CommandRan(ran) if event.by == By::Kernel => Some(ran.text.clone()),
            _ => None,
        })
        .expect("有那一行");
    assert_eq!(line, "/loop off");
    assert_eq!(handle.looping(), None, "重开以后没有循环，要人再 /loop");
    stop(&handle).await;
    // 再重开不再写：人说一句、等它答完，那一行还是一条。
    let handle = home.load_with(&session, &models, &catalog()).await;
    let mut subscription = watch(&handle).await;
    ask(&handle, "m1", say("在吗")).await.expect("会话在跑");
    turn(&mut subscription).await;
    assert_eq!(stopped_lines(&home.log(&session)), 1);
}

#[tokio::test]
async fn she_is_told_when_no_loop_runs_and_a_bare_session_has_no_loops() {
    let home = Home::new();
    let models = script([
        Play::calls(&[("schedule_wakeup", r#"{"delay_seconds":60}"#)]),
        Play::Says("没有循环。"),
    ]);
    let handle = home.create_with(&models, &catalog()).await;
    let mut subscription = watch(&handle).await;
    ask(&handle, "m1", say("一分钟以后再看"))
        .await
        .expect("会话在跑");
    let results = results(&turn(&mut subscription).await);
    assert_eq!(
        results,
        [(false, "No loop is running in this session.".to_string())]
    );
    let bare = home
        .create_as(
            &models,
            &catalog(),
            Opening {
                no_tools: true,
                ..Default::default()
            },
        )
        .await;
    assert!(!bare.loops(), "无功能的一档工具面是空的，不认 /loop");
}
