//! 回顾的字进快照（施工 3-8 四补，`docs/blueprint/policy.md`「`CoreTexts`」；LM-8 删了回顾和它的指令）：出厂的快照带着回顾的
//! 四份字（两种标签、两句记号）和两个数，起标题写对话记录借它们；以前造的快照里多的那一格指令读的时候不理。

use crate::kernel::event::Event;
use crate::kernel::history::History;
use crate::kernel::ledger::Ledger;

use super::*;

/// 一段说完了一轮的有效历史：人说「hi」，她答「好。」，这一轮完了。
fn answered() -> History {
    let lines = [
        r#"{"seq":1,"at":"2026-09-25T07:00:00.000Z","kind":"session.created","by":{"kind":"kernel"},"body":{"policy":"sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"}}"#,
        r#"{"seq":2,"at":"2026-09-25T07:00:00.000Z","kind":"message.user","by":{"kind":"person"},"body":{"blocks":[{"type":"text","text":"hi"}]}}"#,
        r#"{"seq":3,"at":"2026-09-25T07:00:00.000Z","kind":"turn.started","turn":3,"by":{"kind":"kernel"},"body":{"trigger":2}}"#,
        r#"{"seq":4,"at":"2026-09-25T07:00:00.000Z","kind":"message.assistant","turn":3,"by":{"kind":"model","endpoint":"deepseek","model":"deepseek-v4"},"body":{"blocks":[{"type":"text","text":"好。"}],"seen":3}}"#,
        r#"{"seq":5,"at":"2026-09-25T07:00:00.000Z","kind":"turn.ended","turn":3,"by":{"kind":"kernel"},"body":{"reason":"completed"}}"#,
    ];
    let (mut ledger, mut history) = (Ledger::default(), History::default());
    for line in lines {
        let event = Event::from_line(line).unwrap();
        ledger.append(&event).unwrap();
        history.append(event);
    }
    history
}

#[test]
fn recap_labels_go_in_without_the_instruction_and_older_snapshots_still_read() {
    let snapshot = engineer();
    assert_eq!(snapshot.recap, Some(crate::policy::RECAP));
    let bytes = String::from_utf8(snapshot.to_bytes()).unwrap();
    assert!(
        bytes.contains(r#","recap":{"user":"#),
        "标签、记号照旧进快照：{bytes}"
    );
    assert!(
        !bytes.contains("Write a short recap"),
        "回顾的指令不进快照了"
    );
    let retitled = |snapshot: &Snapshot| {
        snapshot
            .policy()
            .unwrap()
            .assembler
            .retitle(&answered())
            .map(|(request, _)| serde_json::to_string(&request.messages).unwrap())
    };
    let text = retitled(&snapshot).expect("起标题照回顾的标签写对话记录");
    assert!(text.contains("User: hi\\n\\nAssistant: 好。"), "{text}");
    // 以前造的快照里回顾的字多一格指令：读的时候不理，起标题照旧。
    let older = bytes.replacen(
        r#","recap":{"user":"#,
        r#","recap":{"instruction":"Write a short recap.\nConversation:\n","user":"#,
        1,
    );
    assert_ne!(older, bytes);
    let read = Snapshot::from_bytes(older.as_bytes()).unwrap();
    assert_eq!(read.to_bytes(), bytes.as_bytes(), "多的那一格不理");
    assert_eq!(retitled(&read), Some(text));
    // 更早的快照没有回顾的数：不起标题（起标题借它们）。
    let mut no_numbers = snapshot.clone();
    no_numbers.recap = None;
    assert_eq!(retitled(&no_numbers), None);
}
