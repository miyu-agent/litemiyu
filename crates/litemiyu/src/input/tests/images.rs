//! 图片块（LM-28，Miyu 终端 `input/tests/attach.rs` 里附件那几条，蓝图「输入框」第 12 条）：贴进来的图片收成 `[图片 N]`，整块
//! 跳过、整块删，可以有几张、和字混着排，编号照先后接着正文里已有的；发出去字里照留块上的字，图片的文件另交。

use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyModifiers};

use super::{input, press};
use crate::input::{Draft, Piece, Saved, Sent};

const LABEL: &str = "[图片 {n}]";

fn shot(n: u32) -> PathBuf {
    PathBuf::from(format!("/cache/tui/pasted/1/{n:016x}.png"))
}

#[test]
fn pasted_pictures_are_numbered_blocks_mixed_with_text() {
    let mut i = input(60);
    i.editor.insert("看这张");
    i.attach_image(shot(1), LABEL);
    i.renumber(0, LABEL);
    i.editor.insert("和这张");
    i.attach_image(shot(2), LABEL);
    i.renumber(0, LABEL);
    assert_eq!(i.editor.text(), "看这张[图片 1]和这张[图片 2]");
    let draft = i.draft();
    assert_eq!(draft.images(), [shot(1), shot(2)]);
    assert_eq!(
        draft.expand(),
        "看这张[图片 1]和这张[图片 2]",
        "发出去字里照留块上的字：模型分得出哪张是哪张"
    );
    // 正文里已经有两张了：接着编。
    i.renumber(2, LABEL);
    assert_eq!(i.editor.text(), "看这张[图片 3]和这张[图片 4]");
    // 没有图片块的不动。
    let mut plain = input(60);
    plain.editor.insert("自己打的 [图片 1]");
    plain.renumber(5, LABEL);
    assert_eq!(plain.editor.text(), "自己打的 [图片 1]", "自己打的字不算块");
    assert!(plain.draft().images().is_empty());
}

#[test]
fn a_picture_block_is_deleted_whole_and_the_rest_renumber() {
    let mut i = input(60);
    i.attach_image(shot(1), LABEL);
    i.editor.insert("甲");
    i.attach_image(shot(2), LABEL);
    i.renumber(0, LABEL);
    assert_eq!(i.editor.text(), "[图片 1]甲[图片 2]");
    // 光标到第一块后面，退格：整块删掉。
    press(&mut i, KeyCode::Home, KeyModifiers::NONE);
    press(&mut i, KeyCode::Right, KeyModifiers::NONE);
    assert_eq!(i.editor.cursor(), "[图片 1]".len(), "一下跳过整块");
    press(&mut i, KeyCode::Backspace, KeyModifiers::NONE);
    i.renumber(0, LABEL);
    assert_eq!(i.editor.text(), "甲[图片 1]", "剩下的照先后重编");
    assert_eq!(i.draft().images(), [shot(2)]);
    // 光标在块后面的字里：重编以后还在原来那个字后面。
    let mut i = input(60);
    i.attach_image(shot(1), LABEL);
    i.editor.insert("甲乙");
    i.renumber(9, LABEL);
    assert_eq!(i.editor.text(), "[图片 10]甲乙");
    assert_eq!(
        i.editor.cursor(),
        "[图片 10]甲乙".len(),
        "块变长了，光标跟着挪"
    );
}

#[test]
fn copied_picture_files_become_picture_blocks_and_other_files_stay_file_blocks() {
    let dir = std::env::temp_dir().join(format!("litemiyu-image-files-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let (photo, notes) = (dir.join("晚霞.png"), dir.join("notes.txt"));
    for file in [&photo, &notes] {
        std::fs::write(file, b"x").unwrap();
    }
    let mut i = input(60);
    let text = format!("{}\n{}", photo.display(), notes.display());
    let words = || Some(("[{name}]".to_string(), "[{name}/]".to_string()));
    assert!(i.paste_files(&text, words, LABEL, |path| path == photo));
    i.renumber(0, LABEL);
    assert_eq!(i.editor.text(), "[图片 1] [notes.txt]");
    let draft = i.draft();
    assert_eq!(draft.images(), std::slice::from_ref(&photo));
    assert_eq!(
        draft.expand(),
        format!("[图片 1] {}", notes.display()),
        "图片当图片交，别的文件写路径"
    );
    std::fs::remove_dir_all(&dir).unwrap_or_default();
}

#[test]
fn picture_blocks_survive_the_history_file_and_come_back_from_the_transcript() {
    // 输入历史记的是文件的路径，不记图片的字节。
    let dir = std::env::temp_dir().join(format!("litemiyu-image-saved-{}", std::process::id()));
    let saved = Saved::at(Some(dir.join("history.jsonl")));
    let mut i = input(60);
    i.editor.insert("看");
    i.attach_image(shot(7), LABEL);
    i.renumber(0, LABEL);
    let draft = i.draft();
    saved.append(&Sent {
        draft: draft.clone(),
        at: crate::input::saved::now(),
    });
    let back = saved.load(10);
    assert_eq!(back.len(), 1);
    assert_eq!(back[0].draft, draft);
    assert!(back[0].draft.blocks[0].image);
    std::fs::remove_dir_all(&dir).unwrap_or_default();
    // 被退回、撤销放回的话照正文里记着的块拼：图片块连文件接回来；不知道文件在哪的（应急模式里发的）块上的字照字留着。
    let piece = |file: Option<PathBuf>| Piece {
        label: "[图片 1]".into(),
        full: "[图片 1]".into(),
        image: true,
        file,
    };
    let known = Draft::from_pasted("看[图片 1]", &[piece(Some(shot(7)))]);
    assert_eq!(known, draft);
    let unknown = Draft::from_pasted("看[图片 1]", &[piece(None)]);
    assert_eq!(unknown, Draft::plain("看[图片 1]"));
}

/// 字、一个图片块、字：`甲[图片 1]乙`。
fn picture_between() -> crate::input::InputBox {
    let mut i = input(60);
    i.editor.insert("甲");
    i.attach_image(shot(1), LABEL);
    i.editor.insert("乙");
    i.renumber(0, LABEL);
    assert_eq!(i.editor.text(), "甲[图片 1]乙");
    i
}

#[test]
fn every_way_of_deleting_takes_a_picture_block_whole() {
    // LM-72：用户说「[图片 1] 删除的时候是当成文字的」。每一种删法都整块删，不留半截，图片也跟着不带了。
    let after = "甲[图片 1]".len();
    let before = "甲".len();
    // 光标在块后面：退格、`Ctrl+H`（有的终端把退格键发成它）、`Ctrl+W`、`Alt+退格`、`Ctrl+退格`。
    for (code, modifiers) in [
        (KeyCode::Backspace, KeyModifiers::NONE),
        (KeyCode::Char('h'), KeyModifiers::CONTROL),
        (KeyCode::Char('w'), KeyModifiers::CONTROL),
        (KeyCode::Backspace, KeyModifiers::ALT),
        (KeyCode::Backspace, KeyModifiers::CONTROL),
    ] {
        let mut i = picture_between();
        i.editor.move_to(after, false);
        press(&mut i, code, modifiers);
        assert_eq!(i.editor.text(), "甲乙", "{code:?} {modifiers:?}");
        assert!(i.draft().images().is_empty(), "{code:?} {modifiers:?}");
    }
    // 光标在块前面：`Delete`。
    let mut i = picture_between();
    i.editor.move_to(before, false);
    press(&mut i, KeyCode::Delete, KeyModifiers::NONE);
    assert_eq!(i.editor.text(), "甲乙");
    // `Shift+←` 选：一下选过整块，退格删的是整块。
    let mut i = picture_between();
    i.editor.move_to(after, false);
    press(&mut i, KeyCode::Left, KeyModifiers::SHIFT);
    assert_eq!(i.editor.selection(), Some((before, after)));
    press(&mut i, KeyCode::Backspace, KeyModifiers::NONE);
    assert_eq!(i.editor.text(), "甲乙");
    // 选区的一头拖到块中间：落到近的那一头，选区不会只盖住半块；删的、打字盖掉的是整块或者不碰它。
    let mut i = picture_between();
    i.editor.select(0, before + 4);
    assert_eq!(i.editor.selection(), Some((0, before)), "离块头近");
    press(&mut i, KeyCode::Backspace, KeyModifiers::NONE);
    assert_eq!(i.editor.text(), "[图片 1]乙");
    assert_eq!(i.draft().images(), [shot(1)]);
    let mut i = picture_between();
    i.editor.select(0, after - 2);
    assert_eq!(i.editor.selection(), Some((0, after)), "离块尾近");
    press(&mut i, KeyCode::Char('丙'), KeyModifiers::NONE);
    assert_eq!(i.editor.text(), "丙乙");
    assert!(i.draft().images().is_empty());
    // 块前面紧挨着字、后面紧挨着字：`Ctrl+W` 只删这一块，不连前面的字。
    let mut i = picture_between();
    i.editor.move_to(after, false);
    i.editor.delete_word();
    assert_eq!(i.editor.text(), "甲乙");
}

#[test]
fn the_cursor_never_stops_inside_a_picture_block() {
    // 不管怎么挪（点在中间、按词跳、上下、行首行尾），光标都落在块的两头，在块里打不进字。
    let inside = |i: &crate::input::InputBox| {
        let at = i.editor.cursor();
        at > "甲".len() && at < "甲[图片 1]".len()
    };
    let text = "甲[图片 1]乙";
    for at in (0..=text.len()).filter(|at| text.is_char_boundary(*at)) {
        let mut i = picture_between();
        i.editor.move_to(at, false);
        assert!(!inside(&i), "点在第 {at} 个字节");
        i.editor.insert("丁");
        assert!(
            i.editor.text().contains("[图片 1]"),
            "块没被打断：{}",
            i.editor.text()
        );
    }
    for (code, modifiers) in [
        (KeyCode::Left, KeyModifiers::NONE),
        (KeyCode::Right, KeyModifiers::NONE),
        (KeyCode::Left, KeyModifiers::CONTROL),
        (KeyCode::Right, KeyModifiers::CONTROL),
        (KeyCode::Home, KeyModifiers::NONE),
        (KeyCode::End, KeyModifiers::NONE),
        (KeyCode::Up, KeyModifiers::NONE),
        (KeyCode::Down, KeyModifiers::NONE),
    ] {
        for start in [0, "甲".len(), "甲[图片 1]".len(), text.len()] {
            let mut i = picture_between();
            i.editor.move_to(start, false);
            press(&mut i, code, modifiers);
            assert!(!inside(&i), "从 {start} 按 {code:?} {modifiers:?}");
        }
    }
}

#[test]
fn ctrl_u_and_ctrl_k_do_nothing_in_the_box() {
    // 输入框没有绑 `Ctrl+U`、`Ctrl+K`（不是 readline 那样删到行首、行尾）：按了什么都不变，块也还在。
    for c in ['u', 'k'] {
        let mut i = picture_between();
        i.editor.move_to("甲[图片 1]".len(), false);
        press(&mut i, KeyCode::Char(c), KeyModifiers::CONTROL);
        assert_eq!(i.editor.text(), "甲[图片 1]乙");
        assert_eq!(i.draft().images(), [shot(1)]);
    }
}
