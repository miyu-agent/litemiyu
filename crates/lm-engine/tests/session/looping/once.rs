//! 到点继续（LM-20）：没开 `/loop` 的会话里她调 `schedule_wakeup` 并写了 `prompt`，会话挂上一个到点继续；到点照定时任务的
//! 标签交进那句话开一轮，交过就没了，那一轮里她再调一次才有下一次。没写 `prompt` 的说缺；再调一次换掉原来的；`stop`、
//! `/loop off` 停得掉；打断那一轮不动挂着的；到点时正在跑一轮的，那句话排着、这一轮的下一次请求带上；重开时还挂着的写一行；子会话、没人看着的一次运行
//! 里不给；人开着固定间隔的循环时照旧拒。
//!
//! 不真等：同 `looping.rs`，到点前把 tokio 的钟拨过去。

use std::sync::Arc;

use lm_engine::session::Pace;
use lm_engine::session::testkit::{Play, Script};
use lm_engine::tool::{Catalog, Tool};
use lm_kernel::kernel::event::{Body, ToolStatus};
use lm_kernel::kernel::request::{Message, Request};
use lm_kernel::kernel::session::Command;

use super::{catalog, fired, results, script, skip, stopped_lines, turn};
use crate::support::jobs::full;
use crate::support::*;

/// 定十分钟以后接着看 CI。
const LATER: &str = r#"{"delay_seconds":600,"prompt":"看看 CI 过了没有","reason":"CI 大约十分钟"}"#;

/// 用不了的那一句（`software/loop/common/off.txt`）。
const OFF: &str = "Loops are not available in this session.";

/// 没有在跑的循环（`software/loop/wakeup/no-loop.txt`）。
const NO_LOOP: &str = "No loop is running in this session.";

/// 人说一句，等这一轮结束：交回这一轮的工具结果。
async fn said(
    handle: &lm_engine::session::Handle,
    subscription: &mut lm_engine::session::Subscription,
    id: &str,
    words: &str,
) -> Vec<(bool, String)> {
    ask(handle, id, say(words)).await.expect("会话在跑");
    results(&turn(subscription).await)
}

#[tokio::test]
async fn without_a_loop_she_schedules_one_continuation_and_it_fires_once() {
    let home = Home::new();
    let models = script([
        Play::calls(&[("schedule_wakeup", LATER)]),
        Play::Says("十分钟以后我再看。"),
        // 到点开的一轮：她不再定。
        Play::Says("CI 过了。"),
    ]);
    let handle = home.create_with(&models, &catalog()).await;
    let mut subscription = watch(&handle).await;
    let first = said(&handle, &mut subscription, "m1", "CI 好了告诉我").await;
    assert!(
        first.len() == 1 && first[0].0 && first[0].1.starts_with("Next iteration at "),
        "没开循环也定上了：{first:?}"
    );
    let waiting = handle.looping().expect("挂着一个到点继续");
    assert_eq!(waiting.pace, Pace::Dynamic);
    assert_eq!(waiting.prompt, "看看 CI 过了没有");
    assert!(waiting.next.is_some(), "界面看得到什么时候");
    assert_eq!(models.requests().len(), 2, "到点以前不开轮");
    skip(601).await;
    turn(&mut subscription).await;
    let log = home.log(handle.id());
    assert_eq!(
        fired(&log),
        [(None, "看看 CI 过了没有".to_string())],
        "到点交进来一次，不算循环"
    );
    let woken = format!("{:?}", models.requests().last().expect("请求过").1);
    assert!(
        woken.contains("<scheduled-task id=") && !woken.contains("loop-wakeup"),
        "到点交进来的话包着定时任务的标签：{woken}"
    );
    assert_eq!(handle.looping(), None, "交过就没了");
    skip(3_600).await;
    // 之后拨过一小时也不再开轮：人接着说的一句开的是下一轮。
    let script_left = models.requests().len();
    assert_eq!(script_left, 3, "只开了到点的那一轮");
    assert_eq!(fired(&home.log(handle.id())).len(), 1);
}

#[tokio::test]
async fn she_schedules_again_inside_the_continued_turn() {
    let home = Home::new();
    let models = script([
        Play::calls(&[("schedule_wakeup", LATER)]),
        Play::Says("十分钟以后我再看。"),
        // 到点开的一轮：还没好，再定一次。
        Play::calls(&[("schedule_wakeup", LATER)]),
        Play::Says("还在跑，再等十分钟。"),
        Play::Says("CI 过了。"),
    ]);
    let handle = home.create_with(&models, &catalog()).await;
    let mut subscription = watch(&handle).await;
    said(&handle, &mut subscription, "m1", "CI 好了告诉我").await;
    let first = handle.looping().expect("挂着").id;
    skip(601).await;
    let again = results(&turn(&mut subscription).await);
    assert!(
        again.len() == 1 && again[0].0,
        "那一轮里再定成了：{again:?}"
    );
    let second = handle.looping().expect("又挂上了").id;
    assert_ne!(first, second, "是新的一个");
    skip(601).await;
    turn(&mut subscription).await;
    assert_eq!(fired(&home.log(handle.id())).len(), 2);
    assert_eq!(handle.looping(), None);
}

#[tokio::test]
async fn without_a_prompt_she_is_told_it_is_required() {
    let home = Home::new();
    let models = script([
        Play::calls(&[("schedule_wakeup", r#"{"delay_seconds":60}"#)]),
        Play::Says("要写 prompt。"),
    ]);
    let handle = home.create_with(&models, &catalog()).await;
    let mut subscription = watch(&handle).await;
    let results = said(&handle, &mut subscription, "m1", "一分钟以后再看").await;
    assert_eq!(
        results,
        [(
            false,
            "prompt is required when no loop is running in this session.".to_string()
        )]
    );
    assert_eq!(handle.looping(), None, "没定上");
}

#[tokio::test]
async fn calling_again_replaces_the_pending_one_and_stop_cancels_it() {
    let home = Home::new();
    let models = script([
        Play::calls(&[("schedule_wakeup", LATER)]),
        Play::Says("定了。"),
        // 再调一次：换成二十分钟、另一句话。
        Play::calls(&[(
            "schedule_wakeup",
            r#"{"delay_seconds":1200,"prompt":"看看部署"}"#,
        )]),
        Play::Says("改了。"),
        Play::Says("部署好了。"),
        // 再定一个，又停掉；没有挂着的再停，说没有。
        Play::calls(&[("schedule_wakeup", LATER)]),
        Play::calls(&[("schedule_wakeup", r#"{"stop":true}"#)]),
        Play::calls(&[("schedule_wakeup", r#"{"stop":true}"#)]),
        Play::Says("停了。"),
    ]);
    let handle = home.create_with(&models, &catalog()).await;
    let mut subscription = watch(&handle).await;
    said(&handle, &mut subscription, "m1", "CI 好了告诉我").await;
    said(&handle, &mut subscription, "m2", "改成看部署").await;
    assert_eq!(
        handle.looping().map(|looping| looping.prompt),
        Some("看看部署".to_string())
    );
    skip(601).await;
    assert_eq!(
        fired(&home.log(handle.id())),
        [],
        "原来十分钟的那个换掉了，不到点"
    );
    skip(600).await;
    turn(&mut subscription).await;
    assert_eq!(
        fired(&home.log(handle.id())),
        [(None, "看看部署".to_string())]
    );
    let stopped = said(&handle, &mut subscription, "m3", "再定一个然后停掉").await;
    assert_eq!(
        stopped
            .iter()
            .map(|(ok, text)| (*ok, text.as_str()))
            .collect::<Vec<_>>()[1..],
        [(true, "Loop stopped."), (false, NO_LOOP)]
    );
    assert_eq!(handle.looping(), None);
    skip(3_600).await;
    assert_eq!(fired(&home.log(handle.id())).len(), 1, "停掉的不到点");
}

#[tokio::test]
async fn the_person_stops_it_with_loop_off() {
    let home = Home::new();
    let models = script([
        Play::calls(&[("schedule_wakeup", LATER)]),
        Play::Says("定了。"),
    ]);
    let handle = home.create_with(&models, &catalog()).await;
    let mut subscription = watch(&handle).await;
    said(&handle, &mut subscription, "m1", "CI 好了告诉我").await;
    assert!(
        within("停", handle.stop_loop()).await.expect("会话在跑"),
        "停掉了一个"
    );
    assert_eq!(handle.looping(), None);
    skip(3_600).await;
    assert_eq!(fired(&home.log(handle.id())), []);
    assert_eq!(models.requests().len(), 2);
}

#[tokio::test]
async fn interrupting_the_turn_keeps_the_pending_one() {
    let home = Home::new();
    let models = script([
        Play::calls(&[("schedule_wakeup", LATER)]),
        // 定完接着说的那一次停住：人按 Esc 打断。
        Play::Holds,
        Play::Says("CI 过了。"),
    ]);
    let handle = home.create_with(&models, &catalog()).await;
    let mut subscription = watch(&handle).await;
    ask(&handle, "m1", say("CI 好了告诉我"))
        .await
        .expect("会话在跑");
    until_logged(&home, handle.id(), |log| {
        log.iter()
            .any(|event| matches!(event.body, Body::ToolResult(_)))
    })
    .await;
    until_requested(&models, 2).await;
    ask(
        &handle,
        "stop-1",
        Command::Interrupt {
            queued: lm_kernel::kernel::session::Queued::Return,
        },
    )
    .await
    .expect("会话在跑");
    turn(&mut subscription).await;
    assert!(handle.looping().is_some(), "打断那一轮，挂着的还在");
    skip(601).await;
    turn(&mut subscription).await;
    assert_eq!(fired(&home.log(handle.id())).len(), 1, "到点照样继续");
}

/// 等到模型被请求过 `count` 次。
async fn until_requested(models: &Script, count: usize) {
    within("等请求", async {
        while models.requests().len() < count {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await;
}

#[tokio::test]
async fn due_while_a_turn_runs_it_joins_that_turn() {
    let home = Home::outside_temp();
    let models = script([
        Play::calls(&[("schedule_wakeup", LATER)]),
        Play::Says("定了。"),
        // 人接着问的一轮跑一条三秒的命令（真的进程，不跟着 tokio 的钟）：到点时它还在跑。
        Play::calls(&[(
            "shell",
            r#"{"command":"python3 -c 'import time; time.sleep(3)'; echo done","description":"Wait"}"#,
        )]),
        Play::Says("命令跑完了，CI 也过了。"),
    ]);
    let opening = Opening {
        cwd: home.scratch.0.join("work").to_string_lossy().into_owned(),
        ..Opening::default()
    };
    let handle = home.create_as(&models, &everything(), opening).await;
    let mut subscription = watch(&handle).await;
    said(&handle, &mut subscription, "m1", "CI 好了告诉我").await;
    // 闲着的时候先把钟拨到差一秒到点，再让人问一句。
    skip(599).await;
    assert_eq!(models.requests().len(), 2, "还没到点");
    ask(&handle, "m2", say("顺便跑个命令"))
        .await
        .expect("会话在跑");
    until_requested(&models, 3).await;
    tokio::time::sleep(std::time::Duration::from_millis(1_500)).await;
    assert_eq!(models.requests().len(), 3, "到点了，那一轮还在跑，不另开");
    assert_eq!(handle.looping(), None, "到点交过了，排着");
    turn(&mut subscription).await;
    let log = until_logged(&home, handle.id(), |log| fired(log).len() == 1).await;
    // 照「回合中途到的话」收（同循环到点）：命令跑完以后的那一次请求带上它，不另开一轮。
    let kinds = kinds(&log);
    let at = |kind: &str| kinds.iter().rposition(|k| *k == kind).expect("有这一条");
    assert!(
        at("tool.result") > at("turn.started") && at("message.user") > at("turn.started"),
        "到点的那一句记在这一轮里：{kinds:?}"
    );
    let requests = models.requests();
    assert_eq!(requests.len(), 4, "没有另开一轮");
    let last = format!("{:?}", requests[3].1.messages);
    assert!(
        last.contains("<scheduled-task id="),
        "命令跑完以后的那一次请求带着到点的那一句：{last}"
    );
}

#[tokio::test]
async fn reopening_tells_once_when_one_was_still_pending() {
    let home = Home::new();
    let models = script([
        Play::calls(&[("schedule_wakeup", LATER)]),
        Play::Says("定了。"),
        Play::Says("在。"),
    ]);
    let handle = home.create_with(&models, &catalog()).await;
    let mut subscription = watch(&handle).await;
    said(&handle, &mut subscription, "m1", "CI 好了告诉我").await;
    let session = handle.id().clone();
    stop(&handle).await;
    // 关了以后重开：没有挂着的了，写一行告诉人。
    let handle = home.load_with(&session, &models, &catalog()).await;
    until_logged(&home, &session, |log| stopped_lines(log) == 1).await;
    assert_eq!(handle.looping(), None, "程序关了不会到点");
    skip(3_600).await;
    assert_eq!(fired(&home.log(&session)), []);
    stop(&handle).await;
    let handle = home.load_with(&session, &models, &catalog()).await;
    let mut subscription = watch(&handle).await;
    said(&handle, &mut subscription, "m2", "在吗").await;
    assert_eq!(stopped_lines(&home.log(&session)), 1, "只写一次");
}

#[tokio::test]
async fn reopening_says_nothing_after_it_fired() {
    let home = Home::new();
    let models = script([
        Play::calls(&[("schedule_wakeup", LATER)]),
        Play::Says("定了。"),
        Play::Says("CI 过了。"),
        Play::Says("在。"),
    ]);
    let handle = home.create_with(&models, &catalog()).await;
    let mut subscription = watch(&handle).await;
    said(&handle, &mut subscription, "m1", "CI 好了告诉我").await;
    skip(601).await;
    turn(&mut subscription).await;
    let session = handle.id().clone();
    stop(&handle).await;
    let handle = home.load_with(&session, &models, &catalog()).await;
    let mut subscription = watch(&handle).await;
    said(&handle, &mut subscription, "m2", "在吗").await;
    assert_eq!(
        stopped_lines(&home.log(&session)),
        0,
        "已经到过点、没有挂着的，重开不写那一行"
    );
}

#[tokio::test]
async fn a_one_shot_run_with_no_one_watching_refuses() {
    let home = Home::new();
    let models = script([
        Play::calls(&[("schedule_wakeup", LATER)]),
        Play::Says("这里定不了。"),
    ]);
    let handle = home.create_with(&models, &catalog()).await;
    // `litemiyu ask`：会话照有人的造，再交「这一次没人」。
    handle.unattend().expect("会话在跑");
    let mut subscription = watch(&handle).await;
    let results = said(&handle, &mut subscription, "m1", "CI 好了告诉我").await;
    assert_eq!(results, [(false, OFF.to_string())]);
    assert_eq!(handle.looping(), None);
}

#[tokio::test]
async fn a_fixed_loop_the_person_started_is_left_alone() {
    let home = Home::new();
    let models = script([
        Play::calls(&[("schedule_wakeup", LATER)]),
        Play::Says("人开的是固定间隔的。"),
    ]);
    let handle = home.create_with(&models, &catalog()).await;
    let mut subscription = watch(&handle).await;
    within("开循环", handle.start_loop(Some(300), "ping".to_string()))
        .await
        .expect("会话在跑");
    let results = results(&turn(&mut subscription).await);
    assert_eq!(results, [(false, NO_LOOP.to_string())], "照旧拒");
    assert_eq!(
        handle
            .looping()
            .map(|looping| (looping.pace, looping.prompt)),
        Some((Pace::Fixed { every: 300 }, "ping".to_string())),
        "人开的循环原样"
    );
}

/// 基础系统加循环的工具：派得出子代理。
fn everything() -> Catalog {
    let mut tools: Vec<Arc<dyn Tool>> = lm_engine::basesystem::tools().expect("读得出");
    tools.extend(lm_engine::schedule::tools().expect("读得出"));
    Catalog::new(tools).expect("合写法")
}

/// 普通子代理的 system 末尾接的那一段的开头；fork 的外壳里的一句。
const CHILD: &str = "You are a subagent, started by another session";
const FORK: &str = "You are a fork of the agent above.";

/// 主会话同一步派一个 fork 的、一个不带的；两个子会话都去调 `schedule_wakeup`，拿到结果说完。
fn family(request: &Request) -> Play {
    let child = request.system.contains(CHILD) || format!("{:?}", request.messages).contains(FORK);
    match (child, request.messages.last()) {
        (true, Some(Message::Tool { .. })) => Play::Says("CHILD-DONE"),
        (true, _) => Play::calls(&[("schedule_wakeup", LATER)]),
        (false, Some(Message::Tool { .. })) => Play::Says("派出去了。"),
        (false, Some(message)) if format!("{message:?}").contains("<subagent-report") => {
            Play::Says("看到回报了。")
        }
        (false, _) => Play::calls(&[
            (
                "subagent",
                r#"{"description":"Fork one","prompt":"TASK-F","fork":true}"#,
            ),
            (
                "subagent",
                r#"{"description":"Plain one","prompt":"TASK-P"}"#,
            ),
        ]),
    }
}

#[tokio::test]
async fn subagents_cannot_schedule() {
    let home = Home::outside_temp();
    let models = Script::new([]).answers(family).recording();
    let handle = home.create_as(&models, &everything(), full(&home)).await;
    ask(&handle, "m1", say("派两个")).await.expect("会话在跑");
    // 两个子会话都调过、都拿到了结果。
    let asked = || -> Vec<(bool, Request)> {
        models
            .requests()
            .into_iter()
            .map(|(_, request)| request)
            .filter(|request| {
                matches!(request.messages.last(), Some(Message::Tool { .. }))
                    && (request.system.contains(CHILD)
                        || format!("{:?}", request.messages).contains(FORK))
            })
            .map(|request| (request.system.contains(CHILD), request))
            .collect()
    };
    let mut waited = 0;
    while asked().len() < 2 {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        waited += 1;
        assert!(
            waited < 2_000,
            "二十秒内两个子会话没调完：{:#?}",
            models
                .requests()
                .iter()
                .map(|(_, request)| (
                    request.system.contains(CHILD),
                    format!("{:?}", request.messages.last())
                ))
                .collect::<Vec<_>>()
        );
    }
    let children = asked();
    for (plain, request) in &children {
        let last = format!("{:?}", request.messages.last());
        if *plain {
            // 不带 fork 的子代理：工具面上没有这件。
            assert!(
                !request
                    .tools
                    .iter()
                    .any(|tool| tool.name == "schedule_wakeup"),
                "普通子代理的工具面上不给"
            );
            assert!(!last.contains("Next iteration at"), "没定上：{last}");
        } else {
            // fork 的工具面和主会话一样（共用缓存），调了当场拒。
            assert!(last.contains(OFF), "fork 子代理调了说用不了：{last}");
        }
    }
    assert_eq!(handle.looping(), None, "主会话也没被挂上");
    let failed = home
        .log(handle.id())
        .iter()
        .filter(|event| matches!(&event.body, Body::ToolResult(result) if result.status != ToolStatus::Ok))
        .count();
    assert_eq!(failed, 0, "主会话自己的调用都成了");
}
