//! 起标题进快照（施工 3-8 五补，`docs/blueprint/policy.md`「`CoreTexts`」）：出厂的快照带着起标题的指令和两个数，造出的组装器
//! 起得出标题、内核拿到最多几个字；字、数少了哪一样都不起（回顾的标签没有的也不起）；以前造的快照没有这两格，读进来再写出去
//! 一字不差，不起；更早一些的快照的数里多一格 `tries`（会话自己起标题试几次，砍了），照样读得出。

use crate::kernel::event::Event;
use crate::kernel::history::History;
use crate::kernel::ledger::Ledger;
use crate::kernel::session::Titles;

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

/// 照快照造的策略：起标题的那一条 user 的字（组装器），和交给内核的两个数。
fn titled(snapshot: &Snapshot) -> (Option<String>, Option<Titles>) {
    let policy = snapshot.policy().unwrap();
    let text = policy
        .assembler
        .retitle(&answered())
        .map(|(request, upto)| {
            assert_eq!(upto.get(), 4);
            serde_json::to_string(&request.messages).unwrap()
        });
    (text, policy.titles)
}

#[test]
fn title_texts_and_numbers_go_in_and_older_snapshots_lack_them() {
    let snapshot = engineer();
    assert_eq!(snapshot.title, Some(crate::policy::TITLE));
    let (text, titles) = titled(&snapshot);
    let text = text.expect("出厂的起得出标题");
    assert!(
        text.contains("Write a title of 3 to 7 words")
            && text.ends_with("Conversation:\\nUser: hi\\n\\nAssistant: 好。\"}]}]"),
        "{text}"
    );
    assert_eq!(titles, Some(Titles { chars: 50 }));
    let mut no_numbers = snapshot.clone();
    no_numbers.title = None;
    assert_eq!(titled(&no_numbers), (None, None), "没有数的不起");
    let mut no_texts = snapshot.clone();
    no_texts.core.title = None;
    assert_eq!(titled(&no_texts), (None, None), "没有字的不起");
    let mut no_labels = snapshot.clone();
    no_labels.core.recap = None;
    assert_eq!(titled(&no_labels).0, None, "没有回顾的标签的不起");
    // 快照里的数照快照的。
    let mut other = snapshot.clone();
    other.title = Some(crate::policy::TitleNumbers {
        tokens: 1,
        chars: 7,
    });
    let (tiny, titles) = titled(&other);
    assert!(
        tiny.unwrap().ends_with("Conversation:\\n\"}]}]"),
        "一个 token 也放不下指令，对话记录一个字节都不剩"
    );
    assert_eq!(titles, Some(Titles { chars: 7 }));

    let bytes = String::from_utf8(snapshot.to_bytes()).unwrap();
    let texts_start = bytes.find(r#","title":{"instruction""#).unwrap();
    let texts_end = texts_start + bytes[texts_start..].find(r#""}"#).unwrap() + 2;
    let older = bytes[..texts_start].to_string() + &bytes[texts_end..];
    let older = older.replacen(TITLE_NUMBERS, "", 1);
    let read = Snapshot::from_bytes(older.as_bytes()).unwrap();
    assert_eq!((read.title, read.core.title.is_none()), (None, true));
    assert_eq!(read.to_bytes(), older.as_bytes(), "读进来再写出去一字不差");
    assert_eq!(titled(&read), (None, None));
}

/// 更早一些的快照（2026-11-14 以前造的）：起标题的数里多一格 `tries`。照样读得出，数照它的，内核拿到的照旧；写出去的没有
/// 这一格（读回来的快照只在改了以后另写一份新的，旧的那份按哈希原样留着）。
#[test]
fn an_older_snapshot_with_tries_still_reads() {
    let snapshot = engineer();
    let bytes = String::from_utf8(snapshot.to_bytes()).unwrap();
    let older = bytes.replacen(
        TITLE_NUMBERS,
        r#","title":{"tokens":1024,"chars":50,"tries":2}"#,
        1,
    );
    assert_ne!(older, bytes, "出厂的字节里有起标题的数");
    let read = Snapshot::from_bytes(older.as_bytes()).unwrap();
    assert_eq!(read.title, Some(crate::policy::TITLE));
    assert_eq!(titled(&read), titled(&snapshot));
    assert_eq!(read.to_bytes(), snapshot.to_bytes());
}
