//! 找回来（蓝图 `tui.md`「按键」`Ctrl+C`、「输入框」第 7 条）：`Ctrl+C` 清掉的那句留最近一份、`↑` 找回；撤销放回的
//! 那句照发出去时的样子，长文、附件还是块。

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::paste::{folding, submit};
use crate::input::{Action, Draft, InputBox};

fn press(i: &mut InputBox, code: KeyCode, modifiers: KeyModifiers) -> Action {
    i.key(KeyEvent::new(code, modifiers))
}

/// 一段带着长文块和附件的话：交回输入框和那个文件。
fn busy_draft() -> InputBox {
    let mut i = folding();
    i.editor.insert("写了半天：");
    i.paste(
        &(1..=12)
            .map(|n| format!("第 {n} 行"))
            .collect::<Vec<_>>()
            .join("\n"),
    );
    i
}

#[test]
fn a_ctrl_c_clear_is_kept_once_outside_the_history_and_up_brings_it_back() {
    // 2026-09-30 项目主人：误按 Ctrl+C，辛苦打的字全没了；清掉的别进历史列表，不然每清一次都污染它。
    let ctrl_c = |i: &mut InputBox| press(i, KeyCode::Char('c'), KeyModifiers::CONTROL);
    let up = |i: &mut InputBox| press(i, KeyCode::Up, KeyModifiers::NONE);
    let mut i = busy_draft();
    i.remember(Draft::plain("发过的一句"));
    let before = i.draft();
    assert_eq!(ctrl_c(&mut i), Action::Cleared);
    assert!(i.editor.is_empty());
    assert_eq!(i.sent().len(), 1, "不进输入历史");
    up(&mut i);
    assert_eq!(i.draft(), before, "↑ 先拿回清掉的，长文还是块");
    i.editor.move_to(0, false);
    up(&mut i);
    assert_eq!(i.editor.text(), "发过的一句", "再按才进历史");
    // 只留最近一份：清两次，拿回的是后一次的。
    let mut i = folding();
    i.remember(Draft::plain("发过的一句"));
    i.editor.insert("甲");
    ctrl_c(&mut i);
    i.editor.insert("乙");
    ctrl_c(&mut i);
    up(&mut i);
    assert_eq!(i.editor.text(), "乙");
    // 发出去一句话以后丢掉：↑ 照旧先翻到发过的。
    i.editor.take();
    i.editor.insert("丙");
    ctrl_c(&mut i);
    i.forget_cleared();
    up(&mut i);
    assert_eq!(i.editor.text(), "发过的一句");
}

#[test]
fn a_line_is_found_in_the_history_as_it_was_sent() {
    // 被退回的排队消息：照正文里的字在输入历史里找发出去时的样子（「输入框」第 8 条）。
    let mut i = busy_draft();
    let sent = submit(&mut i);
    i.remember(sent.clone());
    assert_eq!(i.sent_by_text(&sent.text), Some(sent.clone()));
    assert_eq!(i.sent_by_text("别的话"), None);
}

#[test]
fn a_recalled_line_stays_recalled_until_it_is_edited() {
    // 2026-10-01 项目主人报：往上翻得动、往下翻不回来。翻出来的命令还没改过时不弹列表，界面照 `recalled` 定。
    let mut i = InputBox::new(8, std::time::Duration::from_millis(400));
    // 照画出来的宽度折行：一行放得下这几句，`↑` `↓` 在第一行、最后一行时翻历史。
    i.place(ratatui::layout::Rect::new(0, 0, 40, 8));
    i.remember(Draft::plain("/help"));
    i.remember(Draft::plain("/copy"));
    assert!(!i.recalled(), "没在翻");
    press(&mut i, KeyCode::Up, KeyModifiers::NONE);
    assert_eq!(i.editor.text(), "/copy");
    assert!(i.recalled());
    press(&mut i, KeyCode::Up, KeyModifiers::NONE);
    assert_eq!(i.editor.text(), "/help");
    assert!(i.recalled());
    press(&mut i, KeyCode::Down, KeyModifiers::NONE);
    press(&mut i, KeyCode::Down, KeyModifiers::NONE);
    assert_eq!(i.editor.text(), "", "翻回没发的那句（空的）");
    assert!(!i.recalled());
    press(&mut i, KeyCode::Up, KeyModifiers::NONE);
    press(&mut i, KeyCode::Char('x'), KeyModifiers::NONE);
    assert_eq!(i.editor.text(), "/copyx");
    assert!(!i.recalled(), "改了一个字：照常弹列表");
}
