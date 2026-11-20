//! 等人确认到时自动拒（Miyu 施工 CD-1，照 `miyu-session/tests/confirm_timeout.rs` 搬，配置 `permission.confirm.timeout`）：会话
//! actor 照这一轮的配置把等多久交给内核，内核记请求时写上截止的时刻、交出到时叫醒，actor 起定时的任务，到时送回去。没人确认
//! 的，内核替人拒绝，她拿到那一句接着干；人先答了的照常跑；配 `0` 的一直等；没配的等 5 分钟。
//!
//! Miyu 用一件「装了什么」的假工具（完全放开下每次都问）；LiteMiyu 只有高危命令问人（设计第五节第 6 条），这里照
//! `hazard.rs` 用真的 `shell` 跑 `git push --force`，工作目录写成不存在的目录：放行了也一条都跑不起来。

use std::time::Duration;

use lm_engine::session::Handle;
use lm_engine::session::testkit::{Play, Script};
use lm_engine::tool::Catalog;
use lm_kernel::kernel::event::{Body, Decision, Event, ToolStatus};
use lm_kernel::kernel::origin::By;
use lm_kernel::kernel::session::{Answer, Command};

use crate::support::*;

/// 到时写给她的那一句：出厂的原文。
const WORDS: &str = "Not run: no one answered the approval request in time.";

/// 真的基础系统，`shell` 在里面。
fn tools() -> Catalog {
    lm_engine::basesystem::catalog().expect("编进程序的字读得出来")
}

/// 造一个有人能确认的会话，配置是 `config`（没有的照出厂），让她在不存在的工作目录里强制推送，等请求落了盘。交回会话、剧本。
async fn asked(home: &mut Home, config: Option<&str>) -> (Handle, Script) {
    if let Some(text) = config {
        home.configs = crate::support::routing::configs(text, &[]);
    }
    let args = serde_json::json!({ "command": "git push --force", "description": "Test" });
    let args = args.to_string();
    let script = Script::new([
        Play::calls(&[("shell", args.as_str())]),
        Play::Says("那我换个办法。"),
    ]);
    let opening = Opening {
        attended: true,
        cwd: home
            .scratch
            .0
            .join("nowhere")
            .to_string_lossy()
            .into_owned(),
        ..Opening::default()
    };
    let handle = home.create_as(&script, &tools(), opening).await;
    ask(&handle, "cmd-1", say("推上去"))
        .await
        .expect("会话在跑");
    until_logged(home, handle.id(), |log| {
        kinds(log).contains(&"tool.approval_requested")
    })
    .await;
    (handle, script)
}

/// 日志里的确认请求：记下的时刻到截止的时刻差几毫秒，一直等的没有。
fn deadline_ms(log: &[Event]) -> Option<i64> {
    log.iter().find_map(|event| match &event.body {
        Body::ApprovalRequested(request) => Some(
            request
                .expires_at
                .map(|at| at.unix_millis() - event.at.unix_millis()),
        ),
        _ => None,
    })?
}

/// 日志里的决定：选项、谁定的。
fn decisions(log: &[Event]) -> Vec<(Decision, By)> {
    log.iter()
        .filter_map(|event| match &event.body {
            Body::ApprovalDecided(decided) => Some((decided.decision.clone(), event.by.clone())),
            _ => None,
        })
        .collect()
}

/// 日志里的结果：状态、谁写的、给人看的说法的键。
fn results(log: &[Event]) -> Vec<(ToolStatus, By, Option<String>)> {
    log.iter()
        .filter_map(|event| match &event.body {
            Body::ToolResult(result) => Some((
                result.status.clone(),
                event.by.clone(),
                result.human.as_ref().map(|said| said.key.clone()),
            )),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn left_unanswered_it_is_denied_after_the_timeout_and_she_goes_on() {
    let mut home = Home::new();
    let (handle, script) = asked(&mut home, Some("[permission.confirm]\ntimeout = 1\n")).await;
    assert_eq!(deadline_ms(&home.log(handle.id())), Some(1000));
    let log = until_logged(&home, handle.id(), |log| kinds(log).contains(&"turn.ended")).await;
    stop(&handle).await;
    assert_eq!(
        decisions(&log),
        [(Decision::Deny, By::Kernel)],
        "内核替人拒的"
    );
    assert_eq!(
        results(&log),
        [(
            ToolStatus::Denied,
            By::Kernel,
            Some("core/tool-results/approval-timed-out".to_string())
        )],
        "没跑"
    );
    // 她看到的是那一句，接着说了话。
    let requests = script.requests();
    assert_eq!(requests.len(), 2, "拿到结果又问了一次");
    assert!(
        format!("{:?}", requests[1].1).contains(WORDS),
        "她看得到那一句"
    );
}

#[tokio::test]
async fn an_answer_in_time_runs_it_and_the_deadline_passes_quietly() {
    let mut home = Home::new();
    let (handle, _script) = asked(&mut home, Some("[permission.confirm]\ntimeout = 1\n")).await;
    let call_id = home
        .log(handle.id())
        .iter()
        .find_map(|event| match &event.body {
            Body::ApprovalRequested(request) => Some(request.call_id),
            _ => None,
        })
        .expect("有请求");
    let answer = Answer::Approval {
        decision: Decision::Once,
        reason: None,
        attachments: Vec::new(),
    };
    ask(&handle, "cmd-2", Command::Answer { call_id, answer })
        .await
        .expect("会话在跑");
    until_logged(&home, handle.id(), |log| kinds(log).contains(&"turn.ended")).await;
    // 过了截止的时刻再看：到时送回去了也不理。
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let log = home.log(handle.id());
    stop(&handle).await;
    assert!(
        matches!(
            decisions(&log).as_slice(),
            [(Decision::Once, By::Person(_))]
        ),
        "{:?}",
        decisions(&log)
    );
    let results = results(&log);
    assert_eq!(results.len(), 1, "{results:?}");
    assert_ne!(
        results[0].0,
        ToolStatus::Denied,
        "照常跑了（工作目录不在，跑不起来）"
    );
}

#[tokio::test]
async fn zero_waits_forever_and_unset_waits_five_minutes() {
    let mut home = Home::new();
    let (handle, _script) = asked(&mut home, Some("[permission.confirm]\ntimeout = 0\n")).await;
    assert_eq!(deadline_ms(&home.log(handle.id())), None, "0 是一直等");
    tokio::time::sleep(Duration::from_millis(1500)).await;
    let log = home.log(handle.id());
    stop(&handle).await;
    assert!(decisions(&log).is_empty(), "还在等：{:?}", kinds(&log));
    assert!(!kinds(&log).contains(&"tool.result"));

    let mut home = Home::new();
    let (handle, _) = asked(&mut home, None).await;
    let log = home.log(handle.id());
    stop(&handle).await;
    assert_eq!(deadline_ms(&log), Some(300_000), "出厂 5 分钟");
}
