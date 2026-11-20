//! 旧日志里的 `show`（2026-11-14 砍了 `show`）：表里没有它的样子、没有它那一句，照原名写一行，不崩。

use std::time::Instant;

use lm_kernel::kernel::event::Said;
use serde_json::json;

use super::{rows, segment, step, text, thought};
use crate::core::ToolStatus;
use crate::transcript::{StepKind, ToolState};
use crate::ui::test_support::Fixture;

#[test]
fn an_old_show_step_is_named_as_it_was_called() {
    let f = Fixture::new();
    let t0 = Instant::now();
    let call = step(
        StepKind::Tool {
            name: "show".into(),
            args: r#"{"path":"pic.png"}"#.into(),
            parsed: json!({"path": "pic.png"}),
            state: ToolState::Done(ToolStatus::Ok),
            output: "Shown to the user.".into(),
            said: Some(Said {
                key: "software/basesystem/show/shown".into(),
                fields: Default::default(),
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
    assert!(shown.iter().any(|l| l.contains("show")), "{shown:?}");
}
