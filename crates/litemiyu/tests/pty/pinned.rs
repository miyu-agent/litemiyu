//! 往上翻过时正文顶上钉一行这一段对应的你说的话，下面空一行，点一下跳回那句话（`ui/pinned.rs`；照 Miyu 终端
//! `tests/pinned.rs`，去掉挂载位藏起来的那一条：LiteMiyu 没有挂载位的配置）。

use std::time::Duration;

use crate::replies::says;
use crate::support::{Options, Session, served};

/// 没有侧边栏的宽度：屏幕上只有正文里、钉着的那一行里有你说的话。
const COLS: u16 = 100;

/// 一段回答：每段一行，`tag` 打头好认；每段写得比问的话长，钉着的那一行盖不住时露得出来。
fn long(tag: &str) -> String {
    (1..=10)
        .map(|i| {
            format!(
                "{tag} 第 {i} 段。这一段写得比问的那句话长一些，好看出下面的字有没有露出来。\n\n"
            )
        })
        .collect()
}

/// 有这句的那几行。
fn rows_with(lines: &[String], word: &str) -> Vec<usize> {
    lines
        .iter()
        .enumerate()
        .filter(|(_, l)| l.contains(word))
        .map(|(i, _)| i)
        .collect()
}

fn lines(session: &Session) -> Vec<String> {
    session.screen().lines().map(str::to_string).collect()
}

#[test]
fn scrolled_up_the_top_pins_the_question_and_a_click_jumps_back() {
    let server = served(vec![
        says(&[&long("甲")]),
        says(&[&long("乙")]),
        says(&[&long("丙")]),
    ]);
    let options = Options {
        cols: Some(COLS),
        rows: Some(30),
        ..Options::default()
    };
    let session = Session::start_with(&server.base_url, &[], &options);
    session.wait_for("❯", "输入框");
    for (said, last) in [
        ("第一问 alpha", "甲 第 10 段。"),
        ("第二问 beta", "乙 第 10 段。"),
        ("第三问 gamma", "丙 第 10 段。"),
    ] {
        session.type_text(said);
        session.press_enter();
        // 长回答分几帧到的会替人停在回答开头：按 Ctrl+End 回到最底下再看（同 `sidebar.rs`）。
        session.wait_until(last, |s| {
            let screen = s.screen();
            if screen.contains(last) && screen.contains(crate::support::DONE) {
                return true;
            }
            s.press(b"\x1b[1;5F");
            false
        });
    }
    // 跟着最新的：不钉（第二问早滚出去了，屏幕上没有它）。
    assert!(
        rows_with(&lines(&session), "第二问 beta").is_empty(),
        "跟着最新的不钉：\n{}",
        session.screen()
    );
    // 正文上滚轮一格一格往上，直到第二问整句滚出视口、顶上钉着它：只有正文区第一行有它，下面空一行，再下面露着乙的回答。
    let mut pinned = None;
    for _ in 0..40 {
        session.press(b"\x1b[<64;20;10M");
        std::thread::sleep(Duration::from_millis(80));
        let lines = lines(&session);
        let at = rows_with(&lines, "第二问 beta");
        if at.len() == 1
            && at[0] <= 1
            && lines[at[0] + 1].trim().is_empty()
            && lines[at[0] + 2..at[0] + 7]
                .iter()
                .any(|l| l.contains("乙 第"))
        {
            pinned = Some(at[0]);
            break;
        }
    }
    let row = pinned
        .unwrap_or_else(|| panic!("往上翻到乙的回答，顶上钉着第二问：\n{}", session.screen()));
    let lines_now = lines(&session);
    // 钉着的那一行整行盖住下面的正文：句子后面不露出底下那一行剩下的字（Miyu 2026-11-05 验收）。
    assert!(
        lines_now[row].trim_end().ends_with("第二问 beta"),
        "钉着的那一行后面不该有残字：{:?}",
        lines_now[row]
    );
    // 竖线后面空一格就是字，竖线和字同一个暗色（Miyu 发版前 check 第 6 条）。
    let line = &lines_now[row];
    let bar = line.chars().take_while(|c| *c == ' ').count();
    assert!(
        line.trim_start().starts_with("┃ 第二问"),
        "竖线后面一格就是字：{line:?}"
    );
    let (row16, col) = (
        u16::try_from(row).expect("行号"),
        u16::try_from(bar).expect("列"),
    );
    assert_eq!(
        session.fg(row16, col),
        session.fg(row16, col + 2),
        "竖线和字同一个暗色"
    );
    assert_ne!(
        session.fg(row16, col + 2),
        Some(vt100::Color::Default),
        "字不是默认的亮色"
    );
    // 点钉着的那一行：跳回第二问，它自己那一行顶到视口最上面，不再钉着（正文里你说的话上下各有一行空竖线）。
    session.click(col + 3, row16);
    session.wait_until("点了跳回第二问", |s| {
        let lines = lines(s);
        let at = rows_with(&lines, "第二问 beta");
        at.len() == 1 && (1..=3).contains(&at[0]) && lines[at[0] - 1].trim() == "┃"
    });
}
