//! 只留几屏和全留逐格对照，撤销那几样（第十五趟）：日志里中间撤过几轮（撤的那几轮跨了好几段，撤的时候早放掉了）、撤了又
//! 恢复，打开以后翻到顶、翻回底一屏屏一样；打开以后看着撤掉最后一轮，再翻上去翻回来也一样。

use crossterm::event::KeyCode;
use lm_kernel::kernel::event::{Body, Event};
use serde_json::{Value, json};

use super::{Ui, at, both, session};
use lm_kernel::kernel::session::Undid;

use crate::core::{Push, Update};

/// 照序号是 `seq` 的那几格（`seq`、`turn`、`trigger`、`seen`）不小于 `from` 的往后挪 `by`。
fn shift(event: &mut Value, from: u64, by: u64) {
    let bump = |v: &mut Value| {
        if let Some(n) = v.as_u64().filter(|&n| n >= from) {
            *v = json!(n + by);
        }
    };
    bump(&mut event["seq"]);
    if event.get("turn").is_some() {
        bump(&mut event["turn"]);
    }
    for key in ["trigger", "seen"] {
        if event["body"].get(key).is_some() {
            bump(&mut event["body"][key]);
        }
    }
}

/// 在第 `after` 轮结束以后插几条（`made` 照插在哪个序号写出来），后面的序号往后挪。
pub(super) fn insert(
    events: &mut Vec<Value>,
    after: usize,
    made: impl Fn(u64, &[u64]) -> Vec<Value>,
) {
    let turns: Vec<u64> = events
        .iter()
        .filter(|e| e["kind"] == "turn.started")
        .filter_map(|e| e["seq"].as_u64())
        .collect();
    let ended = events
        .iter()
        .filter(|e| e["kind"] == "turn.ended")
        .nth(after)
        .and_then(|e| e["seq"].as_u64())
        .expect("有这一轮");
    let added = made(ended + 1, &turns);
    let by = added.len() as u64;
    for event in events.iter_mut() {
        shift(event, ended + 1, by);
    }
    let at = events
        .iter()
        .position(|e| e["seq"].as_u64() == Some(ended + 1 + by))
        .unwrap_or(events.len());
    events.splice(at..at, added);
}

/// 一条事件。
pub(super) fn event(seq: u64, kind: &str, cause: &str, body: Value) -> Value {
    json!({"seq": seq, "at": at(seq), "kind": kind, "by": {"kind": "person"}, "cause": cause, "body": body})
}

/// 120 轮：第 40 轮结束后撤掉第 38–40 轮；第 80 轮结束后撤掉第 80 轮又恢复（`/restore`）。
fn undone_session() -> Vec<Event> {
    let mut events: Vec<Value> = session(120)
        .iter()
        .map(|e| serde_json::to_value(e).expect("事件写得成 JSON"))
        .collect();
    insert(&mut events, 80, |seq, turns| {
        vec![
            event(
                seq,
                "turn.reverted",
                "cmd-u2",
                json!({"turns": [turns[80]]}),
            ),
            event(
                seq + 1,
                "turn.unreverted",
                "cmd-r2",
                json!({"turns": [turns[80]]}),
            ),
        ]
    });
    insert(&mut events, 40, |seq, turns| {
        let files = json!({"files": [{"result": turns[39] + 1, "effect": 0, "path": "/tmp/a.rs",
            "action": "write", "outcome": "restored"}]});
        vec![
            event(
                seq,
                "turn.reverted",
                "cmd-u1",
                json!({"turns": &turns[38..=40]}),
            ),
            event(seq + 1, "files.restored", "cmd-u1", files),
        ]
    });
    events
        .into_iter()
        .map(|v| serde_json::from_value(v).expect("事件读得回来"))
        .collect()
}

/// 翻到顶、再翻回底，一屏屏一样。
pub(super) fn page_up_and_back(full: &mut Ui, kept: &mut Ui) {
    let mut pages = 0;
    while full.first() > 0 {
        pages += 1;
        both(full, kept, KeyCode::PageUp, &format!("上翻第 {pages} 下"));
        assert!(pages < 2000, "翻不到顶");
    }
    let mut down = 0;
    while full.app.screen.view.top.is_some() {
        down += 1;
        both(full, kept, KeyCode::PageDown, &format!("下翻第 {down} 下"));
        assert!(down < 4000, "翻不回底");
    }
}

#[test]
fn turns_undone_long_ago_stay_hidden_when_read_back_and_match_screen_by_screen() {
    let events = undone_session();
    let mut full = Ui::open(&events, usize::MAX, 72, 20);
    let mut kept = Ui::open(&events, super::super::SCREENS, 72, 20);
    assert_eq!(full.draw(), kept.draw(), "打开以后底下一样");
    let shown = |ui: &Ui, text: &str| {
        ui.app
            .talk
            .transcript
            .entries
            .iter()
            .any(|e| !e.hidden && e.text.contains(text))
    };
    assert!(!shown(&full, "第 39 轮回答"), "撤掉的那几轮藏着");
    assert!(shown(&full, "第 80 轮回答"), "撤了又恢复的那一轮露着");
    let lines = |ui: &Ui| {
        let kind = crate::transcript::Kind::Undo;
        ui.app
            .talk
            .transcript
            .entries
            .iter()
            .filter(|e| e.kind == kind && !e.hidden)
            .count()
    };
    assert_eq!(
        lines(&full),
        1,
        "恢复去掉了它那一行撤销说明，只剩第 38–40 轮的"
    );

    page_up_and_back(&mut full, &mut kept);
    assert!(
        !shown(&kept, "第 39 轮回答") && !shown(&kept, "第 38 轮的第 0 行问话"),
        "读回来的段里撤掉的那几轮照样藏着"
    );
}

#[test]
fn undoing_the_last_turn_while_watching_matches_after_paging() {
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
            cause: None,
        }));
        ui.app.core(Update::Undone {
            restore: false,
            undid: Undid::default(),
        });
    }
    let (a, b) = (full.settle(), kept.settle());
    assert_eq!(a, b, "撤掉以后一样");
    assert!(
        a.iter()
            .any(|l| l.contains("↶ 已撤销 · /restore 恢复 · 第 89 轮")),
        "撤销说明那一行贴着底：\n{}",
        a.join("\n")
    );
    page_up_and_back(&mut full, &mut kept);
}

#[test]
fn what_the_undo_also_undid_is_drawn_whichever_comes_first_and_after_paging() {
    // A15（Miyu 终端 `ui/undo_rows.rs::rows`、`transcript/words.rs::undo_counts`）：撤销的回应说撤掉了压缩、清空：说明那一行
    // 下面各一行；点开最后一行写停了几个任务。回应先到、推送先到都一样；翻到顶（说明那一段放掉）再翻回来，几行照旧。
    let events = session(90);
    let last = events
        .iter()
        .rev()
        .find(|e| matches!(e.body, Body::TurnStarted(_)))
        .map(|e| e.seq.get())
        .expect("有一轮");
    let seq = events.last().map_or(0, |e| e.seq.get()) + 1;
    let undid = Undid {
        compactions: 1,
        clears: 1,
        jobs: 2,
    };
    let reverted = || {
        Update::Push(Push::Reverted {
            turns: vec![last],
            seq,
            cause: None,
        })
    };
    let answered = || Update::Undone {
        restore: false,
        undid,
    };
    let mut full = Ui::open(&events, usize::MAX, 72, 20);
    let mut kept = Ui::open(&events, super::super::SCREENS, 72, 20);
    // 一个先到推送，一个先到回应。
    full.app.core(reverted());
    full.app.core(answered());
    kept.app.core(answered());
    kept.app.core(reverted());
    let (a, b) = (full.settle(), kept.settle());
    assert_eq!(a, b, "谁先到都一样");
    let has = |screen: &[String], text: &str| screen.iter().any(|l| l.contains(text));
    assert!(
        has(&a, "↶ 已撤销 · /restore 恢复")
            && has(&a, "  已撤销压缩，上下文恢复到压缩前")
            && has(&a, "  已撤销清空，上下文恢复到清空前"),
        "头一行下面两行，和「已撤销」对齐：\n{}",
        a.join("\n")
    );
    assert!(!has(&a, "已停止 2 个任务"), "停了几个任务收着不写");
    page_up_and_back(&mut full, &mut kept);
    let back = kept.settle();
    assert!(
        has(&back, "已撤销压缩，上下文恢复到压缩前")
            && has(&back, "已撤销清空，上下文恢复到清空前"),
        "放掉又读回来的那一段照旧写着：\n{}",
        back.join("\n")
    );
    // 点开：最后一行写停了几个任务。
    for ui in [&mut full, &mut kept] {
        let line = ui
            .app
            .talk
            .transcript
            .entries
            .iter_mut()
            .rev()
            .find(|e| e.kind == crate::transcript::Kind::Undo)
            .expect("有说明那一行");
        line.open = true;
    }
    let (a, b) = (full.settle(), kept.settle());
    assert_eq!(a, b, "点开以后一样");
    assert!(
        has(&a, "  已停止 2 个任务"),
        "点开最后写停了几个任务：\n{}",
        a.join("\n")
    );
}
