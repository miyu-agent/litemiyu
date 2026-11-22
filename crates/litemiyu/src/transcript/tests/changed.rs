//! 改过的文件的小账（`transcript/changed.rs`，照 Miyu 终端 `changes/tests.rs`）：只算编辑、写入成了的，按文件归在一起、先改的
//! 在前，加减行数照每一步合计；撤销掉的、没成的、命令改的不算，恢复了又算回来；草稿正文不记。

use serde_json::{Value, json};

use super::apply;
use crate::core::{EndReason, Push, ToolStatus};
use crate::transcript::{Changed, Lookup, Transcript};

/// 她调了一件工具、有了结果：块、调用编号、结果。
fn called(call: &str, name: &str, args: &Value, status: ToolStatus) -> Vec<Push> {
    vec![
        Push::BlockStart {
            index: 0,
            block: crate::core::Block::ToolCall(name.into()),
        },
        Push::Delta {
            index: 0,
            text: args.to_string(),
        },
        Push::BlockEnd(0),
        Push::Calls(vec![call.into()]),
        Push::ToolResult {
            call_id: call.into(),
            status,
            text: String::new(),
            said: None,
            display: Default::default(),
            detail: None,
        },
    ]
}

fn edit(path: &str, old: &str, new: &str) -> Value {
    json!({"file_path": path, "edits": [{"old_string": old, "new_string": new}]})
}

fn write(path: &str, content: &str) -> Value {
    json!({"file_path": path, "content": content})
}

/// 一轮：开轮、几次调用、结束。
fn turn(turn: u64, calls: Vec<Vec<Push>>) -> Vec<Push> {
    let mut out = vec![Push::TurnStarted(turn, None)];
    out.extend(calls.into_iter().flatten());
    out.push(Push::TurnEnded(EndReason::Completed));
    out
}

fn listed(t: &Transcript) -> Vec<(String, u32, u32)> {
    t.changed()
        .into_iter()
        .map(
            |Changed {
                 path,
                 added,
                 removed,
             }| (path.to_string(), added, removed),
        )
        .collect()
}

/// 两轮：第一轮编辑 a.rs、写 new.md、跑一条命令、一次没成的编辑；第二轮又改 a.rs 两次。
fn two_turns() -> Vec<Push> {
    let ok = ToolStatus::Ok;
    let mut pushes = turn(
        1,
        vec![
            called("c1", "edit", &edit("/w/a.rs", "a\n", "b\nc\n"), ok),
            called("c2", "write", &write("/w/new.md", "x\ny\nz\n"), ok),
            called(
                "c3",
                "shell",
                &json!({"command": "sed -i s/a/b/ /w/a.rs"}),
                ok,
            ),
            called(
                "c4",
                "edit",
                &edit("/w/bad.rs", "a\n", "b\n"),
                ToolStatus::Error,
            ),
            called(
                "c5",
                "write",
                &write("/w/denied.rs", "a\n"),
                ToolStatus::Denied,
            ),
        ],
    );
    pushes.extend(turn(
        2,
        vec![
            called("c6", "edit", &edit("/w/a.rs", "c\n", "d\n"), ok),
            called("c7", "edit", &edit("/w/a.rs", "d\ne\n", "f\n"), ok),
        ],
    ));
    pushes
}

#[test]
fn edits_and_writes_group_by_file_in_first_changed_order() {
    let mut t = Transcript::default();
    apply(&mut t, two_turns());
    assert_eq!(
        listed(&t),
        [
            // 第一轮 +2 -1，第二轮 +1 -1、+1 -2。
            ("/w/a.rs".to_string(), 4, 4),
            ("/w/new.md".to_string(), 3, 0),
        ],
        "命令改的、出错的、被拒的不算"
    );
    // 改它的那几轮：同一轮里改了两次的并成一笔。
    let turns: Vec<u64> = t
        .changed_in("/w/a.rs")
        .iter()
        .map(|(turn, _)| *turn)
        .collect();
    assert_eq!(turns, [1, 2]);
    assert!(t.changed_in("/w/none.rs").is_empty());
}

#[test]
fn an_undone_turn_does_not_count_and_counts_again_when_restored() {
    let mut t = Transcript::default();
    apply(&mut t, two_turns());
    apply(
        &mut t,
        vec![Push::Reverted {
            turns: vec![2],
            seq: 90,
            cause: None,
        }],
    );
    assert_eq!(
        listed(&t),
        [
            ("/w/a.rs".to_string(), 2, 1),
            ("/w/new.md".to_string(), 3, 0),
        ],
        "撤掉的第二轮不算"
    );
    apply(
        &mut t,
        vec![Push::Reverted {
            turns: vec![1],
            seq: 91,
            cause: None,
        }],
    );
    assert!(listed(&t).is_empty(), "两轮都撤了：没有改过的文件");
    apply(&mut t, vec![Push::Unreverted(vec![1, 2])]);
    assert_eq!(listed(&t).len(), 2, "恢复了又算回来");
    assert_eq!(listed(&t)[0], ("/w/a.rs".to_string(), 4, 4));
}

#[test]
fn a_file_first_changed_in_an_undone_turn_takes_its_place_from_the_next_change() {
    let ok = ToolStatus::Ok;
    let mut t = Transcript::default();
    let mut pushes = turn(1, vec![called("c1", "write", &write("/w/b.md", "1\n"), ok)]);
    pushes.extend(turn(
        2,
        vec![called("c2", "write", &write("/w/a.md", "1\n2\n"), ok)],
    ));
    pushes.extend(turn(
        3,
        vec![called("c3", "write", &write("/w/b.md", "1\n2\n3\n"), ok)],
    ));
    pushes.push(Push::Reverted {
        turns: vec![1],
        seq: 90,
        cause: None,
    });
    apply(&mut t, pushes);
    assert_eq!(
        listed(&t),
        [("/w/a.md".to_string(), 2, 0), ("/w/b.md".to_string(), 3, 0)],
        "照没撤掉的里头一次改的先后"
    );
}

#[test]
fn a_draft_does_not_keep_the_ledger_and_a_lookup_finds_the_diffs() {
    let mut draft = Transcript::untracked();
    apply(&mut draft, two_turns());
    assert!(draft.changed().is_empty(), "草稿不记账");
    // 从日志里找（`Lookup`）：照先后交回每一步的差异和它是哪一轮的。
    let texts = crate::config::Config::builtin().unwrap().text_shared();
    let mut lookup = Lookup::new(texts, "/w/a.rs".into());
    for push in two_turns() {
        lookup.push(push);
    }
    let found = lookup.finish();
    let got: Vec<(u64, usize, usize)> = found
        .iter()
        .map(|f| (f.turn, f.diff.added, f.diff.removed))
        .collect();
    assert_eq!(got, [(1, 2, 1), (2, 1, 1), (2, 1, 2)]);
}

#[test]
fn the_tools_that_count_are_the_ones_the_timeline_calls_edits() {
    use crate::config::ToolKind;
    let config = crate::config::Config::builtin().unwrap();
    let mut edits: Vec<&str> = config
        .timeline
        .kinds
        .iter()
        .filter(|(_, kind)| **kind == ToolKind::Edit)
        .map(|(name, _)| name.as_str())
        .collect();
    edits.sort_unstable();
    assert_eq!(edits, super::super::changed::EDIT_TOOLS);
}
