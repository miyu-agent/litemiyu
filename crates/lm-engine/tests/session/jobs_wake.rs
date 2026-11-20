//! L5 一「做完的样子」（执行计划）：模型经真的 `shell` 把一条命令放到后台，这一轮结束；命令结束时会话被
//! 叫醒开一轮，那一轮的请求里看得到回报（第几号、标题、退出码、输出多少字、末尾几行）。真的基础系统，跑无害的短命令。

use lm_engine::session::testkit::{Play, Script};
use lm_kernel::kernel::block::Block;
use lm_kernel::kernel::request::{Message, Request};

use crate::support::jobs::*;
use crate::support::*;

/// 请求里的全部字，连成一段。
fn words(request: &Request) -> String {
    request
        .messages
        .iter()
        .flat_map(|message| match message {
            Message::User { blocks }
            | Message::Assistant { blocks }
            | Message::Tool { blocks, .. } => blocks.iter(),
        })
        .filter_map(|block| match block {
            Block::Text(text) => Some(text.text.as_str()),
            _ => None,
        })
        .collect()
}

#[tokio::test]
async fn the_woken_turn_sees_the_report() {
    let home = Home::outside_temp();
    let args = serde_json::json!({
        "command": "sleep 0.5; echo done-here; exit 4",
        "description": "Count files",
        "run_in_background": true,
    })
    .to_string();
    let script = Script::new([
        Play::calls(&[("shell", args.as_str())]),
        Play::Says("放出去了。"),
        Play::Says("看到它结束了。"),
    ]);
    let handle = home.create_as(&script, &base_system(), full(&home)).await;
    talk(&handle, "cmd-1", "后台跑一下").await;
    assert_eq!(script.requests().len(), 2, "这一轮问了两次：调工具、说完");
    let started = format!("{:?}", results(&home.log(handle.id()))[0].blocks);
    assert!(
        started.contains("Started j1 in the background."),
        "{started}"
    );
    let log = until_logged(&home, handle.id(), |log| turns_ended(log) == 2).await;
    assert_eq!(reports(&log).len(), 1, "结束报了一条");
    let requests = script.requests();
    assert_eq!(requests.len(), 3, "叫醒开的那一轮问了一次");
    let woken = words(&requests[2].1);
    for part in [
        r#"<command-ended job="j1" title="Count files" reason="exited">"#,
        "Exit code 4.",
        "done-here",
    ] {
        assert!(woken.contains(part), "叫醒的那一轮看得到 {part:?}：{woken}");
    }
    stop(&handle).await;
}
