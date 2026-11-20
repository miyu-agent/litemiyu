//! 旧日志里的 `web_search`、`web_fetch`、`trash`（LM-2 砍了这三件）：时间线不再单算「搜索了几次网络」「抓取了几个网页」，
//! 表里没有它们的样子，照原名写一行、算进「用了几个工具」，不崩。

use std::time::Instant;

use serde_json::json;

use super::{rows, segment, step, text, thought};
use crate::core::ToolStatus;
use crate::transcript::{Step, StepKind, ToolState};
use crate::ui::test_support::Fixture;

fn old(name: &str, parsed: serde_json::Value, start: u64) -> Step {
    step(
        StepKind::Tool {
            name: name.into(),
            args: parsed.to_string(),
            parsed,
            state: ToolState::Done(ToolStatus::Ok),
            output: "ok".into(),
            said: None,
        },
        Instant::now(),
        start,
        1,
    )
}

#[test]
fn old_web_and_trash_steps_are_plain_tools_named_as_called() {
    let f = Fixture::new();
    let t0 = Instant::now();
    let steps = vec![
        thought(t0, 0),
        old("web_search", json!({"query": "rust"}), 1),
        old("web_fetch", json!({"url": "https://a.example"}), 2),
        old("trash", json!({"file_path": "old.txt"}), 3),
    ];
    let folded = segment(steps.clone(), None);
    assert_eq!(
        text(&rows(0, &folded, &f.ctx())),
        ["  Used 3 tools · 1 thought · 4s"]
    );
    let open = segment(steps, Some(true));
    let shown = text(&rows(0, &open, &f.ctx()));
    for name in ["web_search", "web_fetch", "trash"] {
        assert!(shown.iter().any(|l| l.contains(name)), "{name}: {shown:?}");
    }
}
