//! 侧边栏（执行计划第六节 L4 二六）：宽屏看得到侧边栏和说过的几句，点一句正文跳回那里；窄屏没有侧边栏。会话那几段（Miyu
//! `ui/sidebar.rs`，界面线第二十一趟）：名字、短编号、工作区、上下文、用量，数和底栏、`/status` 对得上；点短编号复制完整编号；
//! 比侧边栏高的滚轮滚它。

use crate::replies::says;
use crate::support::{Options, Session, served};

/// 侧边栏从第几列起（160 列宽、侧边栏 38 列，`layout.json`）：竖线在它左边一列。
const SIDE: u16 = 160 - 38;

/// 一段很长的回答：比一屏高，说过的那句话滚出视口。
fn long() -> String {
    (1..=60).map(|i| format!("第 {i} 段。\n\n")).collect()
}

/// Ctrl+End：回到正文最底下，跟着最新的。
const CTRL_END: &[u8] = b"\x1b[1;5F";

/// 等正文最底下照 `ready` 说好了。长回答分几帧到的，视口替人停在回答开头（`tui.md`「正文」第 1 条），收尾那一行在屏幕下面；
/// 一口气整段到的照旧跟着最新的：分不分几帧看机器忙不忙（全仓并发跑时停过、等不到收尾那一行）。没好就按 Ctrl+End 回到最底下
/// 再看，回答的开头停过一次就不再停。
fn wait_bottom(session: &Session, what: &str, ready: impl Fn(&Session) -> bool) {
    session.wait_until(what, |s| {
        if ready(s) {
            return true;
        }
        s.press(CTRL_END);
        false
    });
}

#[test]
fn a_wide_window_lists_what_was_said_and_a_click_jumps_back() {
    let answer = long();
    // 每一份都回同一段。
    let server = served(vec![says(&[&answer]); 8]);
    let session = Session::start(&server.base_url, &[]);
    session.wait_for("❯", "输入框");
    session.type_text("最早那一句");
    session.press_enter();
    wait_bottom(&session, "第一轮结束（收尾行）", |s| {
        s.columns(0, SIDE - 1).contains(crate::support::DONE)
    });
    session.type_text("后来那一句");
    session.press_enter();
    // 第二段回答也很长：最早那一句滚出了正文（侧边栏里还在），后来那一句也列上了。
    wait_bottom(
        &session,
        "第二轮答完、最早那一句滚出正文",
        |s| {
            let body = s.columns(0, SIDE - 1);
            s.columns(SIDE, 38).contains("后来那一句")
                && !body.contains("最早那一句")
                && !body.contains("后来那一句")
                && body.contains(crate::support::DONE)
                && body.contains("第 60 段。")
        },
    );
    let side = session.columns(SIDE, 38);
    assert!(side.contains("跳转消息"), "宽屏有侧边栏：\n{side}");
    let found = |said: &str| {
        side.find(said)
            .unwrap_or_else(|| panic!("说过的话列在侧边栏：{said}\n{side}"))
    };
    let (early, later) = (found("最早那一句"), found("后来那一句"));
    assert!(early < later, "旧的在上、新的在下：\n{side}");
    // 点最早那一句：跳回正文里那一句。
    let row = session.row_of(SIDE, "最早那一句").expect("侧边栏里那一行");
    session.click(SIDE + 3, row);
    session.wait_until("跳回最早那一句", |s| {
        s.columns(0, SIDE - 1).contains("最早那一句")
    });
}

#[test]
fn a_narrow_window_has_no_sidebar() {
    let server = served(vec![says(&["好的。"]); 4]);
    let options = Options {
        cols: Some(100),
        ..Options::default()
    };
    let session = Session::start_with(&server.base_url, &[], &options);
    session.wait_for("❯", "输入框");
    session.type_text("一句话");
    session.press_enter();
    let screen = session.wait_for(crate::support::DONE, "回合结束");
    assert!(screen.contains("好的。"));
    assert!(!screen.contains("跳转消息"), "窄屏没有侧边栏：\n{screen}");
}

/// 一家供应商、一个写了窗口和价格的模型（同 `status.rs`）：假服务器一次报 120 输入、12 输出，一次 0.36 美元。
fn priced(base_url: &str) -> String {
    format!(
        "[providers.fake]\ndriver = \"openai-chat\"\nbase_url = \"{base_url}\"\nlocal = false\nkey = {{ env = \"LITEMIYU_TEST_KEY\" }}\n\n\
         [providers.fake.models.\"m1\"]\nwindow = 128000\nprice = {{ input = 2000.0, output = 10000.0 }}\n\n[models]\nchat = \"fake/m1\"\n"
    )
}

/// 侧边栏一行行（去掉缩进、行尾空格；最右边那一列是指针停在侧边栏上时画的滚动条，不要）。
fn side_rows(session: &Session) -> Vec<String> {
    session
        .columns(SIDE, 37)
        .lines()
        .map(|l| l.trim().to_string())
        .collect()
}

#[test]
fn a_wide_window_shows_the_session_and_its_numbers_match_the_footer_and_status() {
    let server = served(vec![says(&["第一句的回答"])]);
    let session = Session::launch(Some(&priced(&server.base_url)), &[], &[]);
    session.wait_for("❯", "输入框");
    session.type_text("头一句话");
    session.press_enter();
    session.wait_for("第一句的回答", "答完");
    // 底栏（同 `status.rs`）：上下文 132/128k、累计 132、命中 0%、花费 $0.36。
    session.wait_until("底栏写上下文、累计、花费", |s| {
        s.screen().contains("132/128k(0.1%) · Σ132(C0%) · $0.36")
    });
    session.wait_until("侧边栏写用量和花费", |s| {
        side_rows(s).iter().any(|l| l == "花费 $0.36")
    });
    let rows = side_rows(&session);
    let at = |line: &str| {
        rows.iter()
            .position(|l| l == line)
            .unwrap_or_else(|| panic!("侧边栏有「{line}」：\n{}", rows.join("\n")))
    };
    // 会话：没起名字的写「未命名会话」，下面短编号、这个会话用的预设（LM-29）；接着工作区、上下文、用量，段和段之间空一行，
    // 跳转消息、最近会话在后面。
    assert_eq!(
        rows[1],
        "未命名会话",
        "第一行空着，会话名在第二行：\n{}",
        rows.join("\n")
    );
    let id = rows[2].strip_prefix('#').expect("短编号").to_string();
    assert_eq!(id.chars().count(), 8, "短编号 8 位：{id}");
    assert_eq!(rows[3], "预设 全部功能", "用默认预设的也写");
    assert_eq!(rows[4], "");
    assert_eq!(rows[5], "工作区");
    let context = at("上下文");
    assert_eq!(rows[context - 1], "", "段前空一行");
    assert_eq!(
        rows[context + 1],
        "132 / 128k · 0.1%",
        "同底栏 132/128k(0.1%)"
    );
    assert!(
        rows[context + 2].starts_with('▰') && rows[context + 2].chars().count() == 20,
        "进度条 20 格、用过就亮一格：{}",
        rows[context + 2]
    );
    let usage = at("用量");
    assert_eq!(
        rows[usage + 1..usage + 5],
        [
            "共 132 token",
            "输入 120 · 输出 12",
            "缓存命中 0%",
            "花费 $0.36"
        ],
        "同底栏 Σ132(C0%) · $0.36"
    );
    assert!(
        at("跳转消息") > usage && at("最近会话") > at("跳转消息"),
        "跳转消息在最近会话上面"
    );
    // `/status` 写的是同一份：完整编号的末 8 位是短编号，用量、命中、花费同样的数。
    session.type_text("/status");
    session.press_enter();
    // 侧边栏也写「缓存命中」：等状态框才有的「会话 ID」。
    let screen = session.wait_for("会话 ID", "状态框");
    let full = screen
        .lines()
        .find_map(|l| l.split("会话 ID").nth(1))
        .and_then(|rest| rest.split_whitespace().next().map(str::to_string))
        .unwrap_or_else(|| panic!("状态框有会话 ID：\n{screen}"));
    assert!(full.ends_with(&id), "完整编号 {full} 末 8 位是 {id}");
    for same in [
        "132 / 128k（0.1%）",
        "共 132 token · 输入 120 · 输出 12",
        "$0.36",
    ] {
        assert!(screen.contains(same), "状态框写 {same}：\n{screen}");
    }
    session.press(b"\x1b");
    session.wait_until("Esc 关", |s| !s.screen().contains("会话 ID"));
    // 点短编号：复制完整编号（36 个字符）。
    let row = session
        .row_of(SIDE, &format!("#{id}"))
        .expect("短编号那一行");
    session.click(SIDE + 4, row);
    session.wait_for("已复制 36 个字符", "点短编号复制完整编号");
    session.shot("sidebar");
}

#[test]
fn the_wheel_scrolls_a_sidebar_taller_than_the_window() {
    // Miyu `18d33683`（排期第 14 条）：窗口矮，最近会话排不下；侧边栏上滚轮往下滚，最近会话露出来、上面几段跟着往上走。
    let server = served(vec![says(&["第一句的回答"])]);
    let options = Options {
        rows: Some(20),
        ..Options::default()
    };
    let session = Session::launch_with(Some(&priced(&server.base_url)), &[], &[], &options);
    session.wait_for("❯", "输入框");
    session.type_text("头一句话");
    session.press_enter();
    session.wait_for("第一句的回答", "答完");
    session.wait_until("侧边栏写到花费", |s| {
        side_rows(s).iter().any(|l| l == "花费 $0.36")
    });
    let rows = side_rows(&session);
    assert!(
        !rows.iter().any(|l| l == "最近会话"),
        "矮窗口里最近会话排不下：\n{}",
        rows.join("\n")
    );
    assert_eq!(rows[1], "未命名会话");
    // 指针停在侧边栏上往下滚三格（一格三行）：滚到底，最近会话露出来，会话名滚出去。
    for _ in 0..3 {
        session.press(format!("\x1b[<65;{};5M", SIDE + 5).as_bytes());
    }
    session.wait_until("最近会话露出来", |s| {
        let rows = side_rows(s);
        rows.iter().any(|l| l == "最近会话") && !rows.iter().any(|l| l == "未命名会话")
    });
    // 往上滚回去：会话名又在第二行。
    for _ in 0..9 {
        session.press(format!("\x1b[<64;{};5M", SIDE + 5).as_bytes());
    }
    session.wait_until("滚回顶上", |s| {
        side_rows(s).get(1).is_some_and(|l| l == "未命名会话")
    });
}
