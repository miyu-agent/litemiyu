//! 读回比按键慢（第十三趟）：只留几屏的那一份要的读回先扣着不交，翻页先落进还没读回的空档（画的是占位的空行），再把读回
//! 放进来，和全留的逐格比。机器忙时真程序里就是这样（读回在另一条线程，按键先到）；这里先后是定的，不靠运气。
//!
//! 找回视口的规矩（`ui/body.rs` 的 `remap`）：读回以后，人眼前靠着已有内容的那一边不动。往上翻进空档，视口底下露着空档
//! 下面那一条，照离它几行找回；往下翻进空档，照离空档上面那一条几行找回；整屏都是空档的，照最后一次看见的那一边。

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::jump::open_jump;
use super::live::{back_down, one_more};
use super::{Ui, both, replayed, session};
use crate::core::{Command, Update};

/// 一对界面：全留的、只留几屏的，72×20。
fn pair(events: &[lm_kernel::kernel::event::Event]) -> (Ui, Ui) {
    (
        Ui::open(events, usize::MAX, 72, 20),
        Ui::open(events, super::super::SCREENS, 72, 20),
    )
}

/// 只留几屏的那一份按一下 `code`、画一帧，要的读回扣进 `late`，不交。
fn held(ui: &mut Ui, code: KeyCode, late: &mut Vec<Command>) {
    ui.app.key(KeyEvent::new(code, KeyModifiers::NONE));
    ui.draw();
    late.append(&mut ui.app.talk.wanted);
}

/// 扣着的读回照要的先后交进去，画稳。
fn release(ui: &mut Ui, late: &mut Vec<Command>) -> Vec<String> {
    let pending = std::mem::take(&mut ui.app.talk.wanted);
    ui.app.talk.wanted = late.drain(..).chain(pending).collect();
    ui.settle()
}

/// 视口顶上落在放掉的段（空档）里。
fn in_gap(ui: &Ui) -> bool {
    ui.app.screen.view.rows.gap_at(ui.first()).is_some()
}

/// 两边一起按 `code`，全留的画稳、只留几屏的扣着读回：按到只留几屏的视口顶上落进空档，再多按 `more` 下（每下都还在空档
/// 里、没到顶）。
fn into_gap(full: &mut Ui, kept: &mut Ui, code: KeyCode, more: usize, late: &mut Vec<Command>) {
    let mut presses = 0;
    while !in_gap(kept) {
        full.key(code);
        held(kept, code, late);
        presses += 1;
        assert!(presses < 60, "按了 {presses} 下还没落进空档");
    }
    for k in 0..more {
        full.key(code);
        held(kept, code, late);
        assert!(
            in_gap(kept) && kept.first() > 0,
            "多按的第 {k} 下还在空档里"
        );
    }
    assert!(!late.is_empty(), "要了读回、扣着");
}

#[test]
fn paging_up_into_a_gap_before_it_is_read_back_matches() {
    let events = session(120);
    for more in [0, 1, 4] {
        let (mut full, mut kept) = pair(&events);
        let mut late = Vec::new();
        into_gap(&mut full, &mut kept, KeyCode::PageUp, more, &mut late);
        let at = format!("上翻进空档再 {more} 下");
        assert_eq!(full.draw(), release(&mut kept, &mut late), "{at}，读回到了");
        for k in 0..6 {
            both(
                &mut full,
                &mut kept,
                KeyCode::PageUp,
                &format!("{at}，接着上翻第 {k} 下"),
            );
        }
        back_down(&mut full, &mut kept, &at);
    }
}

#[test]
fn paging_down_into_a_gap_before_it_is_read_back_matches() {
    // 先 `/jump` 跳到中间（那一段读回来了，下面的段打开以后一直没排过，行数是估的），再往下翻进空档。
    let events = session(120);
    for more in [0, 3] {
        let (mut full, mut kept) = pair(&events);
        assert_eq!(open_jump(&mut full), open_jump(&mut kept), "开了 /jump");
        for k in 0..70 {
            both(
                &mut full,
                &mut kept,
                KeyCode::Up,
                &format!("往前选第 {k} 下"),
            );
        }
        both(&mut full, &mut kept, KeyCode::Enter, "跳到中间");
        let mut late = Vec::new();
        into_gap(&mut full, &mut kept, KeyCode::PageDown, more, &mut late);
        let at = format!("下翻进空档再 {more} 下");
        assert_eq!(full.draw(), release(&mut kept, &mut late), "{at}，读回到了");
        back_down(&mut full, &mut kept, &at);
    }
}

#[test]
fn turning_back_inside_a_gap_before_it_is_read_back_matches() {
    // 上翻几屏都落在空档里，再往回翻两下（还在空档里），读回才到。
    let events = session(120);
    let (mut full, mut kept) = pair(&events);
    let mut late = Vec::new();
    into_gap(&mut full, &mut kept, KeyCode::PageUp, 6, &mut late);
    for _ in 0..2 {
        full.key(KeyCode::PageDown);
        held(&mut kept, KeyCode::PageDown, &mut late);
        assert!(in_gap(&kept), "往回翻了还在空档里");
    }
    assert_eq!(full.draw(), release(&mut kept, &mut late), "读回到了");
    back_down(&mut full, &mut kept, "空档里往回翻");
}

#[test]
fn a_new_turn_while_waiting_on_a_read_back_matches() {
    // 上翻进空档、读回还没到时又来了一轮（推送一条条到，每条画一帧）。
    let events = session(100);
    let extra = one_more(&events, false);
    let (mut full, mut kept) = pair(&events);
    let mut late = Vec::new();
    into_gap(&mut full, &mut kept, KeyCode::PageUp, 2, &mut late);
    full.events.extend_from_slice(&extra);
    kept.events.extend_from_slice(&extra);
    for push in replayed(&extra) {
        full.app.core(Update::Push(push.clone()));
        full.draw();
        kept.app.core(Update::Push(push));
        kept.draw();
        late.append(&mut kept.app.talk.wanted);
    }
    assert_eq!(
        full.settle(),
        release(&mut kept, &mut late),
        "来了一轮，读回才到"
    );
    for k in 0..4 {
        both(
            &mut full,
            &mut kept,
            KeyCode::PageUp,
            &format!("再上翻第 {k} 下"),
        );
    }
    back_down(&mut full, &mut kept, "来了一轮以后");
}

#[test]
fn a_jump_while_a_page_read_back_is_late_matches() {
    // 上翻进空档、读回没到时 `/jump` 跳到前面：翻页要的那一截晚到（跳的那一段先到、后到各一次），两边逐格一样。
    let events = session(120);
    for jump_first in [true, false] {
        let (mut full, mut kept) = pair(&events);
        let mut late = Vec::new();
        into_gap(&mut full, &mut kept, KeyCode::PageUp, 2, &mut late);
        assert_eq!(open_jump(&mut full), open_jump(&mut kept), "开了 /jump");
        for k in 0..80 {
            both(
                &mut full,
                &mut kept,
                KeyCode::Up,
                &format!("往前选第 {k} 下"),
            );
        }
        let at = if jump_first {
            "跳的先到"
        } else {
            "翻页的先到"
        };
        full.key(KeyCode::Enter);
        let mut jumped = Vec::new();
        held(&mut kept, KeyCode::Enter, &mut jumped);
        assert!(!jumped.is_empty(), "跳的那一段要读回");
        if jump_first {
            jumped.append(&mut late);
            late = jumped;
        } else {
            late.append(&mut jumped);
        }
        assert_eq!(full.draw(), release(&mut kept, &mut late), "{at}：跳过去了");
        for k in 0..3 {
            both(
                &mut full,
                &mut kept,
                KeyCode::PageUp,
                &format!("{at}：上翻第 {k} 下"),
            );
        }
        back_down(&mut full, &mut kept, at);
    }
}

#[test]
fn a_resize_while_waiting_on_a_read_back_keeps_the_seen_lines() {
    // 上翻进空档、读回没到时窗口变窄：视口里露着的那一条（空档下面的）照旧在原来那一行，读回到了也不动。
    let events = session(120);
    let (mut full, mut kept) = pair(&events);
    let mut late = Vec::new();
    into_gap(&mut full, &mut kept, KeyCode::PageUp, 0, &mut late);
    full.resize(50, 24);
    (kept.width, kept.height) = (50, 24);
    let before = kept.draw();
    late.append(&mut kept.app.talk.wanted);
    let after = release(&mut kept, &mut late);
    let seen: Vec<_> = before
        .iter()
        .zip(&after)
        .skip_while(|(b, _)| b.trim().is_empty())
        .collect();
    assert!(!seen.is_empty(), "空档下面那一条露在视口里");
    for (k, (b, a)) in seen.iter().enumerate() {
        assert_eq!(b, a, "露着的第 {k} 行读回以后不动");
    }
    // 回到底（视口顶上差着估错的那几行，翻回去的路上不比）再翻：和全留的又逐格一样。
    for ui in [&mut full, &mut kept] {
        ui.app.screen.view.follow();
    }
    assert_eq!(full.settle(), kept.settle(), "变窄以后回到底");
    for k in 0..8 {
        both(
            &mut full,
            &mut kept,
            KeyCode::PageUp,
            &format!("变窄、回底以后上翻第 {k} 下"),
        );
    }
    back_down(&mut full, &mut kept, "变窄以后再回底");
}
