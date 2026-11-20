//! 正文只留几屏（红线 5，`src/app/window.rs`）在真程序里走一遍：同一个几百轮的会话拷两份，一个程序全留
//! （`LITEMIYU_KEEP_SCREENS=all`）、一个只留 2 屏，一起打开、PgUp 翻到顶、翻回底、`/jump` 到中间一句、说一句新的、再翻上去，
//! 每按一下等全留的那一个画完这一帧，只留几屏的那一个（读回到了以后）画出来的正文逐行一样（新说的那一轮收尾行里当场掐的
//! 用时、出字速度两个程序各掐各的，不比）。

use crate::replies::says;
use crate::support::made::{REPLY, make, said};
use crate::support::{Options, Session, served_by};

/// 几轮。
const TURNS: usize = 240;
const PAGE_UP: &[u8] = b"\x1b[5~";
const PAGE_DOWN: &[u8] = b"\x1b[6~";
const UP: &[u8] = b"\x1b[A";

/// 新说的那一句，她回的。
const NEW_REPLY: &str = "新说的那一句收到了：链接顺序已经换好，重新编过一遍没有缺符号。";

fn new_reply(_: &str) -> String {
    says(&[NEW_REPLY])
}

/// 两个程序一起按 `key`：等全留的画完这一帧，再等只留几屏的画成一样（读回在路上的先画空档）。交回这一帧和上一帧比变没变。
fn both(full: &Session, kept: &Session, key: &[u8], what: &str) -> bool {
    let before = full.body();
    let (a, b) = (full.written(), kept.written());
    full.press(key);
    kept.press(key);
    let now = full.frame_after(a);
    kept.frame_after(b);
    same(full, kept, what);
    now != before
}

/// 正文，新说的那一轮的收尾行里模型后面那一截不比：用时、出字的速度是两个程序各自当场掐的表（从日志补发的那些两边一样）。
fn body(one: &Session) -> String {
    one.body()
        .lines()
        .map(|line| match line.find("fake/m1") {
            Some(at) => line[..at].to_string(),
            None => line.to_string(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// 等只留几屏的画成和全留的一样（读回在路上的先画空档）；三十秒还不一样的，两份都印出来。
fn same(full: &Session, kept: &Session, what: &str) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    while body(kept) != body(full) {
        assert!(
            std::time::Instant::now() < deadline,
            "{what}：只留几屏的画得和全留的不一样\n全留：\n{}\n只留几屏：\n{}",
            full.screen(),
            kept.screen()
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

/// 两个程序一起打字（不比：输入框不在比的那几行里）。
fn type_both(full: &Session, kept: &Session, text: &str) {
    full.type_text(text);
    kept.type_text(text);
}

#[test]
fn a_long_session_kept_to_a_few_screens_draws_like_keeping_it_all() {
    let seed = std::env::temp_dir().join(format!("litemiyu-pty-window-{}", std::process::id()));
    make(&seed, TURNS);
    let base = served_by(new_reply);
    // 输入框空着时轮换的小贴士两个程序各挑各的，长的那条折成两行时输入框高一行、正文矮一行，PgUp 翻的行数就差一行：
    // 照 Windows Terminal 那样一条都不写（`src/tips.rs`，只管小贴士）。
    let options = |keep: &'static str| Options {
        env: vec![("LITEMIYU_KEEP_SCREENS", keep), ("WT_SESSION", "1")],
        ..Options::default()
    };
    let full = Session::seeded(&seed, &base, &options("all"));
    let kept = Session::seeded(&seed, &base, &options("2"));
    drop(std::fs::remove_dir_all(&seed));
    for one in [&full, &kept] {
        one.wait_for("❯", "输入框");
        one.type_text("/sessions");
        one.press_enter();
        one.wait_for("未打开", "列表里有造好的那个会话");
        one.press_enter();
        one.wait_for(&said(TURNS), "打开了，最后一句画出来了");
    }
    same(&full, &kept, "打开以后底下一样");
    assert!(full.body().contains(REPLY));
    let mut pages = 0;
    while both(&full, &kept, PAGE_UP, &format!("上翻第 {pages} 下")) {
        pages += 1;
        assert!(pages < 3000, "翻不到顶");
    }
    assert!(full.body().contains(&said(1)), "到顶了：{}", full.body());
    let mut down = 0;
    while both(&full, &kept, PAGE_DOWN, &format!("下翻第 {down} 下")) {
        down += 1;
        assert!(down < 3000, "翻不回底");
    }
    // `/jump` 到中间一句：列表开着时往前选一百句、回车跳过去。
    type_both(&full, &kept, "/jump");
    both(&full, &kept, b"\r", "开 /jump");
    same(&full, &kept, "/jump 的列表列全了");
    for k in 0..TURNS / 2 {
        both(&full, &kept, UP, &format!("往前选第 {k} 句"));
    }
    both(&full, &kept, b"\r", "跳到中间一句");
    let middle = said(TURNS - TURNS / 2);
    assert!(full.body().contains(&middle), "跳到了：{}", full.body());
    // 说一句新的：回到底、她回完，一样。
    type_both(&full, &kept, "再编一次看看还缺不缺符号");
    full.press_enter();
    kept.press_enter();
    for one in [&full, &kept] {
        one.wait_for(NEW_REPLY, "新说的那一句她回了");
    }
    same(&full, &kept, "说完一句以后一样");
    for k in 0..8 {
        both(&full, &kept, PAGE_UP, &format!("说完以后上翻第 {k} 下"));
    }
}
