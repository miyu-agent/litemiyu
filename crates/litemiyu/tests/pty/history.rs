//! 输入历史记进文件（执行计划第六节 L4 二三）：上次记下的，第一次按 `↑` 才读回来翻得到；这次发的接着记进去。

use crate::replies::says;
use crate::support::{Options, Session, served};

const CTRL_R: &[u8] = b"\x12";
const UP: &[u8] = b"\x1b[A";

#[test]
fn input_history_comes_back_from_the_file_and_new_lines_go_into_it() {
    let server = served(vec![says(&["好的。"]); 4]);
    let options = Options {
        history: Some("{\"at\":1,\"text\":\"上次说的那句\"}\n"),
        ..Options::default()
    };
    let session = Session::start_with(&server.base_url, &[], &options);
    session.wait_for("❯", "输入框");
    session.type_text("\x1b[A");
    session.wait_for("❯ 上次说的那句", "按 ↑ 翻到上次记下的");
    session.type_text("\x1b[B");
    session.type_text("这次说的");
    session.press_enter();
    session.wait_for(crate::support::DONE, "回合结束");
    let file = session.state().join("history.jsonl");
    let text = std::fs::read_to_string(&file).expect("记着输入历史");
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines.len(), 2, "原来那句加这次发的：{text}");
    assert!(lines[1].contains("这次说的"), "{text}");
    // 再按 ↑：这次发的在最前，再往前是上次的，不重复。
    session.type_text("\x1b[A");
    session.wait_for("❯ 这次说的", "翻到这次发的");
    session.type_text("\x1b[A");
    session.wait_for("❯ 上次说的那句", "再往前是上次的");
}

/// Miyu 发版前 check 第 52 条（miyu-tui `29d8b1ec`、`741c4866` 的 `tests/history.rs`，2026-11-12 项目主人：「Ctrl+R 打开的历史的
/// 列表有点太矮了……预览框默认只有一行，导致会因为内容上下来回变化高度」）：框的高度打开时定死，上下选、搜索筛少了都不变；
/// 终端够 100 列时预览在右边，竖线接上上边框（`┬`）和横线（`┴`），140 列开着侧边栏也左右排，80 列照上下排。Miyu 在一个终端里
/// 改窗口大小，这里的伪终端起来就定了宽，照三种宽各起一个。
#[test]
fn ctrl_r_keeps_its_height_and_puts_the_preview_on_the_right_when_wide() {
    // 30 条历史（连进会话说的那句 31 条）：双数的三行、单数的一行（原来露着的几条长短不一，预览就一伸一缩）。
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("时钟在 1970 年以后")
        .as_millis() as u64;
    let saved: String = (0..30u64)
        .rev()
        .map(|n| {
            let text = if n % 2 == 0 {
                format!("第 {n} 条\n第二行\n第三行")
            } else {
                format!("第 {n} 条")
            };
            let line = serde_json::json!({"at": now - n * 60_000, "text": text});
            format!("{line}\n")
        })
        .collect();
    let saved: &'static str = Box::leak(saved.into_boxed_str());
    let start = |cols: u16| {
        let server = served(vec![says(&["好。"])]);
        let options = Options {
            history: Some(saved),
            cols: Some(cols),
            ..Options::default()
        };
        let session = Session::start_with(&server.base_url, &[], &options);
        session.wait_for("❯", "输入框");
        // 先说一句进会话（首页的输入框窄），历史多一条。
        session.type_text("你好");
        session.press_enter();
        session.wait_for("好。", "她回了");
        session.press(CTRL_R);
        session.wait_for("历史  31 条", "历史列表开了");
        session.wait_until("框画全了", |s| box_at(s).is_some());
        (server, session)
    };
    let (_server, wide) = start(180);
    let at = box_rows(&wide);
    let lines = screen_lines(&wide);
    assert!(
        lines[at.0].contains('┬'),
        "宽框预览在右边：\n{}",
        wide.screen()
    );
    assert!(lines.iter().any(|l| l.contains('┴')), "{}", wide.screen());
    for k in 0..5 {
        wide.press(UP);
        step_to(&wide, k);
        assert_eq!(box_rows(&wide), at, "上下选框不变：\n{}", wide.screen());
    }
    // 筛少了：框照旧（按键模式出厂是 Vim：先按 `/` 再打字）。
    wide.type_text("/29");
    wide.wait_for("历史  1 条", "筛到一条");
    assert_eq!(box_rows(&wide), at, "筛少了框不变：\n{}", wide.screen());
    wide.type_text("xyz");
    wide.wait_for("历史  0 条", "一条都对不上");
    assert_eq!(box_rows(&wide), at, "{}", wide.screen());
    // 140 列：侧边栏占掉一块、框里不到 100 列，照终端宽照样左右排（2026-11-12 项目主人：「参考 Claude Code」）。
    let (_server, mid) = start(140);
    let lines = screen_lines(&mid);
    assert!(
        lines[box_rows(&mid).0].contains('┬'),
        "140 列开着侧边栏也左右排：\n{}",
        mid.screen()
    );
    // 80 列：上下排，预览固定 6 行；框还是一打开就定死。
    let (_server, narrow) = start(80);
    let at = box_rows(&narrow);
    assert!(
        !screen_lines(&narrow)[at.0].contains('┬'),
        "窄框上下排：\n{}",
        narrow.screen()
    );
    for k in 0..5 {
        narrow.press(UP);
        step_to(&narrow, k);
        assert_eq!(box_rows(&narrow), at, "上下选框不变：\n{}", narrow.screen());
    }
    narrow.type_text("/29");
    narrow.wait_for("历史  1 条", "筛到一条");
    assert_eq!(box_rows(&narrow), at, "{}", narrow.screen());
}

/// 等选中挪到第 `k` 条（最新的是进会话说的那句，往上是 `第 0 条`、`第 1 条`……）：选中的那一行写着 `❯`。
fn step_to(session: &Session, k: usize) {
    let wanted = format!("第 {k} 条");
    session.wait_until(&format!("选中{wanted}"), |s| {
        screen_lines(s)
            .iter()
            .any(|l| l.contains('❯') && l.contains(&wanted))
    });
}

/// 屏幕上的每一行。
fn screen_lines(session: &Session) -> Vec<String> {
    session.screen().lines().map(str::to_string).collect()
}

/// 框的上边、下边在屏幕第几行（上边写着「历史」，下边是它下面第一条 `╰` 打头的线：按键提示在搜的时候换成打字的那一句）。
fn box_rows(session: &Session) -> (usize, usize) {
    box_at(session).unwrap_or_else(|| panic!("框开着、上下边都在：\n{}", session.screen()))
}

/// 同 [`box_rows`]，框没画全的是 `None`。
fn box_at(session: &Session) -> Option<(usize, usize)> {
    let lines = screen_lines(session);
    let top = lines.iter().position(|l| l.contains("历史  "))?;
    let bottom = (top..lines.len()).find(|&r| lines[r].trim_start().starts_with('╰'))?;
    Some((top, bottom))
}
