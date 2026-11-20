//! 旧日志里的 `todowrite`（2026-11-14 砍了 `todowrite`）：表里没有它的样子、没有它那一句，照原名写一行，不崩。

use std::time::Instant;

use lm_kernel::kernel::event::Said;
use serde_json::json;

use super::{rows, segment, step, text, thought};
use crate::core::ToolStatus;
use crate::transcript::{StepKind, ToolState};
use crate::ui::test_support::Fixture;

#[test]
fn an_old_todowrite_step_is_named_as_it_was_called() {
    let f = Fixture::new();
    let t0 = Instant::now();
    let args = r#"{"todos":[{"content":"读代码","status":"completed"},{"content":"写测试","status":"in_progress"}]}"#;
    let call = step(
        StepKind::Tool {
            name: "todowrite".into(),
            args: args.into(),
            parsed: serde_json::from_str(args).unwrap_or(json!({})),
            state: ToolState::Done(ToolStatus::Ok),
            output: "Todo list updated: 1 of 2 done.".into(),
            said: Some(Said {
                key: "software/basesystem/todowrite/updated".into(),
                fields: [("done", "1"), ("total", "2")]
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .into(),
            }),
        },
        t0,
        1,
        1,
    );
    // 收起的一行照常数步数。
    let folded = segment(vec![thought(t0, 0), call.clone()], None);
    assert_eq!(
        text(&rows(0, &folded, &f.ctx())),
        ["  Used 1 tool · 1 thought · 2s"]
    );
    // 铺开的一步一行，照原名写。
    let open = segment(vec![thought(t0, 0), call], Some(true));
    let shown = text(&rows(0, &open, &f.ctx()));
    assert!(shown.iter().any(|l| l.contains("todowrite")), "{shown:?}");
}
