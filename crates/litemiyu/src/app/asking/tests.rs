//! 确认、提问交了被拒（核心 YC-7 新加的拒绝原因 `id_reused`）：抽屉重新打开，提示写界面的短话，不写空白、不崩。

use serde_json::json;

use crate::app::App;
use crate::config::Config;
use crate::core::{Asking, Core, Push, Update};

#[test]
fn an_answer_refused_as_id_reused_reopens_the_drawer_and_says_why() {
    let mut app = App::new(
        Config::builtin().expect("出厂的配置读得懂"),
        Core::new(|_| true),
    );
    app.core(Update::Ready("main".into()));
    let asked = serde_json::from_value(json!({"call_id": "call_1_1", "access": "execute"}))
        .expect("事件读得懂");
    app.core(Update::Push(Push::Asking(Asking::Approval(asked))));
    assert!(app.talk.drawers.current.is_some(), "抽屉开着");
    app.talk.drawers = Default::default();
    app.core(Update::AnswerRefused {
        call: "call_1_1".into(),
        reason: Some("id_reused".into()),
        message: "id_reused".into(),
    });
    assert!(app.talk.drawers.current.is_some(), "重新打开");
    let notice = app.typing.notice.as_ref().map(|n| n.text.clone());
    assert_eq!(
        notice.as_deref(),
        Some("同一个请求编号换了参数，没有收。请换一个新的编号")
    );
}

fn app() -> App {
    let mut app = App::new(
        Config::builtin().expect("出厂的配置读得懂"),
        Core::new(|_| true),
    );
    app.core(Update::Ready("main".into()));
    app
}

fn denied(reason: &str) -> Asking {
    let decided = serde_json::from_value(
        json!({"call_id": "call_1_1", "decision": "deny", "reason": reason}),
    )
    .expect("事件读得懂");
    Asking::Decided(decided)
}

fn approval() -> Asking {
    let asked = serde_json::from_value(json!({"call_id": "call_1_1", "access": "execute"}))
        .expect("事件读得懂");
    Asking::Approval(asked)
}

/// Miyu `app/asking.rs::answered`（`asked.filter(|_| session.is_none())`）：子代理问的题答了、确认定了，抽屉收起，结果不写进
/// 主会话的正文。
#[test]
fn what_a_subagent_asked_is_not_reported_in_the_main_transcript() {
    let mut app = app();
    let child = |push| Update::Child {
        session: "child".into(),
        push,
    };
    app.core(child(Push::Asking(approval())));
    assert!(app.talk.drawers.current.is_some(), "子代理问的也开抽屉");
    let before = app.talk.transcript.entries.len();
    app.core(child(Push::Asking(denied("不行"))));
    assert!(app.talk.drawers.current.is_none(), "定了收起");
    assert_eq!(
        app.talk.transcript.entries.len(),
        before,
        "主会话的正文不写：{:?}",
        app.talk.transcript.entries.last().map(|e| &e.text)
    );
}

/// Miyu `app/drawer.rs::report_into`（`job_line(JobMark::Failed, text, full)`）：不允许留在正文的那一行照后台任务结束的那一行
/// 画，红 `✗` 打头，理由多行的点开看全文。
#[test]
fn a_denial_is_a_failed_mark_row_that_opens_to_the_whole_reason() {
    let mut app = app();
    app.core(Update::Push(Push::Asking(approval())));
    app.core(Update::Push(Push::Asking(denied("不行\n换个办法"))));
    let entry = app.talk.transcript.entries.last().expect("写了一行");
    assert_eq!(entry.text, "不允许 · 不行 …");
    assert_eq!(entry.mark.as_deref(), Some("✗ "));
    assert_eq!(entry.mark_tone, crate::transcript::MarkTone::Bad);
    assert!(entry.report, "照后台任务那一行画");
    assert_eq!(entry.detail, "不行\n换个办法", "点开看全文");
}

/// 第二十六趟（验收时报的）：一轮里确认选了「不允许」，这一轮撤掉（`/undo`）时那一行跟着藏起来，`/restore` 回来时
/// 跟着回来；回答了的提问那一块一样。原来这两样不记在哪一轮名下，撤销以后还留着，切走再切回来才没。
#[test]
fn a_denial_and_an_answer_go_away_with_their_turn_and_come_back_with_it() {
    let mut app = app();
    app.core(Update::Push(Push::TurnStarted(3, None)));
    app.core(Update::Push(Push::Asking(approval())));
    app.core(Update::Push(Push::Asking(denied("不行"))));
    let asked = serde_json::from_value(json!({"call_id": "call_1_2", "questions": [
        {"question": "哪一个？", "options": [{"label": "甲"}, {"label": "乙"}]}
    ]}))
    .expect("事件读得懂");
    app.core(Update::Push(Push::Asking(Asking::Asked(asked))));
    let answered =
        serde_json::from_value(json!({"call_id": "call_1_2", "answers": [{"picked": ["甲"]}]}))
            .expect("事件读得懂");
    app.core(Update::Push(Push::Asking(Asking::Answered(answered))));
    let shown = |app: &App, kind: crate::transcript::Kind| {
        app.talk
            .transcript
            .entries
            .iter()
            .filter(|e| e.kind == kind && !e.hidden)
            .count()
    };
    let denial = |app: &App| {
        app.talk
            .transcript
            .entries
            .iter()
            .filter(|e| e.text.starts_with("不允许") && !e.hidden)
            .count()
    };
    assert_eq!(denial(&app), 1, "不允许写了一行");
    assert_eq!(
        shown(&app, crate::transcript::Kind::Answered),
        1,
        "回答写了一块"
    );
    app.core(Update::Push(Push::TurnEnded(
        crate::core::EndReason::Completed,
    )));
    app.core(Update::Push(Push::Reverted {
        turns: vec![3],
        seq: 40,
        cause: None,
    }));
    assert_eq!(denial(&app), 0, "撤掉那一轮：不允许那一行跟着藏");
    assert_eq!(
        shown(&app, crate::transcript::Kind::Answered),
        0,
        "回答那一块也藏"
    );
    app.core(Update::Push(Push::Unreverted(vec![3])));
    assert_eq!(denial(&app), 1, "恢复了：跟着回来");
    assert_eq!(shown(&app, crate::transcript::Kind::Answered), 1);
}
