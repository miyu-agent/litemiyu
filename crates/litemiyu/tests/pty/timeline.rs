//! 一段带四步工具调用的对话（`read`、`shell`、`edit`、`shell` 各一下，中间夹着回答）在伪终端里跑完，看时间线的样子。
//! 设了 `LITEMIYU_SHOTS` 指一个目录的，把最后的屏幕存进去：带颜色转义的原样一份（`timeline.ansi`）、去掉转义的一份
//! （`timeline.txt`），给人看时间线的样子。

use crate::replies::{calls, says, says_then_calls};
use crate::support::{Options, Session, served};

#[test]
fn four_tools_with_words_between_end_in_one_timeline() {
    let server = served(vec![
        says_then_calls("先看看笔记。", "read", r#"{"file_path":"notes.txt"}"#),
        says_then_calls(
            "再找找还有哪里写着 TODO。",
            "shell",
            r#"{"command":"grep -rn TODO .","description":"Find TODO"}"#,
        ),
        says_then_calls(
            "只有笔记里这一处，改掉。",
            "edit",
            r#"{"file_path":"notes.txt","edits":[{"old_string":"TODO: 买牛奶","new_string":"DONE: 买牛奶"}]}"#,
        ),
        says_then_calls(
            "跑一下看看改好了没有。",
            "shell",
            r#"{"command":"cat notes.txt","description":"Show the notes"}"#,
        ),
        says(&[
            "改好了：`notes.txt` 里的 **TODO** 换成了 DONE，",
            "别的文件没有要改的。",
        ]),
    ]);
    let options = Options {
        env: vec![("COLORTERM", "truecolor")],
        answer: Some(("c0c0/caca/f5f5", "1a1a/1b1b/2626")),
        sixteen: Some(crate::colors::TOKYO),
        ..Options::default()
    };
    let notes = "购物清单\nTODO: 买牛奶\n";
    let session = Session::start_with(&server.base_url, &[("notes.txt", notes)], &options);
    session.wait_for("❯", "输入框");
    session.type_text("把笔记里的 TODO 改成 DONE");
    session.press_enter();
    session.wait_for("别的文件没有要改的。", "最后的回答");
    let screen = session.wait_for("✻", "回合结束的收尾行");
    for said in [
        "先看看笔记。",
        "再找找还有哪里写着 TODO。",
        "跑一下看看改好了没有。",
    ] {
        assert!(
            screen.contains(said),
            "中间的回答还在正文里：{said}\n{screen}"
        );
    }
    let edited = std::fs::read_to_string(session.work().join("notes.txt")).expect("读得到");
    assert_eq!(edited, "购物清单\nDONE: 买牛奶\n", "真的改了文件");
    if let Some(dir) = std::env::var_os("LITEMIYU_SHOTS") {
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).expect("建得了目录");
        std::fs::write(dir.join("timeline.ansi"), session.formatted()).expect("写得进");
        std::fs::write(dir.join("timeline.txt"), screen).expect("写得进");
    }
}

#[test]
fn a_running_step_under_the_finished_ones_one_per_line() {
    // 样张「跑到一半」：她说一句以后接连调三件工具，read、找 TODO 的 shell 做完了一步一行带连接线，等构建的 shell 那一步
    // 正在跑（`sh -c 'sleep 4'` 自己会退出，不留进程）。
    let server = served(vec![
        says_then_calls(
            "先看看笔记、找找 TODO，再跑一下构建。",
            "read",
            r#"{"file_path":"notes.txt"}"#,
        ),
        calls(
            "shell",
            r#"{"command":"grep -n TODO notes.txt","description":"Find TODO"}"#,
        ),
        calls(
            "shell",
            r#"{"command":"sh -c 'sleep 4'","description":"Wait for the build"}"#,
        ),
        says(&["构建跑完了。"]),
    ]);
    let options = Options {
        env: vec![("COLORTERM", "truecolor")],
        answer: Some(("c0c0/caca/f5f5", "1a1a/1b1b/2626")),
        sixteen: Some(crate::colors::TOKYO),
        ..Options::default()
    };
    let notes = "购物清单\nTODO: 买牛奶\n";
    let session = Session::start_with(&server.base_url, &[("notes.txt", notes)], &options);
    session.wait_for("❯", "输入框");
    session.type_text("看看笔记，再跑一下构建");
    session.press_enter();
    let screen = session.wait_for("Wait for the build", "正在跑的那一步");
    assert!(
        screen.contains("先看看笔记、找找 TODO"),
        "前面的回答还在：\n{screen}"
    );
    if let Some(dir) = std::env::var_os("LITEMIYU_SHOTS") {
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).expect("建得了目录");
        std::fs::write(dir.join("timeline-running.ansi"), session.formatted()).expect("写得进");
        std::fs::write(dir.join("timeline-running.txt"), &screen).expect("写得进");
    }
    session.wait_for("构建跑完了。", "命令自己退出、她接着说完");
}
