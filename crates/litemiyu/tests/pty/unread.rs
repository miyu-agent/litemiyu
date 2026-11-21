//! 打开会话读不了的两种（界面线第十八趟，照 Miyu 终端 `tests/unread.rs`、核心 YC-2/YC-6 和 `LoadError::passing()`、
//! `broken_at()`）：
//!
//! - 日志中间坏了一行：只读打开坏行以前的，正文末尾写坏在第几行，输入框写怎么新开会话；还要说的被拒、字退回框里、弹「只能看」；
//!   日志一个字节都不动。`/sessions` 打开、`litemiyu -c` 都这样；长会话里 `/jump` 回到放掉的段照样读得回来。
//! - 一时读不了（读日志时读写出错）：弹「会话暂时无法读取，5 秒后自动重试」，不说成坏了，到点自己再开（Miyu `app/unread.rs`）。

use std::path::{Path, PathBuf};

use crate::replies::says;
use crate::support::made::{make, said};
use crate::support::{Options, Session, served_by};

fn answer(_: &str) -> String {
    says(&["回答。"])
}

/// 数据根 `data` 里唯一那个会话的第一段日志。
fn log_of(data: &Path) -> PathBuf {
    let sessions = lm_engine::store::paths::Paths::under(data).sessions();
    let dir = std::fs::read_dir(&sessions)
        .expect("有会话目录")
        .filter_map(Result::ok)
        .find(|e| e.path().is_dir())
        .expect("有一个会话")
        .path();
    dir.join("000000000001.jsonl")
}

/// 把日志里头一个写着 `text` 的那一行换成坏的，交回它是第几行（从 1 数）。
fn break_line_with(log: &Path, text: &str) -> usize {
    let whole = std::fs::read_to_string(log).expect("读得了日志");
    let mut lines: Vec<&str> = whole.lines().collect();
    let at = lines
        .iter()
        .position(|line| line.contains(text))
        .expect("日志里有那一句");
    assert!(at >= 1, "坏在第一行的不算只读：{at}");
    lines[at] = "{坏了";
    std::fs::write(log, lines.join("\n") + "\n").expect("写得进");
    at + 1
}

/// 说完一句：屏幕上的「回答。」多了一个。
fn say(session: &Session, text: &str) {
    let before = session.screen().matches("回答。").count();
    session.type_text(text);
    session.press_enter();
    session.wait_until("她回了", |s| {
        s.screen().matches("回答。").count() > before
    });
}

#[test]
fn a_log_broken_in_the_middle_opens_view_only_from_the_list() {
    let base = served_by(answer);
    let first = Session::start(&base, &[]);
    first.wait_for("❯", "输入框");
    say(&first, "第一句");
    say(&first, "第二句");
    first.type_text("/exit");
    first.press_enter();
    assert!(first.wait_exit(), "退出码 0");
    let log = log_of(first.data());
    let line = break_line_with(&log, "第二句");
    let broken = std::fs::read(&log).expect("读得了");
    let second = first.another(&[]);
    second.wait_for("❯", "输入框");
    second.type_text("/sessions");
    second.press_enter();
    second.wait_for("未打开", "列表里有那个会话");
    second.press_enter();
    second.wait_for(&format!("会话日志第 {line} 行已损坏"), "写坏在第几行");
    let screen = second.wait_for(
        "这个会话只能查看，输入 /new 新建会话",
        "输入框写怎么新开会话",
    );
    assert!(screen.contains("第一句"), "坏行以前的画着：{screen}");
    assert!(!screen.contains("第二句"), "坏行以后的没有：{screen}");
    // 还要说的：不发出去，字退回框里，弹「只能看」。
    second.type_text("再说一句");
    second.press_enter();
    let screen = second.wait_for(
        "这个会话只能查看，不能继续对话。输入 /new 新建会话",
        "说话被拒",
    );
    assert!(screen.contains("再说一句"), "字退回框里：{screen}");
    assert_eq!(
        std::fs::read(&log).expect("读得了"),
        broken,
        "日志一个字节都不动"
    );
}

#[test]
fn a_session_unreadable_for_now_is_retried_in_a_few_seconds() {
    use std::os::unix::fs::PermissionsExt;

    let base = served_by(answer);
    let first = Session::start(&base, &[]);
    first.wait_for("❯", "输入框");
    say(&first, "第一句");
    first.type_text("/exit");
    first.press_enter();
    assert!(first.wait_exit(), "退出码 0");
    let log = log_of(first.data());
    let second = first.another(&[]);
    second.wait_for("❯", "输入框");
    second.type_text("/sessions");
    second.press_enter();
    second.wait_for("未打开", "列表里有那个会话");
    // 列出来以后日志读不了了（同 Miyu 的测试：权限挡住）。
    let was = std::fs::metadata(&log).expect("有日志").permissions();
    std::fs::set_permissions(&log, std::fs::Permissions::from_mode(0o000)).expect("改得了权限");
    if std::fs::read(&log).is_ok() {
        eprintln!("挡不住读（超级用户），跳过");
        std::fs::set_permissions(&log, was).expect("改回去");
        return;
    }
    second.press_enter();
    // 照 Miyu `app/unread.rs`：说几秒后自动再试，不说成坏了。
    let screen = second.wait_for("会话暂时无法读取，5 秒后自动重试", "照一时读不了说");
    std::fs::set_permissions(&log, was).expect("改回去");
    assert!(!screen.contains("坏了"), "不说成坏了：{screen}");
    assert!(
        !screen.contains("打不开这个会话"),
        "不照别的原因说：{screen}"
    );
    assert!(!screen.contains("回答。"), "还没打开：{screen}");
    // 读得了了：到点自己再开，正文是那个会话的。
    second.wait_for("回答。", "到点自动再试，打开了");
}

/// 几轮：底下几屏以外的段放掉（发版的程序留 2 屏），`/jump` 回到第一句要读回。
const TURNS: usize = 60;

#[test]
fn continuing_a_broken_long_session_views_it_and_jumps_back() {
    let seed = std::env::temp_dir().join(format!("litemiyu-pty-unread-{}", std::process::id()));
    make(&seed, TURNS);
    let line = break_line_with(&log_of(&seed), &said(TURNS - 2));
    let base = served_by(answer);
    let session = Session::seeded_with(&seed, &base, &Options::default(), &["-c"]);
    drop(std::fs::remove_dir_all(&seed));
    session.wait_for(
        &format!("会话日志第 {line} 行已损坏"),
        "-c 打开写坏在第几行",
    );
    let screen = session.wait_for(
        "这个会话只能查看，输入 /new 新建会话",
        "输入框写怎么新开会话",
    );
    assert!(
        screen.contains(&said(TURNS - 3)),
        "坏行以前的最后一句画着：{screen}"
    );
    assert!(
        !screen.contains(&said(TURNS - 2)),
        "坏的那一句没有：{screen}"
    );
    assert!(!screen.contains(&said(1)), "第一句在底下几屏以外：{screen}");
    // `/jump` 选到第一句、回车跳过去：它在放掉的段里，照坏行以前的那一截读回来。列的条数是坏行以前说过的几句（日志里你说的
    // 话也照这一截读）。出厂 Vim 按键：`k` 往更早的挪，从最新的一句挪到第一句。
    session.type_text("/jump");
    session.press_enter();
    let listed = TURNS - 3;
    session.wait_for(&format!("{listed} 条"), "列出坏行以前说过的几句");
    session.type_text(&"k".repeat(listed - 1));
    session.wait_until("选中第一句", |s| {
        s.screen()
            .lines()
            .any(|l| l.contains('❯') && l.contains("第 1 句："))
    });
    session.press_enter();
    session.wait_until("跳到第一句", |s| s.body().contains(&said(1)));
}
