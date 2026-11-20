//! `/jump` 在只留几屏时（「做法」第 7 条）：列表和全留时一样全（放掉的段里你说的话照日志读来的列），跳到第 n 句画出来逐格
//! 一样（放掉的那一段先读回再跳），跳过去以后翻上翻下、回到底照旧一样。

use crossterm::event::KeyCode;

use super::{Ui, both, session};
use crate::app::jump::Listing;

/// 打 `/jump`、回车，画稳：交回这一帧。
pub(super) fn open_jump(ui: &mut Ui) -> Vec<String> {
    for c in "/jump".chars() {
        ui.key(KeyCode::Char(c));
    }
    ui.key(KeyCode::Enter)
}

/// 列表里每一条的字和时刻。
fn listed(ui: &Ui) -> Vec<(String, std::time::SystemTime)> {
    ui.app
        .list_items()
        .0
        .iter()
        .map(|s| (s.draft.text.clone(), s.at))
        .collect()
}

#[test]
fn the_jump_list_is_as_full_as_keeping_everything() {
    let events = session(120);
    let mut full = Ui::open(&events, usize::MAX, 72, 20);
    let mut kept = Ui::open(&events, super::super::SCREENS, 72, 20);
    assert_eq!(open_jump(&mut full), open_jump(&mut kept), "开了 /jump");
    assert!(kept.app.typing.history.open && kept.app.typing.history.jump);
    let all = listed(&full);
    assert_eq!(all.len(), 120, "一轮一句");
    assert_eq!(listed(&kept), all, "列的字、时刻一样");
    assert!(
        matches!(kept.app.talk.listing, Listing::Ready(_)),
        "放掉的段照日志读来的列"
    );
    // 关了就扔，放掉照旧。
    both(&mut full, &mut kept, KeyCode::Esc, "关掉列表");
    assert!(matches!(kept.app.talk.listing, Listing::None));
    assert!(
        kept.resident() * 4 < full.resident(),
        "关了以后照旧只留几屏"
    );
}

#[test]
fn jumping_to_the_nth_sentence_matches_and_paging_from_there_too() {
    let events = session(100);
    for back in [99, 61, 30, 3] {
        let mut full = Ui::open(&events, usize::MAX, 72, 20);
        let mut kept = Ui::open(&events, super::super::SCREENS, 72, 20);
        assert_eq!(open_jump(&mut full), open_jump(&mut kept), "开了 /jump");
        for k in 0..back {
            both(
                &mut full,
                &mut kept,
                KeyCode::Up,
                &format!("往前选第 {k} 下"),
            );
        }
        both(
            &mut full,
            &mut kept,
            KeyCode::Enter,
            &format!("跳到倒数第 {} 句", back + 1),
        );
        assert!(full.app.screen.view.top.is_some(), "跳上去了");
        for k in 0..3 {
            both(
                &mut full,
                &mut kept,
                KeyCode::PageUp,
                &format!("跳过去以后上翻第 {k} 下"),
            );
        }
        let mut down = 0;
        while full.app.screen.view.top.is_some() {
            down += 1;
            both(
                &mut full,
                &mut kept,
                KeyCode::PageDown,
                &format!("倒数第 {} 句以后下翻第 {down} 下", back + 1),
            );
            assert!(down < 4000, "翻不回底");
        }
        assert!(kept.resident() * 4 < full.resident(), "回到底以后又放掉了");
    }
}
