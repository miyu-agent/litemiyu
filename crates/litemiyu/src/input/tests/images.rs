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
