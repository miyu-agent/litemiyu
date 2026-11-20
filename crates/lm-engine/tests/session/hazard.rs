//! 高危命令（施工 5-17，从 Miyu `miyu-session/tests/hazard.rs` 搬来，设计第五节第 6 条）：真的 `shell`。灭顶的直接拒（她拿到那
//! 一句，人看不到确认），删家目录、删 LiteMiyu 的数据目录都是；别的高危问人（说明多 `hazard`，不提规则）；平常的命令不问。
//! LiteMiyu 只有完全放开一级，没有 Miyu 的工作区、只读、沙盒那几条。
//!
//! 这几个测试绝不真跑高危命令：删的目标写成测试自己的临时目录（假的家、临时数据根），工作目录写成不存在的目录——就算拦没
//! 拦住，`shell` 也因为工作目录不在一条都不跑。

use std::path::{Path, PathBuf};

use lm_engine::session::Handle;
use lm_engine::session::testkit::{Play, Script};
use lm_engine::tool::Catalog;
use lm_kernel::kernel::event::{ApprovalRequested, Body, Event, ToolResult, ToolStatus};

use crate::support::*;

/// 真的基础系统，`shell` 在里面。
fn tools() -> Catalog {
    lm_engine::basesystem::catalog().expect("编进程序的字读得出来")
}

/// 让她在工作目录 `cwd` 里跑一次 `command`，等到问人或者有了结果。
async fn ran(home: &Home, cwd: &Path, command: &str) -> Handle {
    let args = serde_json::json!({ "command": command, "description": "Test" }).to_string();
    let script = Script::new([Play::calls(&[("shell", args.as_str())]), Play::Says("好。")]);
    let opening = Opening {
        attended: true,
        cwd: cwd.to_string_lossy().into_owned(),
        ..Opening::default()
    };
    let handle = home.create_as(&script, &tools(), opening).await;
    ask(&handle, "cmd-1", say("hi")).await.expect("会话在跑");
    until_logged(home, handle.id(), |log| {
        let kinds = kinds(log);
        kinds.contains(&"tool.approval_requested") || kinds.contains(&"tool.result")
    })
    .await;
    handle
}

/// 不存在的工作目录：拦漏了也跑不起来。
fn nowhere(home: &Home) -> PathBuf {
    home.scratch.0.join("nowhere")
}

fn requests(log: &[Event]) -> Vec<ApprovalRequested> {
    log.iter()
        .filter_map(|event| match &event.body {
            Body::ApprovalRequested(request) => Some(request.clone()),
            _ => None,
        })
        .collect()
}

fn results(log: &[Event]) -> Vec<ToolResult> {
    log.iter()
        .filter_map(|event| match &event.body {
            Body::ToolResult(result) => Some((**result).clone()),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn wiping_out_home_or_the_data_is_refused() {
    let home = Home::new();
    let data = home.root.data.clone();
    std::fs::create_dir_all(&data).expect("建得了数据目录");
    for (target, reason) in [
        (home.home.clone(), "would delete the home directory"),
        (data.clone(), "would delete LiteMiyu's own data"),
    ] {
        let command = format!("rm -rf '{}'", target.display());
        let handle = ran(&home, &nowhere(&home), &command).await;
        let log = home.log(handle.id());
        stop(&handle).await;
        assert!(requests(&log).is_empty(), "不问：{command}");
        let result = results(&log).remove(0);
        assert_eq!(result.status, ToolStatus::Denied, "{command}");
        let text = format!("{:?}", result.blocks);
        assert!(
            text.contains(&format!(
                "Not run: this command {reason}, which is never allowed."
            )),
            "{text}"
        );
        assert!(target.is_dir(), "没跑：{}", target.display());
    }
}

#[tokio::test]
async fn a_force_push_asks_without_a_rule() {
    let home = Home::new();
    let handle = ran(&home, &nowhere(&home), "git push --force").await;
    let log = home.log(handle.id());
    stop(&handle).await;
    assert!(results(&log).is_empty(), "等人答，没跑");
    let request = requests(&log).remove(0);
    assert_eq!(request.rule, None, "高危的不该一劳永逸");
    let detail = request
        .detail
        .as_ref()
        .map(|detail| detail.get().to_string())
        .unwrap_or_default();
    assert!(detail.contains(r#""hazard":"git_force_push""#), "{detail}");
    assert!(
        detail.contains(r#""command":"git push --force""#),
        "{detail}"
    );
}

#[tokio::test]
async fn ordinary_commands_are_not_held_up() {
    let home = Home::new();
    let work = home.scratch.0.join("work");
    std::fs::create_dir_all(&work).expect("建得了工作区");
    let handle = ran(&home, &work, "echo fine").await;
    let log = home.log(handle.id());
    stop(&handle).await;
    assert!(requests(&log).is_empty(), "不问");
    let result = results(&log).remove(0);
    assert_eq!(result.status, ToolStatus::Ok);
    assert!(format!("{:?}", result.blocks).contains("fine"));
}
