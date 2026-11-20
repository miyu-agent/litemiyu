//! 后台命令在会话里（施工 7-3，`docs/blueprint/agents.md` 第四条、第八条）：真的 `shell` 放到后台，调用当场结束、命令接着
//! 跑，输出写进 `jobs/<编号>.out`，结束了记 `job.reported`（`by` 是起它的那次调用，`cause` 是那一轮的，输出存成 blob），闲着
//! 的她被叫醒；编号照日志往后数，撤掉的回合里用过的不再用；有计划地停下先记 `restarted`、落了盘再杀；没记就停了的，
//! 再载入时补 `aborted`。假的后台命令用 `lm_engine::tool::testkit::Held`，真的 `shell` 跑无害的短命令。子会话领的号带前缀的那两条
//! （Miyu 的 `a_grandchild_numbers_…`、`an_old_child_log_…`）跟子代理在 L5 二搬。

use std::sync::{Arc, Mutex};
use std::time::Duration;

use lm_engine::session::testkit::{Play, Script};
use lm_engine::store::blob::Blobs;
use lm_engine::tool::testkit::{Act, Fake, Held};
use lm_engine::tool::{Catalog, Exit, Tool};
use lm_kernel::kernel::event::{Body, Event, JobReason};
use lm_kernel::kernel::id::{JobId, TurnId};
use lm_kernel::kernel::origin::{By, Tool as ByTool};
use lm_kernel::kernel::session::Command;
use lm_kernel::kernel::tool::Access;

use crate::support::jobs::*;
use crate::support::*;

/// 只有一件假工具 `start`：把 `held` 交给任务端口。
fn starting(held: &Arc<Held>) -> Catalog {
    let tool = Fake::new("start", Access::Read, Act::Background(Arc::clone(held)));
    Catalog::new([tool as Arc<dyn Tool>]).expect("合写法")
}

#[tokio::test]
async fn a_background_command_ends_on_its_own_and_wakes_her() {
    let home = Home::outside_temp();
    let command = "sleep 1; echo hello; exit 2";
    let args = serde_json::json!({ "command": command, "description": "Say hello", "run_in_background": true })
        .to_string();
    let script = Script::new([
        Play::calls(&[("shell", args.as_str())]),
        Play::Says("放出去了。"),
        Play::Says("看到了。"),
    ]);
    let handle = home.create_as(&script, &base_system(), full(&home)).await;
    talk(&handle, "cmd-1", "后台跑一下").await;
    assert!(
        reports(&home.log(handle.id())).is_empty(),
        "这一轮结束了，命令还在跑"
    );
    let log = until_logged(&home, handle.id(), |log| {
        log.iter()
            .filter(|event| matches!(event.body, Body::TurnEnded(_)))
            .count()
            == 2
    })
    .await;
    let started = results(&log)[0];
    assert!(
        started.effects.iter().any(|effect| matches!(effect, lm_kernel::kernel::event::Effect::JobStarted(job) if job.job == JobId::new(1).unwrap() && job.title == "Say hello")),
        "{started:?}"
    );
    let [(event, reported)] = reports(&log)[..] else {
        panic!("记了一条结束：{log:#?}")
    };
    assert_eq!(reported.reason, JobReason::Exited);
    assert_eq!(reported.exit_code, Some(2));
    assert_eq!(
        event.by,
        By::Tool(ByTool {
            call_id: started.call_id
        }),
        "起它的那次调用"
    );
    assert_eq!(event.cause, Some(id("cmd-1")), "那一轮的 cause");
    assert_eq!(event.turn, None, "回报不带回合编号");
    let output = reported.output.clone().expect("存了输出");
    let blobs = Blobs::new(home.root.blobs());
    let text = String::from_utf8(blobs.get(&output).expect("blob 在")).expect("是 UTF-8");
    assert_eq!(text, "hello\n");
    assert_eq!(reported.chars, Some(6));
    let file = lm_engine::store::jobs::output_path(
        &home.root.session(handle.id()),
        &JobId::new(1).unwrap(),
    );
    assert_eq!(
        std::fs::read_to_string(file).expect("输出文件在"),
        "hello\n"
    );
    let woke = log
        .iter()
        .find_map(|later| match &later.body {
            Body::TurnStarted(started) if later.seq > event.seq => Some(started.trigger),
            _ => None,
        })
        .expect("开了一轮");
    assert_eq!(woke, Some(event.seq), "闲着的她由它叫醒");
}

#[tokio::test]
async fn ids_count_on_after_an_undo_and_a_reload() {
    let home = Home::new();
    let first = Held::new(&[]);
    let script = Script::new([
        Play::calls(&[("start", "{}")]),
        Play::Says("放出去了。"),
        Play::Says("结束了。"),
    ]);
    let handle = home.create_with(&script, &starting(&first)).await;
    talk(&handle, "cmd-1", "放一个").await;
    first.end(Exit::Code(0));
    // 结束了叫醒她说一句：等那一轮也结束。
    let log = until_logged(&home, handle.id(), |log| {
        log.iter()
            .filter(|event| matches!(event.body, Body::TurnEnded(_)))
            .count()
            == 2
    })
    .await;
    let turn = log
        .iter()
        .find(|event| matches!(event.body, Body::TurnStarted(_)))
        .map(|event| TurnId::new(event.seq))
        .expect("开过一轮");
    ask(&handle, "cmd-2", Command::Revert { turn: Some(turn) })
        .await
        .expect("会话在跑");
    let session = handle.id().clone();
    stop(&handle).await;
    drop(handle);
    let second = Held::new(&[]);
    let script = Script::new([Play::calls(&[("start", "{}")]), Play::Says("又放了一个。")]);
    let handle = home.load_with(&session, &script, &starting(&second)).await;
    talk(&handle, "cmd-3", "再放一个").await;
    let log = home.log(handle.id());
    let last = results(&log).last().copied().expect("有结果");
    assert!(
        matches!(&last.effects[..], [lm_kernel::kernel::event::Effect::JobStarted(job)] if job.job == JobId::new(2).unwrap()),
        "撤掉的那一轮用过 j1，不再用：{last:?}"
    );
    second.end(Exit::Code(0));
}

#[tokio::test]
async fn a_planned_stop_records_restarted_before_it_kills() {
    let home = Home::new();
    let held = Held::new(&["half\n"]);
    let script = Script::new([Play::calls(&[("start", "{}")]), Play::Says("放出去了。")]);
    let handle = home.create_with(&script, &starting(&held)).await;
    talk(&handle, "cmd-1", "放一个").await;
    // 输出是另一头读的：等它写进文件再停，不然机器忙时停下那一刻还没读到，数出来是 0（2026-10-12 记忆的会话跑门禁撞上过）。
    let file = lm_engine::store::jobs::output_path(
        &home.root.session(handle.id()),
        &JobId::parse(&last_started(&home.log(handle.id()))).expect("合写法"),
    );
    within("输出写进文件", async {
        while std::fs::read_to_string(&file).unwrap_or_default() != "half\n" {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await;
    let seen_at_kill = Arc::new(Mutex::new(None));
    let (root, seen) = (home.root.session(handle.id()), Arc::clone(&seen_at_kill));
    held.on_kill(move || {
        let log = lm_engine::store::log::read_events(&root).expect("日志读得出");
        let recorded = reports(&log)
            .iter()
            .any(|(_, reported)| reported.reason == JobReason::Restarted);
        *seen.lock().unwrap() = Some(recorded);
    });
    stop(&handle).await;
    assert_eq!(held.killed(), 1, "停下时整组杀掉");
    assert_eq!(
        *seen_at_kill.lock().unwrap(),
        Some(true),
        "杀的时候 restarted 已经落了盘"
    );
    let log = home.log(handle.id());
    let [(event, reported)] = reports(&log)[..] else {
        panic!("记了一条结束：{log:#?}")
    };
    assert_eq!(reported.reason, JobReason::Restarted);
    assert_eq!(event.by, By::Kernel);
    assert_eq!(event.cause, None);
    assert!(reported.duration_ms.is_some());
    assert_eq!(reported.chars, Some(5), "到这时的输出");
}

#[tokio::test]
async fn a_session_that_stops_without_a_record_gets_aborted_on_load() {
    let home = Home::new();
    let held = Held::new(&[]);
    let script = Script::new([Play::calls(&[("start", "{}")]), Play::Says("放出去了。")]);
    let handle = home.create_with(&script, &starting(&held)).await;
    let session = handle.id().clone();
    talk(&handle, "cmd-1", "放一个").await;
    // 没人拿着了：actor 停下，整组杀掉，什么都不记，和核心崩了一样。
    drop(handle);
    within("杀掉", async {
        while held.killed() == 0 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await;
    assert!(
        reports(&home.log(&session)).is_empty(),
        "停的时候什么都没记"
    );
    let handle = home.load(&session, &Script::new([])).await;
    let log = until_logged(&home, &session, |log| !reports(log).is_empty()).await;
    let [(event, reported)] = reports(&log)[..] else {
        panic!("补了一条：{log:#?}")
    };
    assert_eq!(reported.reason, JobReason::Aborted);
    assert_eq!(event.by, By::Kernel);
    assert_eq!(reported.duration_ms, None, "什么时候没的不知道");
    assert!(
        !log.iter()
            .skip_while(|later| later.seq <= event.seq)
            .any(|later| matches!(later.body, Body::TurnStarted(_))),
        "只记下，不开轮"
    );
    drop(handle);
}

/// 日志里最后一条结果派的任务编号。
fn last_started(log: &[Event]) -> String {
    results(log)
        .iter()
        .rev()
        .flat_map(|result| &result.effects)
        .find_map(|effect| match effect {
            lm_kernel::kernel::event::Effect::JobStarted(started) => Some(started.job.to_string()),
            _ => None,
        })
        .expect("派过任务")
}
