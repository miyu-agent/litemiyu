//! 文件块（Miyu 终端 `input/tests/attach.rs` 里文件块那几条，附件那几条不搬）：`@` 选的、`Ctrl+V` 复制进来的收成块，发出去
//! 换回路径；点一下打开，悬停记着。

use std::path::PathBuf;

use crossterm::event::{MouseButton, MouseEventKind};

use super::{input, mouse};
use crate::input::{Action, InputBox};

/// 块上的写法（`@` 列表那两句的中文）。
const WORDS: (&str, &str) = ("[{name}]", "[{name}/]");

fn words() -> Option<(String, String)> {
    Some((WORDS.0.to_string(), WORDS.1.to_string()))
}

/// 图片块上的写法；这几条测试里没有图片。
const PICTURE: &str = "[图片 {n}]";

fn no_image(_: &std::path::Path) -> bool {
    false
}

/// 在临时目录里摆几个文件。
fn files(tag: &str, names: &[&str]) -> (PathBuf, Vec<PathBuf>) {
    let dir = std::env::temp_dir().join(format!("litemiyu-files-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let made = names
        .iter()
        .map(|n| {
            let f = dir.join(n);
            std::fs::write(&f, b"x").unwrap();
            f
        })
        .collect();
    (dir, made)
}

#[test]
fn copied_files_become_blocks_that_send_their_paths() {
    let (dir, f) = files("paste", &["links.md", "my notes.txt"]);
    let mut i = input(60);
    let lines: Vec<String> = f.iter().map(|p| p.display().to_string()).collect();
    assert!(i.paste_files(&lines.join("\n"), words, PICTURE, no_image));
    assert_eq!(i.editor.text(), "[links.md] [my notes.txt]");
    assert_eq!(
        i.draft().expand(),
        format!("{} '{}'", f[0].display(), f[1].display()),
        "文件块发出去换回路径，带空格的加引号"
    );
    // 目录照目录的写法。
    let sub = dir.join("子目录");
    std::fs::create_dir_all(&sub).unwrap();
    let mut i = input(60);
    assert!(i.paste_files(&sub.display().to_string(), words, PICTURE, no_image));
    assert_eq!(i.editor.text(), "[子目录/]");
    // 不是本机现成的：什么都不做，照字粘由外面办；写法也不用问。
    let mut i = input(60);
    assert!(!i.paste_files(
        "/nonexistent/a.txt",
        || unreachable!("不是文件不问写法"),
        PICTURE,
        no_image
    ));
    assert!(i.editor.is_empty());
    std::fs::remove_dir_all(&dir).unwrap_or_default();
}

#[test]
fn clicking_a_file_block_in_the_box_opens_it_and_leaves_the_cursor() {
    // Miyu 2026-09-30 项目主人：输入框里还没发出去的块也能点开。点一下打开文件，光标不动；从块上拖是拖选。
    let (dir, f) = files("click", &["a.md"]);
    let mut i = input(20);
    i.editor.insert("看");
    i.paste_files(&f[0].display().to_string(), words, PICTURE, no_image);
    i.editor.insert("吧");
    let end = i.editor.cursor();
    // 框在第 10 列、第 5 行起；「看」占两列，块「[a.md]」占第 2 到 8 列。
    let at = |i: &mut InputBox, kind, col: u16| i.mouse(mouse(kind, 10 + col, 5), true);
    let down = MouseEventKind::Down(MouseButton::Left);
    let up = MouseEventKind::Up(MouseButton::Left);
    at(&mut i, MouseEventKind::Moved, 5);
    assert_eq!(i.hovered_attachment(), Some((3, 9)), "悬停在块上");
    at(&mut i, MouseEventKind::Moved, 9);
    assert_eq!(i.hovered_attachment(), None);
    assert_eq!(at(&mut i, down, 7), Action::None);
    assert_eq!(at(&mut i, up, 7), Action::Open(f[0].display().to_string()));
    assert_eq!(i.editor.cursor(), end, "光标不动");
    assert_eq!(i.editor.selection(), None);
    // 从块上按下去拖：拖选，不打开。
    at(&mut i, down, 4);
    i.mouse(
        mouse(MouseEventKind::Drag(MouseButton::Left), 10 + 10, 5),
        true,
    );
    assert_eq!(
        at(&mut i, up, 10),
        Action::Copy(format!("{}吧", f[0].display()))
    );
    assert!(i.editor.selection().is_some(), "选中了");
    // 点块旁边的字：照旧放光标。
    at(&mut i, down, 0);
    at(&mut i, up, 0);
    assert_eq!(i.editor.cursor(), 0);
    // 鼠标挪出框、敲了键：不再悬停。
    at(&mut i, MouseEventKind::Moved, 5);
    i.unhover();
    assert_eq!(i.hovered_attachment(), None);
    std::fs::remove_dir_all(&dir).unwrap_or_default();
}

#[test]
fn a_terminal_paste_of_a_path_stays_text() {
    // Miyu 2026-10-10 项目主人：「ctrl+shift+V 粘贴文字，ctrl+V 粘贴占位符」。终端送来的粘贴照原样是字。
    let (dir, f) = files("terminal", &["a.md"]);
    let path = f[0].display().to_string();
    let mut i = input(60);
    i.paste(&path);
    assert_eq!(i.editor.text(), path, "路径就是路径");
    assert_eq!(i.editor.openable().count(), 0);
    std::fs::remove_dir_all(&dir).unwrap_or_default();
}
