//! 旧日志里的 `glob`、`grep`（2026-11-14 砍了这两件）：表里没有它们的样子、没有它们那一句，照原名写一行，不崩。

use std::time::Instant;

use lm_kernel::kernel::event::Said;
use serde_json::json;

use super::{rows, segment, step, text, thought};
use crate::core::ToolStatus;
use crate::transcript::{StepKind, ToolState};
use crate::ui::test_support::Fixture;

#[test]
fn old_glob_and_grep_steps_are_named_as_they_were_called() {
    let f = Fixture::new();
    let t0 = Instant::now();
    let glob = step(
        StepKind::Tool {
            name: "glob".into(),
            args: r#"{"pattern":"**/*.rs"}"#.into(),
            parsed: json!({"pattern": "**/*.rs"}),
            state: ToolState::Done(ToolStatus::Ok),
            output: "src/main.rs\n".into(),
            said: Some(Said {
                key: "software/basesystem/glob/files".into(),
                fields: [("count".to_string(), "1".to_string())].into(),
            }),
        },
        t0,
        1,
        1,
    );
    let grep = step(
        StepKind::Tool {
            name: "grep".into(),
            args: r#"{"pattern":"TODO"}"#.into(),
            parsed: json!({"pattern": "TODO"}),
            state: ToolState::Done(ToolStatus::Ok),
            output: "src/main.rs\n".into(),
            said: Some(Said {
                key: "software/basesystem/grep/files".into(),
                fields: [("count".to_string(), "1".to_string())].into(),
            }),
        },
        t0,
        2,
        1,
    );
    // 收起的一行照常数步数。
    let folded = segment(vec![thought(t0, 0), glob.clone(), grep.clone()], None);
    assert_eq!(
        text(&rows(0, &folded, &f.ctx())),
        ["  Used 2 tools · 1 thought · 3s"]
    );
    // 铺开的一步一行，照原名写。
    let open = segment(vec![thought(t0, 0), glob, grep], Some(true));
    let shown = text(&rows(0, &open, &f.ctx()));
    assert!(shown.iter().any(|l| l.contains("glob")), "{shown:?}");
    assert!(shown.iter().any(|l| l.contains("grep")), "{shown:?}");
}
