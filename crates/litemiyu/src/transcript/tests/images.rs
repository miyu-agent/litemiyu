//! 你说的话带的图片（LM-28，Miyu 终端 `transcript/attached.rs`，蓝图「输入框」第 12 条）：界面不画图，画成 `[图片 N]` 的块；
//! 补发来的照字里写着的块上的字对上；编号整个会话一直往下排，撤销藏起来的不算，放掉的段里的照旧算。

use std::path::PathBuf;

use super::super::{Chip, Kind, Transcript};
use super::apply;
use crate::core::Push;

fn images(seq: u64, files: &[Option<&str>]) -> Push {
    Push::Images {
        seq,
        files: files.iter().map(|f| f.map(str::to_string)).collect(),
    }
}

fn said(seq: u64, text: &str) -> Push {
    Push::Said {
        seq,
        text: text.into(),
    }
}

fn chips(t: &Transcript, at: usize) -> Vec<(String, Option<PathBuf>)> {
    t.entries[at]
        .pasted
        .iter()
        .filter(|c| c.image)
        .map(|c| (c.label.clone(), c.file.clone()))
        .collect()
}

#[test]
fn a_replayed_message_draws_its_pictures_as_the_blocks_written_in_it() {
    let mut t = Transcript::default();
    apply(
        &mut t,
        vec![
            said(2, "看 [图片 1] 和 [图片 2]"),
            images(2, &[Some("/s/attachments/a.png"), None]),
        ],
    );
    assert_eq!(
        chips(&t, 0),
        [
            (
                "[图片 1]".to_string(),
                Some(PathBuf::from("/s/attachments/a.png"))
            ),
            ("[图片 2]".to_string(), None),
        ]
    );
    assert_eq!(t.entries[0].text, "看 [图片 1] 和 [图片 2]", "字不动");
    assert_eq!(t.image_count(), 2);
    // 字里没写块上的字的（别的语言的界面发的）：接在字后面，编号接着数。
    apply(
        &mut t,
        vec![said(9, "look at [image 1]"), images(9, &[Some("/b.png")])],
    );
    assert_eq!(t.entries[1].text, "look at [image 1] [图片 3]");
    assert_eq!(t.image_count(), 3);
    // 同一条再来一次（重新订阅补发的）不重复画。
    apply(&mut t, vec![images(9, &[Some("/b.png")])]);
    assert_eq!(chips(&t, 1).len(), 1);
}

#[test]
fn a_message_sent_here_keeps_its_blocks_and_takes_the_path_the_session_kept() {
    // 你在这个界面发的那句发之前就画着图片块，指着暂存的截图；会话推来的是它留着的那一份（暂存的过后会删）。
    let mut t = Transcript::default();
    t.user(
        "看 [图片 1]".into(),
        vec![Chip {
            label: "[图片 1]".into(),
            full: "[图片 1]".into(),
            image: true,
            file: Some("/cache/tui/pasted/1/0a.png".into()),
        }],
    );
    apply(
        &mut t,
        vec![
            Push::UserMessage(2),
            images(2, &[Some("/s/attachments/0a.png")]),
        ],
    );
    assert_eq!(
        chips(&t, 0),
        [(
            "[图片 1]".to_string(),
            Some(PathBuf::from("/s/attachments/0a.png"))
        )]
    );
    // 会话没记路径的（应急模式）：照旧指着原来那一份。
    t.user(
        "再看 [图片 2]".into(),
        vec![Chip {
            label: "[图片 2]".into(),
            full: "[图片 2]".into(),
            image: true,
            file: Some("0b.png".into()),
        }],
    );
    apply(&mut t, vec![Push::UserMessage(5), images(5, &[None])]);
    assert_eq!(
        chips(&t, 1),
        [("[图片 2]".to_string(), Some(PathBuf::from("0b.png")))]
    );
    assert_eq!(t.image_count(), 2);
}

#[test]
fn undone_pictures_do_not_count_and_the_undo_line_remembers_them() {
    let mut t = Transcript::default();
    apply(
        &mut t,
        vec![
            said(2, "看 [图片 1]"),
            images(2, &[Some("/s/attachments/a.png")]),
            Push::TurnStarted(3, Some(2)),
            Push::TurnEnded(crate::core::EndReason::Completed),
        ],
    );
    assert_eq!(t.image_count(), 1);
    apply(
        &mut t,
        vec![Push::Reverted {
            turns: vec![3],
            seq: 6,
            cause: None,
        }],
    );
    assert_eq!(t.image_count(), 0, "撤销藏起来的不算：下一张还是图片 1");
    let line = t
        .entries
        .iter()
        .rev()
        .find(|e| e.kind == Kind::Undo)
        .expect("画了撤销说明");
    assert_eq!(line.text, "看 [图片 1]");
    assert_eq!(
        line.pasted
            .iter()
            .map(|c| c.file.clone())
            .collect::<Vec<_>>(),
        [Some(PathBuf::from("/s/attachments/a.png"))],
        "放回输入框时照它接回图片"
    );
}
