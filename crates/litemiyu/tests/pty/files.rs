//! 改过的文件（`ui/sidebar/files.rs`、`app/files_page.rs`）：她写了一个文件、改了它，侧边栏多一段「文件修改」，一个文件
//! 一行写加减的行数；点一行整屏看差异、右边完整的列表，`Esc` 回会话。没有侧边栏的窄窗口用 `/files` 开，列表在上面。

use crate::replies::{calls, says};
use crate::support::{Options, Session, served};

/// 侧边栏从第几列起（160 列宽、侧边栏 38 列）。
const SIDE: u16 = 160 - 38;

/// 两轮：头一轮写 `plan.md`（3 行），第二轮改它一行。
fn two_turns() -> Vec<String> {
    vec![
        calls(
            "write",
            r#"{"file_path":"plan.md","content":"第一步\n第二步\n第三步\n"}"#,
        ),
        says(&["写好了。"]),
        calls(
            "edit",
            r#"{"file_path":"plan.md","edits":[{"old_string":"第二步\n","new_string":"第二步：量一下\n再核对\n"}]}"#,
        ),
        says(&["改好了。"]),
    ]
}

fn talk(session: &Session) {
    session.wait_for("❯", "输入框");
    session.type_text("写个计划");
    session.press_enter();
    session.wait_for("写好了。", "头一轮答完");
    session.type_text("第二步写细一点");
    session.press_enter();
    session.wait_for("改好了。", "第二轮答完");
}

#[test]
fn the_sidebar_lists_changed_files_and_a_click_opens_the_diff_page() {
    let server = served(two_turns());
    let session = Session::start(&server.base_url, &[]);
    talk(&session);
    // 侧边栏：跳转消息下面一段「文件修改」，一个文件一行，写入 3 行、编辑加 2 删 1。
    session.wait_until("侧边栏列出改过的文件", |s| {
        let side = s.columns(SIDE, 38);
        side.contains("文件修改") && side.contains("plan.md") && side.contains("+5 -1")
    });
    let side = session.columns(SIDE, 38);
    let at = |word: &str| {
        side.find(word)
            .unwrap_or_else(|| panic!("侧边栏有「{word}」：\n{side}"))
    };
    assert!(at("跳转消息") < at("文件修改"), "在跳转消息下面：\n{side}");
    assert!(at("文件修改") < at("最近会话"), "在最近会话上面：\n{side}");
    assert!(!side.contains("循环"), "侧边栏没有循环那一段：\n{side}");
    assert_eq!(
        std::fs::read_to_string(session.work().join("plan.md")).expect("写出来了"),
        "第一步\n第二步：量一下\n再核对\n第三步\n",
        "文件真改了"
    );
    // 点那一行：整屏的「文件修改」页，左边两处改动照先后、中间一道细线，右边完整的列表，最下面按键提示。
    let row = session.row_of(SIDE, "plan.md").expect("侧边栏里那一行");
    session.click(SIDE + 4, row);
    let screen = session.wait_for("文件修改 · 1 个文件", "开了那一页");
    assert!(!screen.contains("❯"), "整屏归它，输入框不画：\n{screen}");
    for want in [
        "plan.md  +5 -1",
        "1  + 第一步",
        "3  + 第三步",
        "- 第二步",
        "+ 第二步：量一下",
        "+ 再核对",
        "↑/↓ 切换文件",
        "Esc 返回",
    ] {
        assert!(screen.contains(want), "那一页有「{want}」：\n{screen}");
    }
    let lines: Vec<&str> = screen.lines().collect();
    let line_of = |word: &str| {
        lines
            .iter()
            .position(|l| l.contains(word))
            .expect("有这一行")
    };
    let rule = lines
        .iter()
        .position(|l| l.trim_start().starts_with("────"))
        .expect("两处改动之间一道细线");
    assert!(
        line_of("3  + 第三步") < rule && rule < line_of("- 第二步"),
        "{screen}"
    );
    // `Esc` 回会话：正文、输入框照原样。
    session.press(b"\x1b");
    let back = session.wait_for("❯", "回到会话");
    assert!(
        back.contains("改好了。") && back.contains("跳转消息"),
        "{back}"
    );
    assert!(!back.contains("文件修改 · 1 个文件"), "{back}");
}

#[test]
fn a_narrow_window_opens_the_page_with_the_files_command() {
    let server = served(two_turns());
    let options = Options {
        cols: Some(90),
        rows: Some(30),
        ..Options::default()
    };
    let session = Session::start_with(&server.base_url, &[], &options);
    talk(&session);
    assert!(
        !session.screen().contains("文件修改"),
        "窄窗口没有侧边栏：\n{}",
        session.screen()
    );
    session.type_text("/files");
    session.press_enter();
    let screen = session.wait_for("文件修改 · 1 个文件", "开了那一页");
    let lines: Vec<&str> = screen.lines().collect();
    let line_of = |word: &str| {
        lines
            .iter()
            .position(|l| l.contains(word))
            .unwrap_or_else(|| panic!("有「{word}」：\n{screen}"))
    };
    // 列表在上面，差异在下面。
    assert!(
        line_of("文件修改 · 1 个文件") < line_of("plan.md  +5 -1"),
        "{screen}"
    );
    assert!(
        line_of("plan.md  +5 -1") < line_of("1  + 第一步"),
        "{screen}"
    );
    session.press(b"q");
    session.wait_for("❯", "回到会话");
}

#[test]
fn files_without_any_change_says_so() {
    let server = served(vec![says(&["好的。"])]);
    let session = Session::start(&server.base_url, &[]);
    session.wait_for("❯", "输入框");
    session.type_text("你好");
    session.press_enter();
    session.wait_for("好的。", "答完");
    session.type_text("/files");
    session.press_enter();
    let screen = session.wait_for("这个会话还没改过文件", "提示一句");
    assert!(screen.contains("❯"), "不开那一页：\n{screen}");
}
