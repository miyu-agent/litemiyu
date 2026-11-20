//! 派子代理这一头的小零件（Miyu `agents/tests.rs` 里留下来的一个；照场所、人格、预设、脚本调工具筛工具面的那几个跟着功能删了）。

use super::*;

/// 结果没人要了时停哪几个（施工 7-5 补）：派出去的子代理，后台命令、没有子会话的不算。
#[test]
fn only_spawned_subagents_count_as_unclaimed() {
    let job = |n: &str| JobId::parse(n).expect("合写法");
    let child = SessionId::parse("01a0d78c-ca52-7d19-8b64-0e3f5a7c2d91").expect("合写法");
    let started = |n: &str, what: JobKind, session: Option<SessionId>| {
        Effect::JobStarted(JobStarted {
            foreground: false,
            fork: false,
            job: job(n),
            what,
            title: "查".to_string(),
            session,
        })
    };
    let effects = [
        started("j1", JobKind::Command, None),
        started("j2", JobKind::Agent, Some(child.clone())),
        started("j3", JobKind::Agent, None),
    ];
    assert_eq!(spawned_in(&effects), [(job("j2"), child)]);
    assert!(spawned_in(&[]).is_empty());
}
