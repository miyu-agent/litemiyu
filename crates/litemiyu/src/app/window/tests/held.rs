//! 等的时候按的键（LM-15）：只留几屏时 `/jump` 回车以后要等日志里你说的话读回来才开列表，跳到放掉的段里的一句要等那一段
//! 读回来、排好才跳过去；全留的这两处都是当场的。这一段里按的键、粘贴先攒着，等完了照先后补办，落点和全留的一样。按键都经
//! `App::handle`（主循环交的那一处），只留几屏的那一份要的读回、你说的话扣着不交，先后是定的。

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};

use super::late::{pair, release};
use super::live::one_more;
use super::{Ui, replayed, session};
use crate::app::jump::{HOLD, Listing};
use crate::core::{Command, Update};

/// 一个按下的键。
fn down(code: KeyCode) -> Event {
    Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
}

/// 全留的：经 `handle` 按一下，画稳。
fn press(ui: &mut Ui, code: KeyCode) -> Vec<String> {
    ui.app.handle(down(code));
    ui.settle()
}

/// 只留几屏的：经 `handle` 按一下、画一帧，要的读回、你说的话扣进 `late`，不交。
fn press_held(ui: &mut Ui, code: KeyCode, late: &mut Vec<Command>) {
    ui.app.handle(down(code));
    ui.draw();
    late.append(&mut ui.app.talk.wanted);
}

/// 两边一起打一串字：全留的画稳，只留几屏的扣着。
fn type_both(full: &mut Ui, kept: &mut Ui, text: &str, late: &mut Vec<Command>) {
    for c in text.chars() {
        press(full, KeyCode::Char(c));
        press_held(kept, KeyCode::Char(c), late);
    }
}

/// 列表、输入框的样子：开没开、是不是 `/jump`、选中第几条、搜的字、输入框里的字。
fn state(ui: &Ui) -> (bool, bool, usize, String, String) {
    let history = &ui.app.typing.history;
    (
        history.open,
        history.jump,
        history.selected,
        history.query.clone(),
        ui.app.typing.input.editor.text().to_string(),
    )
}

/// 打 `/jump`、回车：全留的列表当场开了，只留几屏的在等日志里你说的话（扣在 `late` 里）。
fn open_both(full: &mut Ui, kept: &mut Ui, late: &mut Vec<Command>) {
    type_both(full, kept, "/jump", late);
    press(full, KeyCode::Enter);
    press_held(kept, KeyCode::Enter, late);
    assert!(full.app.typing.history.open, "全留的当场开");
    assert!(!kept.app.typing.history.open, "只留几屏的在等");
    assert!(
        late.iter().any(|c| matches!(c, Command::Spoken { .. })),
        "要了日志里你说的话"
    );
}

#[test]
fn arrows_pressed_before_the_spoken_list_arrives_land_on_the_list() {
    let events = session(120);
    let (mut full, mut kept) = pair(&events);
    let mut late = Vec::new();
    open_both(&mut full, &mut kept, &mut late);
    // 连按很多下：一下下都攒着。
    for _ in 0..60 {
        press(&mut full, KeyCode::Up);
        press_held(&mut kept, KeyCode::Up, &mut late);
    }
    release(&mut kept, &mut late);
    assert_eq!(
        state(&kept),
        state(&full),
        "列表开了以后，攒着的 ↑ 落在列表上"
    );
    assert_eq!(state(&full), (true, true, 60, String::new(), String::new()));
    // 跳过去：同一句，输入框空着。
    let a = press(&mut full, KeyCode::Enter);
    let b = press(&mut kept, KeyCode::Enter);
    assert_eq!(a, b, "跳到同一句");
    assert_eq!(state(&kept), state(&full));
}

#[test]
fn a_search_and_enter_typed_while_waiting_jump_to_the_same_sentence() {
    let events = session(120);
    let (mut full, mut kept) = pair(&events);
    let mut late = Vec::new();
    open_both(&mut full, &mut kept, &mut late);
    type_both(&mut full, &mut kept, "第 37 轮", &mut late);
    press(&mut full, KeyCode::Up);
    press_held(&mut kept, KeyCode::Up, &mut late);
    press(&mut full, KeyCode::Enter);
    press_held(&mut kept, KeyCode::Enter, &mut late);
    assert!(!full.app.typing.history.open, "全留的跳过去了");
    let b = release(&mut kept, &mut late);
    assert_eq!(full.settle(), b, "跳到同一句");
    assert_eq!(state(&kept), state(&full), "列表关了、输入框空着");
}

#[test]
fn esc_while_waiting_gives_up_like_closing_the_open_list() {
    let events = session(120);
    let (mut full, mut kept) = pair(&events);
    let mut late = Vec::new();
    open_both(&mut full, &mut kept, &mut late);
    for _ in 0..2 {
        press(&mut full, KeyCode::Up);
        press_held(&mut kept, KeyCode::Up, &mut late);
    }
    press(&mut full, KeyCode::Esc);
    press_held(&mut kept, KeyCode::Esc, &mut late);
    assert!(
        matches!(kept.app.talk.listing, Listing::None),
        "不等了：{:?}",
        kept.app.talk.listing
    );
    assert_eq!(kept.app.talk.held.count(), 0, "攒着的扔掉");
    // 列表关着、输入框空着（全留的关了列表以后选中第几条还记着，看不见，不比）。
    let shown = |ui: &Ui| (state(ui).0, state(ui).4);
    assert_eq!(shown(&kept), (false, String::new()));
    assert_eq!(shown(&kept), shown(&full));
    // 晚到的你说的话不开列表；之后打的字照常进输入框。
    release(&mut kept, &mut late);
    assert!(!kept.app.typing.history.open, "晚到的不开");
    press(&mut full, KeyCode::Char('x'));
    press(&mut kept, KeyCode::Char('x'));
    assert_eq!(shown(&kept), shown(&full));
    assert_eq!(full.settle(), kept.settle());
}

#[test]
fn esc_after_an_enter_while_waiting_waits_its_turn() {
    let events = session(120);
    let (mut full, mut kept) = pair(&events);
    let mut late = Vec::new();
    open_both(&mut full, &mut kept, &mut late);
    for code in [
        KeyCode::Up,
        KeyCode::Up,
        KeyCode::Up,
        KeyCode::Enter,
        KeyCode::Esc,
    ] {
        press(&mut full, code);
        press_held(&mut kept, code, &mut late);
    }
    assert_eq!(kept.app.talk.held.count(), 5, "回车以后的 Esc 也攒着");
    let b = release(&mut kept, &mut late);
    assert_eq!(full.settle(), b, "跳到同一句，Esc 照全留的那样办");
    assert_eq!(state(&kept), state(&full));
}

#[test]
fn a_spoken_list_that_cannot_be_read_opens_from_the_body_with_the_keys() {
    let events = session(120);
    let (mut full, mut kept) = pair(&events);
    let mut late = Vec::new();
    open_both(&mut full, &mut kept, &mut late);
    for _ in 0..2 {
        press_held(&mut kept, KeyCode::Up, &mut late);
    }
    let session = late
        .iter()
        .find_map(|c| match c {
            Command::Spoken { session, .. } => Some(session.clone()),
            _ => None,
        })
        .expect("要了");
    kept.app.core(Update::Spoken {
        session,
        said: Err("读不了".into()),
    });
    kept.settle();
    assert!(
        kept.app.typing.history.open && kept.app.typing.history.jump,
        "照正文开"
    );
    assert_eq!(state(&kept).2, 2, "攒着的两下 ↑ 落在列表上");
    assert_eq!(state(&kept).4, "", "输入框空着");
    assert_eq!(kept.app.talk.held.count(), 0);
}

#[test]
fn keys_are_held_no_longer_than_the_limit() {
    let events = session(120);
    let (mut full, mut kept) = pair(&events);
    let mut late = Vec::new();
    open_both(&mut full, &mut kept, &mut late);
    press_held(&mut kept, KeyCode::Up, &mut late);
    let since = kept.app.talk.held.since.expect("攒着");
    assert!(
        kept.app.deadline().is_some_and(|d| d <= since + HOLD),
        "主循环到点醒来"
    );
    kept.app.tick();
    assert_eq!(kept.app.talk.held.count(), 1, "没到点照旧等");
    kept.app.talk.held.since = since.checked_sub(HOLD);
    kept.app.tick();
    kept.settle();
    assert!(matches!(kept.app.talk.listing, Listing::None), "不等了");
    assert!(kept.app.typing.history.open, "照读不了办：照正文开");
    assert_eq!(state(&kept).2, 1, "攒着的 ↑ 补办了");
    assert_eq!(kept.app.talk.held.since, None);
    // 晚到的不管。
    let resident = kept.app.list_items().0.len();
    release(&mut kept, &mut late);
    assert_eq!(kept.app.list_items().0.len(), resident, "晚到的不并进来");
    assert!(resident < full.app.list_items().0.len(), "只列着常驻的");
}

#[test]
fn mouse_and_resizing_while_waiting_are_done_at_once() {
    let events = session(120);
    let (mut full, mut kept) = pair(&events);
    let mut late = Vec::new();
    open_both(&mut full, &mut kept, &mut late);
    press_held(&mut kept, KeyCode::Up, &mut late);
    // 滚轮在正文上：当场滚（照眼前这一帧），不攒。
    let first = kept.first();
    let body = kept.app.screen.areas.body;
    kept.app.handle(Event::Mouse(MouseEvent {
        kind: MouseEventKind::ScrollUp,
        column: body.x + 2,
        row: body.y + 2,
        modifiers: KeyModifiers::NONE,
    }));
    assert_eq!(kept.app.talk.held.count(), 1, "鼠标不攒");
    kept.draw();
    late.append(&mut kept.app.talk.wanted);
    assert!(kept.first() < first, "当场滚了");
    // 改窗口大小：当场办（主循环先换了屏幕的大小），不攒。
    kept.app.handle(Event::Resize(60, 24));
    kept.width = 60;
    kept.height = 24;
    kept.draw();
    late.append(&mut kept.app.talk.wanted);
    assert_eq!(kept.app.talk.held.count(), 1, "改大小不攒");
    press_held(&mut kept, KeyCode::Up, &mut late);
    release(&mut kept, &mut late);
    assert_eq!(state(&kept), (true, true, 2, String::new(), String::new()));
}

#[test]
fn a_turn_streaming_in_while_waiting_does_not_move_where_the_keys_land() {
    let events = session(120);
    let (mut full, mut kept) = pair(&events);
    let mut late = Vec::new();
    open_both(&mut full, &mut kept, &mut late);
    press(&mut full, KeyCode::Up);
    press_held(&mut kept, KeyCode::Up, &mut late);
    // 等的时候来了一轮（你说的一句、她回的）：推送一条条到。
    let extra = one_more(&events, false);
    for ui in [&mut full, &mut kept] {
        ui.events.extend_from_slice(&extra);
        for push in replayed(&extra) {
            ui.app.core(Update::Push(push));
        }
    }
    full.settle();
    kept.draw();
    late.append(&mut kept.app.talk.wanted);
    for _ in 0..3 {
        press(&mut full, KeyCode::Up);
        press_held(&mut kept, KeyCode::Up, &mut late);
    }
    release(&mut kept, &mut late);
    assert_eq!(state(&kept), state(&full));
    let a = press(&mut full, KeyCode::Enter);
    let b = press(&mut kept, KeyCode::Enter);
    assert_eq!(a, b, "跳到同一句");
}

#[test]
fn a_paste_while_waiting_goes_into_the_list_search() {
    let events = session(120);
    let (mut full, mut kept) = pair(&events);
    let mut late = Vec::new();
    open_both(&mut full, &mut kept, &mut late);
    full.app.handle(Event::Paste("第 5 轮".into()));
    full.settle();
    kept.app.handle(Event::Paste("第 5 轮".into()));
    kept.draw();
    late.append(&mut kept.app.talk.wanted);
    assert_eq!(kept.app.talk.held.count(), 1, "粘贴也攒着");
    release(&mut kept, &mut late);
    assert_eq!(state(&kept), state(&full));
    assert_eq!(state(&kept).3, "第 5 轮", "进了列表的搜索");
}

#[test]
fn keys_after_jumping_into_a_shed_part_wait_for_the_jump() {
    let events = session(120);
    let (mut full, mut kept) = pair(&events);
    let mut late = Vec::new();
    open_both(&mut full, &mut kept, &mut late);
    release(&mut kept, &mut late);
    for _ in 0..100 {
        press(&mut full, KeyCode::Up);
        press(&mut kept, KeyCode::Up);
    }
    for code in [KeyCode::Enter, KeyCode::PageUp, KeyCode::PageUp] {
        press(&mut full, code);
        press_held(&mut kept, code, &mut late);
    }
    assert!(kept.app.talk.going.is_some(), "在等那一段读回");
    assert_eq!(kept.app.talk.held.count(), 2, "跳过去以前的 PgUp 攒着");
    let b = release(&mut kept, &mut late);
    assert_eq!(full.settle(), b, "跳过去了再翻两屏，和全留的一样");
    assert_eq!(kept.app.talk.held.count(), 0);
}
