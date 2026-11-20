//! 只留几屏和全留逐格对照，重做（`/redo`、`/edit`，第十七趟）：重做的撤销后面跟着同一个命令重发的话，不留撤销说明。日志里
//! 很早重做过一轮（重做的时候那几段早放掉了），打开以后翻到顶、翻回底一屏屏一样，读回来的段里也没有那一行说明；打开以后
//! 看着重做最后一轮，再翻上去翻回来也一样。

use lm_kernel::kernel::event::{Body, Event};
use serde_json::{Value, json};

use super::undo::{event, insert, page_up_and_back};
use super::{Ui, session};
use crate::core::{Push, Update};
use crate::transcript::Kind;

/// 120 轮：第 60 轮结束后重做它（`turn.reverted` 和重发的那句同一个命令）。
fn redone_session() -> Vec<Event> {
    let mut events: Vec<Value> = session(120)
        .iter()
        .map(|e| serde_json::to_value(e).expect("事件写得成 JSON"))
        .collect();
    insert(&mut events, 60, |seq, turns| {
        vec![
            event(seq, "turn.reverted", "cmd-x", json!({"turns": [turns[60]]})),
            event(
                seq + 1,
                "message.user",
                "cmd-x",
                json!({"blocks": [{"type": "text", "text": "第 60 轮重来"}]}),
            ),
        ]
    });
    events
        .into_iter()
        .map(|v| serde_json::from_value(v).expect("事件读得回来"))
        .collect()
}

/// 露着的撤销说明有几行。
fn undo_lines(ui: &Ui) -> usize {
    let entries = &ui.app.talk.transcript.entries;
    entries
        .iter()
        .filter(|e| e.kind == Kind::Undo && !e.hidden)
        .count()
}

/// 露着的条目里有没有写着 `text` 的。
fn shown(ui: &Ui, text: &str) -> bool {
    let entries = &ui.app.talk.transcript.entries;
    entries.iter().any(|e| !e.hidden && e.text.contains(text))
}

#[test]
fn a_redo_long_ago_leaves_no_undo_line_and_matches_screen_by_screen() {
    let events = redone_session();
    let mut full = Ui::open(&events, usize::MAX, 72, 20);
    let mut kept = Ui::open(&events, super::super::SCREENS, 72, 20);
    assert_eq!(full.draw(), kept.draw(), "打开以后底下一样");
    assert_eq!(undo_lines(&full), 0, "重做不留撤销说明");
    assert!(!shown(&full, "第 60 轮回答"), "重做撤掉的那一轮藏着");
    assert!(shown(&full, "第 60 轮重来"), "重发的那句在");
    page_up_and_back(&mut full, &mut kept);
    assert_eq!(undo_lines(&kept), 0, "读回来的段里也没有那一行说明");
    assert!(!shown(&kept, "第 60 轮回答") && shown(&kept, "第 60 轮重来"));
}

#[test]
fn redoing_the_last_turn_while_watching_matches_after_paging() {
    let events = session(90);
    let mut full = Ui::open(&events, usize::MAX, 72, 20);
    let mut kept = Ui::open(&events, super::super::SCREENS, 72, 20);
    let last = events
        .iter()
        .rev()
        .find(|e| matches!(e.body, Body::TurnStarted(_)))
        .map(|e| e.seq.get())
        .expect("有一轮");
    let seq = events.last().map_or(0, |e| e.seq.get()) + 1;
    for ui in [&mut full, &mut kept] {
        ui.app.core(Update::Push(Push::Reverted {
            turns: vec![last],
            seq,
            cause: Some("cmd-redo".into()),
        }));
        ui.app.core(Update::Push(Push::Resent("cmd-redo".into())));
    }
    let (a, b) = (full.settle(), kept.settle());
    assert_eq!(a, b, "重做以后一样");
    assert!(
        !a.iter().any(|l| l.contains("已撤销")),
        "重做不留撤销说明：\n{}",
        a.join("\n")
    );
    assert_eq!(undo_lines(&full), 0);
    page_up_and_back(&mut full, &mut kept);
}
