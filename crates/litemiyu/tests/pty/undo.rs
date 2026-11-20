//! `/undo`（Miyu 终端 `app/takeback.rs` 的 `undo`、`ui/undo_rows.rs`）：撤销上一轮，正文里那一轮藏起来、画一行撤销说明，
//! 那一句放回输入框；改过的文件不动（Miyu 核心 U-1）；还没开会话的当场说。

use crate::replies::{calls, says};
use crate::support::{Session, served};

#[test]
fn undo_hides_the_turn_keeps_the_file_and_puts_the_words_back() {
    let server = served(vec![
        calls(
            "write",
            r#"{"file_path":"made.txt","content":"写进去的字\n"}"#,
        ),
        says(&["写好了这个文件。"]),
    ]);
    let session = Session::start(&server.base_url, &[]);
    session.wait_for("❯", "输入框");
    // 还没开会话：当场说，不去开一个。
    session.type_text("/undo");
    session.press_enter();
    session.wait_for("没有可撤销的回合", "还没开会话");
    session.type_text("帮我写一个文件");
    session.press_enter();
    session.wait_for("写好了这个文件。", "这一轮做完");
    let made = session.work().join("made.txt");
    assert!(made.exists(), "她写了文件");
    session.type_text("/undo");
    session.press_enter();
    session.wait_for(
        "↶ 已撤销 · /restore 恢复 · 帮我写一个文件",
        "撤销说明那一行",
    );
    session.wait_until("那一轮藏起来了", |s| {
        !s.screen().contains("写好了这个文件。")
    });
    session.wait_until("那一句放回输入框", |s| {
        s.screen().lines().any(|l| l.contains("❯ 帮我写一个文件"))
    });
    assert!(made.exists(), "改过的文件不动（Miyu 核心 U-1）");
    // 点开：铺底色写那一句全文（多出一行）。
    let row = session.row_of(2, "↶ 已撤销").expect("撤销那一行在屏幕上");
    session.click(14, row);
    session.wait_until("点开写着全文", |s| {
        s.screen()
            .lines()
            .any(|l| l.trim_start().starts_with("帮我写一个文件"))
    });
}

#[test]
fn nothing_left_to_undo_says_so() {
    let server = served(vec![says(&["好的。"])]);
    let session = Session::start(&server.base_url, &[]);
    session.wait_for("❯", "输入框");
    session.type_text("就一句");
    session.press_enter();
    session.wait_for("好的。", "答完");
    session.type_text("/undo");
    session.press_enter();
    session.wait_for("↶ 已撤销", "撤了");
    // 撤销放回的那句先清掉，再撤一次：没有可撤的了。
    session.press(b"\x15");
    session.type_text("/undo");
    session.press_enter();
    session.wait_for("没有可撤销的回合", "照实说");
}

#[test]
fn restore_brings_the_undone_turn_back_and_drops_the_undo_line() {
    let server = served(vec![says(&["这一轮的回答。"])]);
    let session = Session::start(&server.base_url, &[]);
    session.wait_for("❯", "输入框");
    // 还没开会话：当场说，不去开一个。
    session.type_text("/restore");
    session.press_enter();
    session.wait_for("没有可恢复的撤销", "还没开会话");
    session.type_text("说一句再撤");
    session.press_enter();
    session.wait_for("这一轮的回答。", "答完");
    session.type_text("/undo");
    session.press_enter();
    session.wait_for("↶ 已撤销", "撤了");
    session.wait_until("那一轮藏起来了", |s| {
        !s.screen().contains("这一轮的回答。")
    });
    // 放回来的那句全选着：打 `/restore` 换掉它（不拼在后面）。
    session.type_text("/restore");
    session.press_enter();
    session.wait_for("这一轮的回答。", "那一轮回来了");
    session.wait_until("撤销说明那一行去掉了", |s| {
        !s.screen().contains("↶ 已撤销")
    });
    session.type_text("/restore");
    session.press_enter();
    session.wait_for("没有可恢复的撤销", "没有能再恢复的");
}
