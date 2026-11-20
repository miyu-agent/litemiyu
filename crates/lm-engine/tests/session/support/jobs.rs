//! 后台命令的测试共用的（Miyu `miyu-session/tests/support/jobs.rs`，L5 一搬来）：真的基础系统、没人能确认的开场、说一句等这一轮
//! 结束、日志里的回报和工具结果。Miyu 的「等任务表空了」没搬：LiteMiyu 的任务表一个主会话一张，在会话里面，测试看日志、
//! 看替身的命令杀没杀。

use lm_engine::session::Handle;
use lm_engine::tool::Catalog;
use lm_kernel::kernel::event::{Body, Event, JobReported, ToolResult};

use super::*;

/// 真的基础系统。
pub fn base_system() -> Catalog {
    lm_engine::basesystem::catalog().expect("出厂的字读得出来")
}

/// 没人能确认，在场地的 `work/` 里。
pub fn full(home: &Home) -> Opening {
    Opening {
        attended: false,
        cwd: home.scratch.0.join("work").to_string_lossy().into_owned(),
        ..Opening::default()
    }
}

/// 说一句，等这一轮结束。
pub async fn talk(handle: &Handle, id: &str, words: &str) {
    let mut pushes = watch(handle).await;
    ask(handle, id, say(words)).await.expect("会话在跑");
    until_turn_ends(&mut pushes).await;
}

/// 日志里的 `job.reported`，和那一条事件。
pub fn reports(log: &[Event]) -> Vec<(&Event, &JobReported)> {
    log.iter()
        .filter_map(|event| match &event.body {
            Body::JobReported(reported) => Some((event, &**reported)),
            _ => None,
        })
        .collect()
}

/// 日志里的工具结果。
pub fn results(log: &[Event]) -> Vec<&ToolResult> {
    log.iter()
        .filter_map(|event| match &event.body {
            Body::ToolResult(result) => Some(&**result),
            _ => None,
        })
        .collect()
}

/// 日志里有几条 `turn.ended`。
pub fn turns_ended(log: &[Event]) -> usize {
    log.iter()
        .filter(|event| matches!(event.body, Body::TurnEnded(_)))
        .count()
}
