//! 会话一时读不了自动再试（对照表 D3，Miyu 终端 `app/unread.rs`、`app/tests/unread.rs`）：每 5 秒一次、最多 6 次，试满了弹
//! 「请稍后重试」不再试；切走了的不试。

use super::{Ui, session};
use crate::app::sessions::Opening;
use crate::core::Update;

fn failed() -> Update {
    Update::OpenFailed {
        taken: false,
        unavailable: true,
        message: "Permission denied".to_string(),
    }
}

fn hint(ui: &Ui) -> String {
    ui.app
        .typing
        .notice
        .as_ref()
        .map(|n| n.text.clone())
        .unwrap_or_default()
}

#[test]
fn an_unreadable_session_is_retried_every_five_seconds_six_times_then_given_up() {
    let mut ui = Ui::open(&session(3), usize::MAX, 72, 20);
    ui.app.open_session("s2".to_string());
    ui.app.core(failed());
    assert_eq!(hint(&ui), "会话暂时无法读取，5 秒后自动重试");
    let first = ui.app.talk.reread.clone().expect("记着要再试");
    assert_eq!(
        (first.session.as_str(), first.tries, first.asked),
        ("s2", 0, false)
    );
    assert_eq!(
        ui.app.deadline().map(|d| d <= first.due),
        Some(true),
        "到点醒来"
    );
    for n in 1..=crate::app::unread::TRIES {
        let due = ui.app.talk.reread.as_ref().expect("还在等").due;
        // 还没到点不试。
        ui.app.reread_due(due - std::time::Duration::from_millis(1));
        assert_eq!(ui.app.talk.opening, Opening::None, "第 {n} 次：没到点");
        ui.app.reread_due(due);
        assert_eq!(
            ui.app.talk.opening,
            Opening::Asked("s2".to_string()),
            "第 {n} 次：再开"
        );
        assert_eq!(ui.app.talk.reread.as_ref().map(|r| r.tries), Some(n));
        ui.app.core(failed());
    }
    assert_eq!(hint(&ui), "会话暂时无法读取，请稍后重试", "试满了");
    assert_eq!(ui.app.talk.reread, None, "不再试");
}

#[test]
fn switching_away_before_the_retry_drops_it() {
    let mut ui = Ui::open(&session(3), usize::MAX, 72, 20);
    ui.app.open_session("s2".to_string());
    ui.app.core(failed());
    let due = ui.app.talk.reread.as_ref().expect("记着要再试").due;
    // 这之间换了会话（`/new`、开了别的）：不试了。
    ui.app.talk.transcript.session = Some("s3".to_string());
    ui.app.reread_due(due);
    assert_eq!(ui.app.talk.reread, None);
    assert_eq!(ui.app.talk.opening, Opening::None, "没再开");
}
