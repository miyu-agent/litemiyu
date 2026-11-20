//! `litemiyu ask` 第二趟补的：`--file` 把路径接在话后面（同全屏 `Ctrl+V` 收的文件块）、编辑那一块
//! 印 `-`/`+`。`litemiyu compact` 那条随子命令删了（LM-3），带要求压缩挪到伪终端 `commands.rs` 查。

use crate::cli::{Home, Input, serve, stream};
use crate::replies::{calls, says};

#[test]
fn file_paths_follow_the_message_and_missing_files_are_refused() {
    let server = serve(vec![stream(says(&["看了"])), stream(says(&["题"]))]);
    let home = Home::new(Some(&server.base_url));
    std::fs::write(home.work.join("a b.txt"), "x").expect("写得进");
    std::fs::write(home.work.join("c.txt"), "y").expect("写得进");
    let out = home.run(
        &["ask", "--file", "a b.txt", "--file", "c.txt", "看看这两个"],
        Input::Null,
    );
    assert_eq!(out.code, Some(0), "{out:?}");
    let talk = server.talks().pop().expect("说了");
    let work = home.work.display().to_string();
    let wanted = format!("看看这两个 '{work}/a b.txt' {work}/c.txt");
    let wanted = serde_json::to_string(&wanted).expect("能写成 JSON");
    assert!(talk.contains(wanted.trim_matches('"')), "{talk}");

    let out = home.run(&["ask", "--file", "nope.txt", "看看"], Input::Null);
    assert_eq!(out.code, Some(1), "{out:?}");
    assert!(
        out.stderr.contains("文件不存在：nope.txt"),
        "{}",
        out.stderr
    );
    assert_eq!(server.talks().len(), 1, "不在的不开会话、不发");
}

#[test]
fn an_edit_prints_the_removed_and_added_lines() {
    let server = serve(vec![
        stream(calls("read", r#"{"file_path":"notes.txt"}"#)),
        stream(calls(
            "edit",
            r#"{"file_path":"notes.txt","edits":[{"old_string":"一\n","new_string":"甲\n乙\n"},{"old_string":"三","new_string":"丙"}]}"#,
        )),
        stream(says(&["改好了"])),
        stream(says(&["题"])),
    ]);
    let home = Home::new(Some(&server.base_url));
    std::fs::write(home.work.join("notes.txt"), "一\n二\n三\n").expect("写得进");
    let out = home.run(&["ask", "改一下"], Input::Null);
    assert_eq!(out.code, Some(0), "{out:?}");
    assert_eq!(out.stdout.trim_end(), "改好了");
    assert!(
        out.stderr.contains("← 编辑 notes.txt")
            && out.stderr.contains("-一\n+甲\n+乙\n…\n-三\n+丙\n"),
        "{}",
        out.stderr
    );
    let notes = std::fs::read_to_string(home.work.join("notes.txt")).expect("在");
    assert_eq!(notes, "甲\n乙\n二\n丙\n");
}
