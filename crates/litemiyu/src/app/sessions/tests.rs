//! 打开会话读不了的两种（界面线第十八趟，照 Miyu 终端 `app/tests/unread.rs`）：一时读不了的照「请稍后重试」说，日志中间坏了
//! 一行的正文末尾写坏在哪（只写一次），记着只能看，说话被拒弹「只能看」、字退回框里。

use crate::app::App;
use crate::config::Config;
use crate::core::{Core, Update};

fn app() -> App {
    App::new(
        Config::builtin().expect("出厂的配置读得懂"),
        Core::new(|_| true),
    )
}

fn notice(app: &App) -> Option<String> {
    app.typing.notice.as_ref().map(|n| n.text.clone())
}

#[test]
fn a_session_unreadable_for_now_says_try_again_later() {
    let mut app = app();
    app.core(Update::OpenFailed {
        taken: false,
        unavailable: true,
        message: "Session can't be read right now. Try again later. (Permission denied)".into(),
    });
    assert_eq!(
        notice(&app).as_deref(),
        Some("会话暂时无法读取，请稍后重试")
    );
    app.core(Update::OpenFailed {
        taken: false,
        unavailable: false,
        message: "the session log has no session.created".into(),
    });
    assert_eq!(
        notice(&app).as_deref(),
        Some("无法打开此会话：会话日志缺少开头的记录"),
        "别的原因照 `why` 换成句子（LM-39）"
    );
}

#[test]
fn a_session_broken_in_the_middle_says_where_once_and_stays_view_only() {
    let mut app = app();
    app.core(Update::Opened("s-one".into()));
    app.core(Update::ReadOnly { line: 7 });
    assert!(app.talk.transcript.read_only);
    assert_eq!(
        app.talk.transcript.session.as_deref(),
        Some("s-one"),
        "照旧对着它"
    );
    let line = "会话日志第 7 行已损坏，只能查看这一行之前的内容。如果要继续，请新建会话。";
    let count = |app: &App| {
        let t = &app.talk.transcript.entries;
        t.iter().filter(|e| e.text == line).count()
    };
    assert_eq!(count(&app), 1);
    app.core(Update::ReadOnly { line: 7 });
    assert_eq!(count(&app), 1, "不写第二遍");
    // 说话被拒：弹「只能看」。
    app.core(Update::Unsent {
        reason: Some("session_read_only".into()),
        message: "session_read_only".into(),
    });
    assert_eq!(
        notice(&app).as_deref(),
        Some("这个会话只能查看，不能继续对话。输入 /new 新建会话")
    );
    // `/new` 以后是新的一份：不再只读。
    app.core(Update::Opened("s-two".into()));
    assert!(!app.talk.transcript.read_only);
}

/// 会话列表的鼠标（Miyu `app/switch.rs` 的 `sessions_mouse`，对照表 B67）：悬停选中那一行、不切；滚轮挪露出的那一段，
/// 选中的不动；落在框的边上也算框的，不往下交给正文。
#[test]
fn hovering_picks_a_row_and_the_wheel_scrolls_without_moving_the_pick() {
    use crossterm::event::{KeyModifiers, MouseEvent, MouseEventKind};
    use lm_engine::session::Listed;
    use lm_kernel::kernel::id::SessionId;
    use lm_kernel::kernel::time::Timestamp;
    use ratatui::layout::{Position, Rect};

    let mut app = app();
    let all: Vec<Listed> = (0..15)
        .map(|i| Listed {
            id: SessionId::parse(&format!("0192f3a0-1111-7abc-8def-0000000000{i:02}"))
                .expect("编号的写法"),
            title: format!("第 {i} 个"),
            cwd: "/w".to_string(),
            last_active: Timestamp::from_unix_millis(1_800_000_000_000 - i * 60_000)
                .expect("在范围里"),
            turns: 1,
            open: false,
            busy: false,
            preview: String::new(),
        })
        .collect();
    let mut list = crate::session_list::SessionList::default();
    list.replace(all);
    app.boxes.sessions = Some(list);
    app.boxes.panel = Some(super::Panel::Sessions);
    // 框连边从第 10 行起，字从第 11 行起，露十行：第 0 到 9 个。
    app.screen.areas.menu = Rect::new(0, 10, 80, 12);
    app.screen.areas.menu_text = Rect::new(2, 11, 76, 10);
    app.screen.panel_rows = (0..10).map(Some).collect();
    let at = |kind, column, row| {
        (
            MouseEvent {
                kind,
                column,
                row,
                modifiers: KeyModifiers::NONE,
            },
            Position::new(column, row),
        )
    };
    let list = |app: &App| app.boxes.sessions.as_ref().expect("框开着").selected;
    let (hover, pos) = at(MouseEventKind::Moved, 5, 14);
    assert!(app.sessions_mouse(hover, pos));
    assert_eq!(list(&app), 3, "悬停选中那一行");
    assert!(app.boxes.sessions.is_some(), "悬停不切过去");
    let (wheel, pos) = at(MouseEventKind::ScrollDown, 5, 14);
    assert!(app.sessions_mouse(wheel, pos), "滚轮归框，不往下交给正文");
    let shown = app.boxes.sessions.as_ref().expect("框开着");
    assert_eq!(shown.selected, 3, "滚轮不挪选中的");
    assert!(shown.pinned.is_some(), "露出的那一段钉住了");
    let (edge, pos) = at(MouseEventKind::Moved, 0, 10);
    assert!(app.sessions_mouse(edge, pos), "框的边也算框的");
    assert_eq!(list(&app), 3);
    let (outside, pos) = at(MouseEventKind::ScrollDown, 5, 30);
    assert!(!app.sessions_mouse(outside, pos), "框外的不归它");
}

/// 打开旧会话时框里的字（Miyu `636b2b8d`）：从别的会话切过来的，字留给原来那个、切回来放回；起来时等着开（`-c`、`-s`）、
/// 原来还没开会话的，先打的字不被开好那一下冲掉（第二十一趟量尺在模拟器里慢，这样冲掉过）。
#[test]
fn opening_a_session_keeps_typed_text_with_the_session_it_was_typed_in() {
    let mut app = app();
    app.typing.input.editor.insert("先打的字");
    app.core(Update::Opened("s-one".into()));
    assert_eq!(
        app.typing.input.draft().text,
        "先打的字",
        "原来没开会话：不冲掉"
    );
    app.core(Update::Opened("s-two".into()));
    assert_eq!(app.typing.input.draft().text, "", "切走：字留给 s-one");
    app.typing.input.editor.insert("two 的字");
    app.core(Update::Opened("s-one".into()));
    assert_eq!(app.typing.input.draft().text, "先打的字", "切回来放回");
    app.core(Update::Opened("s-two".into()));
    assert_eq!(app.typing.input.draft().text, "two 的字");
}
