//! 往上翻时正文顶上钉的那一行（`ui/pinned.rs`）：全留和只留几屏画出来一样；钉的是视口最上面那一段对应的你说的话，下面空
//! 一行；跟着最新的、那句话还露着的不钉；点一下跳回那句话。

use crossterm::event::{KeyCode, MouseButton, MouseEvent, MouseEventKind};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::{Ui, both, session};
use crate::transcript::Kind;

/// 钉着的那句话的第一行；没钉的是 `None`。
fn pinned(ui: &Ui) -> Option<String> {
    let id = ui.app.screen.pinned?;
    let entry = ui.app.talk.transcript.entries.iter().find(|e| e.id == id)?;
    assert_eq!(entry.kind, Kind::User, "钉的是你说的话");
    Some(entry.text.lines().next().unwrap_or_default().to_string())
}

/// 画一帧，交回整块画布（要看颜色的用）。
fn buffer(ui: &mut Ui) -> Buffer {
    let mut buffer = Buffer::empty(Rect::new(0, 0, 72, 20));
    crate::ui::draw(&mut crate::ui::Frame::new(&mut buffer), &mut ui.app);
    buffer
}

/// 往上翻到钉着一句话为止，交回那一帧。
fn page_up_until_pinned(full: &mut Ui, kept: &mut Ui) -> Vec<String> {
    for n in 0..40 {
        both(full, kept, KeyCode::PageUp, &format!("上翻第 {n} 下"));
        if pinned(full).is_some() {
            return full.draw();
        }
    }
    panic!("翻了四十下还没钉");
}

#[test]
fn following_the_latest_nothing_is_pinned() {
    let events = session(40);
    let ui = Ui::open(&events, super::super::SCREENS, 72, 20);
    assert!(ui.app.screen.view.top.is_none(), "跟着最新的");
    assert_eq!(pinned(&ui), None);
    assert_eq!(ui.app.screen.areas.pinned, Rect::default());
}

#[test]
fn scrolled_up_the_top_row_pins_the_question_with_a_blank_row_below() {
    let events = session(60);
    let mut full = Ui::open(&events, usize::MAX, 72, 20);
    let mut kept = Ui::open(&events, super::super::SCREENS, 72, 20);
    let screen = page_up_until_pinned(&mut full, &mut kept);
    let said = pinned(&full).expect("钉着");
    assert_eq!(pinned(&kept), Some(said.clone()), "只留几屏的钉同一句");
    let area = full.app.screen.areas.pinned;
    assert_eq!(area.height, 1);
    assert_eq!(area.y, full.app.screen.areas.body.y, "正文区第一行");
    let row = &screen[usize::from(area.y)];
    // 竖线贴正文区左边，后面空一格就是字；放不下的截掉加 `…`。
    let lead = row.trim_start();
    assert!(lead.starts_with("┃ 第 "), "{row:?}");
    let shown = lead.trim_start_matches("┃ ").trim_end_matches('…');
    assert!(
        said.starts_with(shown),
        "写的是那句话的第一行：{row:?} / {said:?}"
    );
    assert_eq!(screen[usize::from(area.y) + 1], "", "下面空一行");
    // 那句话整句都在视口上面：正文里不再露着它。
    assert_eq!(
        screen.iter().filter(|l| l.contains(shown)).count(),
        1,
        "同一句不写两遍：\n{}",
        screen.join("\n")
    );
    // 竖线和字都是暗色，整行铺浅一档的底。
    let buffer = buffer(&mut full);
    let x = u16::try_from(row.len() - lead.len()).unwrap_or(0);
    assert_eq!(x, area.x, "竖线贴正文区左边");
    let (bar, word) = (&buffer[(x, area.y)], &buffer[(x + 2, area.y)]);
    assert_eq!(bar.symbol(), "┃");
    assert_eq!(bar.fg, crate::theme::dim().fg.expect("暗色"));
    assert_eq!(word.fg, bar.fg, "字和竖线同一个暗色");
    let ground = crate::theme::panel().bg.expect("底色");
    assert_eq!(bar.bg, ground);
    assert_eq!(buffer[(area.right() - 1, area.y)].bg, ground, "整行铺底");
    assert_ne!(buffer[(x, area.y + 1)].bg, ground, "下面那一行照正文的底色");
}

#[test]
fn paging_all_the_way_up_and_down_pins_the_same_in_both() {
    let events = session(50);
    let mut full = Ui::open(&events, usize::MAX, 72, 20);
    let mut kept = Ui::open(&events, super::super::SCREENS, 72, 20);
    let mut seen = std::collections::BTreeSet::new();
    let mut pages = 0;
    while full.first() > 0 {
        pages += 1;
        both(
            &mut full,
            &mut kept,
            KeyCode::PageUp,
            &format!("上翻第 {pages} 下"),
        );
        assert_eq!(pinned(&full), pinned(&kept), "上翻第 {pages} 下钉的同一句");
        seen.extend(pinned(&full));
        assert!(pages < 2000, "翻不到顶");
    }
    assert!(seen.len() > 10, "一路上钉过不同的话：{}", seen.len());
    // 到了顶：第一句话露着，不钉。
    assert_eq!(pinned(&full), None);
    while full.app.screen.view.top.is_some() {
        both(&mut full, &mut kept, KeyCode::PageDown, "下翻");
        assert_eq!(pinned(&full), pinned(&kept));
    }
    assert_eq!(pinned(&kept), None, "回到底跟着最新的，不钉");
}

#[test]
fn a_click_on_the_pinned_row_jumps_back_to_that_sentence() {
    let events = session(60);
    for keep in [usize::MAX, super::super::SCREENS] {
        let mut ui = Ui::open(&events, keep, 72, 20);
        let mut other = Ui::open(&events, keep, 72, 20);
        page_up_until_pinned(&mut ui, &mut other);
        let id = ui.app.screen.pinned.expect("钉着");
        let area = ui.app.screen.areas.pinned;
        let click = |kind| MouseEvent {
            kind,
            column: area.x + 3,
            row: area.y,
            modifiers: crossterm::event::KeyModifiers::NONE,
        };
        ui.app.mouse(click(MouseEventKind::Moved));
        assert!(ui.app.pointing(), "停在钉着的那一行上指针变手");
        ui.app.mouse(click(MouseEventKind::Down(MouseButton::Left)));
        ui.app.mouse(click(MouseEventKind::Up(MouseButton::Left)));
        ui.settle();
        let view = &ui.app.screen.view;
        assert_eq!(
            view.rows.start_of(id),
            Some(view.first),
            "那句话顶到视口第一行"
        );
        assert_eq!(pinned(&ui), None, "那句话露着，不再钉");
    }
}

#[test]
fn a_short_body_does_not_pin() {
    let events = session(30);
    // 正文区不到 4 行高：不钉。
    let mut ui = Ui::open(&events, usize::MAX, 72, 9);
    for _ in 0..6 {
        ui.key(KeyCode::PageUp);
        if ui.app.screen.areas.body.height < 4 {
            assert_eq!(pinned(&ui), None);
        }
    }
    assert!(
        ui.app.screen.areas.body.height < 4,
        "这个窗口的正文区不到 4 行"
    );
}
