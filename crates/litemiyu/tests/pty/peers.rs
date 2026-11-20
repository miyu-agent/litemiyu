//! 窗口之间的话（设计第五节第 8 条，L9 界面 一）：一个窗口里她 `send_message` 给另一个窗口开着的会话，那边照别处来的话画
//! 一行「从会话 xxxx 收到消息 · 预览」、被叫醒回一句；订没开着的会话「空了告诉我」，当场写一行「未打开，已停止等待」。

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use crate::replies::{calls, says};
use crate::support::{Session, served_by};

/// 另一个窗口的会话编号：几个测试各用各的。
static TO_OPEN: OnceLock<String> = OnceLock::new();
static TO_CLOSED: OnceLock<String> = OnceLock::new();
static TO_CLOSED_DURING: OnceLock<String> = OnceLock::new();
static TO_OPEN_AFTER: OnceLock<String> = OnceLock::new();
/// 「一轮进行中」那一条的数据目录：假模型回工具结果那一次先等这一边会话的日志里有了 `peer.idle`。
static DURING_DATA: OnceLock<PathBuf> = OnceLock::new();

/// 等一样东西最多多久（同 `support.rs`）。
const WAIT: Duration = Duration::from_secs(30);

/// 数据目录 `data` 里除了 `other` 以外的会话，日志里有没有一条 `peer.idle`（落了盘：界面那一头的推送照落了盘的事件来）。
fn peer_idle_logged(data: &Path, other: &str) -> bool {
    fn has(dir: &Path) -> bool {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return false;
        };
        entries.flatten().any(|entry| {
            let path = entry.path();
            if path.is_dir() {
                return has(&path);
            }
            std::fs::read(&path)
                .is_ok_and(|bytes| bytes.windows(11).any(|w| w == br#""peer.idle""#))
        })
    }
    let dir = lm_engine::store::paths::Paths::under(data).sessions();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy() != other)
        .any(|entry| has(&entry.path()))
}

/// 会话 `session` 的收件目录里还有没读的信（`.json`）：有收件目录、里面一封都没有了的是送到了、读完了。
fn inbox_read(data: &Path, session: &str) -> bool {
    let inbox = lm_engine::store::paths::Paths::under(data)
        .sessions()
        .join(session)
        .join("inbox");
    std::fs::read_dir(inbox).is_ok_and(|entries| {
        !entries
            .flatten()
            .any(|entry| entry.file_name().to_string_lossy().ends_with(".json"))
    })
}

/// 照请求现回：叫她发话的那一个调 `send_message`，收到别处来的话的那一个回一句。
fn answer(body: &str) -> String {
    let tool = body.contains(r#""role":"tool""#);
    if body.contains("发给另一个窗口") {
        return if tool {
            says(&["发过去了。"])
        } else {
            let to = TO_OPEN.get().expect("先记下另一个窗口的会话");
            calls(
                "send_message",
                &format!(r#"{{"to":"{to}","message":"CI 绿了"}}"#),
            )
        };
    }
    if body.contains("等另一个窗口空了") {
        return if tool {
            says(&["等着了。"])
        } else {
            let to = TO_CLOSED.get().expect("先记下另一个窗口的会话");
            calls(
                "send_message",
                &format!(r#"{{"to":"{to}","notify_when_idle":true}}"#),
            )
        };
    }
    if body.contains("盯住关着的那个窗口") {
        let to = TO_CLOSED_DURING.get().expect("先记下另一个窗口的会话");
        if !tool {
            return calls(
                "send_message",
                &format!(r#"{{"to":"{to}","notify_when_idle":true}}"#),
            );
        }
        // 钉「一轮进行中」：「没开着」的回报落了盘，这一轮才答完（照文件等，最多 30 秒；等不到的答一句测试认得出的）。
        let data = DURING_DATA.get().expect("先记下数据目录");
        let deadline = Instant::now() + WAIT;
        while !peer_idle_logged(data, to) {
            if Instant::now() > deadline {
                return says(&["没等到 peer.idle。"]);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        return says(&["等着了。"]);
    }
    if body.contains("盯住开着的那个窗口") {
        return if tool {
            says(&["等着了。"])
        } else {
            let to = TO_OPEN_AFTER.get().expect("先记下另一个窗口的会话");
            calls(
                "send_message",
                &format!(r#"{{"to":"{to}","notify_when_idle":true}}"#),
            )
        };
    }
    if body.contains("CI 绿了") {
        return says(&["收到那边的话了。"]);
    }
    says(&["在。"])
}

/// 数据目录里唯一的那个会话的编号。
fn only_session(session: &Session) -> String {
    let dir = lm_engine::store::paths::Paths::under(session.data()).sessions();
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .expect("会话目录在")
        .map(|entry| {
            entry
                .expect("读得了")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert_eq!(names.len(), 1, "只有一个会话：{names:?}");
    names.remove(0)
}

#[test]
fn a_message_from_another_window_shows_where_it_came_from_and_wakes_her() {
    let base_url = served_by(answer);
    let other = Session::start(&base_url, &[]);
    other.wait_for("❯", "输入框");
    other.type_text("你好");
    other.press_enter();
    other.wait_for("在。", "另一个窗口的会话开好了");
    TO_OPEN.get_or_init(|| only_session(&other));
    let me = other.another(&[]);
    me.wait_for("❯", "输入框");
    me.type_text("发给另一个窗口说 CI 绿了");
    me.press_enter();
    me.wait_for("发过去了。", "这边发出去了");
    let screen = other.wait_for("收到消息 · CI 绿了", "那边照别处来的话画了一行");
    other.shot("peer");
    assert!(screen.contains("从会话 "), "来处写是哪个会话：{screen}");
    other.wait_for("收到那边的话了。", "那边被叫醒、回了一句");
}

/// 「没开着」的回报和这一轮收尾谁先到不一定（引擎另起任务查对方开没开，`lm-engine` `peers/watches.rs::place`）：先到的进时间线
/// （收起那一行数「1 条回报」，Miyu 发版前 check 21，`b91ede91`），后到的单独一行（Miyu `job_from`）。两种先后都看得见那句话：
/// 照两种都成立的等，先后各钉一条在下面。
#[test]
fn waiting_on_a_session_no_window_has_open_says_it_is_not_open() {
    let base_url = served_by(answer);
    let other = Session::start(&base_url, &[]);
    other.wait_for("❯", "输入框");
    other.type_text("你好");
    other.press_enter();
    other.wait_for("在。", "另一个窗口的会话开好了");
    TO_CLOSED.get_or_init(|| only_session(&other));
    other.type_text("/exit");
    other.press_enter();
    assert!(other.wait_exit(), "退出码 0");
    let me = other.another(&[]);
    me.wait_for("❯", "输入框");
    me.type_text("等另一个窗口空了告诉我");
    me.press_enter();
    me.wait_for("等着了。", "这一轮答完");
    me.wait_until(
        "没开着的回报：单独一行，或者收起那一行数着 1 条回报",
        |s| s.screen().contains("未打开，已停止等待") || counted(s).is_some(),
    );
    if !me.screen().contains("未打开，已停止等待") {
        let row = counted(&me).expect("收起那一行在");
        me.click(12, row);
    }
    me.wait_for("未打开，已停止等待", "看得到没开着那一句");
}

/// 收起那一行数着 1 条回报的是第几行（中文、英文的收起行都认）。
fn counted(s: &Session) -> Option<u16> {
    s.row_of(0, "1 report").or_else(|| s.row_of(0, "1 条回报"))
}

/// 钉「一轮进行中」：假模型等这一边的日志里有了 `peer.idle` 才答「等着了。」，回报一定先于收尾到，记成时间线里的一步：
/// 收起那一行数「1 条回报」，屏上没有单独的那一行，点开那一段看得到它。
#[test]
fn not_open_during_the_turn_is_a_step_of_the_timeline() {
    let base_url = served_by(answer);
    let other = Session::start(&base_url, &[]);
    other.wait_for("❯", "输入框");
    other.type_text("你好");
    other.press_enter();
    other.wait_for("在。", "另一个窗口的会话开好了");
    TO_CLOSED_DURING.get_or_init(|| only_session(&other));
    DURING_DATA.get_or_init(|| other.data().to_path_buf());
    other.type_text("/exit");
    other.press_enter();
    assert!(other.wait_exit(), "退出码 0");
    let me = other.another(&[]);
    me.wait_for("❯", "输入框");
    me.type_text("盯住关着的那个窗口，空了告诉我");
    me.press_enter();
    me.wait_for("等着了。", "回报落了盘以后这一轮答完");
    me.wait_until("收起那一行数着 1 条回报", |s| {
        counted(s).is_some()
    });
    let screen = me.screen();
    assert!(
        !screen.contains("未打开，已停止等待"),
        "一轮进行中来的回报不单独一行：\n{screen}"
    );
    let row = counted(&me).expect("收起那一行在");
    me.click(12, row);
    me.wait_for("未打开，已停止等待", "点开看到没开着那一步");
}

/// 钉「一轮结束以后」：对方窗口开着、闲着，订进名单不上膛（`lm-engine` `peers/waiters.rs`），这一边一轮答完、收尾行出来以后
/// 对方 `/exit`，名单上的回一封「关了」：那句单独一行，在收尾行下面，收起那一行里没有「回报」。
#[test]
fn not_open_after_the_turn_is_a_line_of_its_own() {
    let base_url = served_by(answer);
    let other = Session::start(&base_url, &[]);
    other.wait_for("❯", "输入框");
    other.type_text("你好");
    other.press_enter();
    other.wait_for("在。", "另一个窗口的会话开好了");
    let to = TO_OPEN_AFTER.get_or_init(|| only_session(&other)).clone();
    let me = other.another(&[]);
    me.wait_for("❯", "输入框");
    me.type_text("盯住开着的那个窗口，空了告诉我");
    me.press_enter();
    me.wait_for("等着了。", "这一轮答完");
    me.wait_until("这一轮的收尾行", |s| {
        let screen = s.screen();
        screen
            .split_once("等着了。")
            .is_some_and(|(_, after)| after.contains('✻'))
    });
    // 对方读完了登记的那封信（名单上有了这一边）才关：不然关的时候名单是空的，不回「关了」。
    let deadline = Instant::now() + WAIT;
    while !inbox_read(other.data(), &to) {
        assert!(Instant::now() < deadline, "{WAIT:?} 内对方没读完登记的信");
        std::thread::sleep(Duration::from_millis(20));
    }
    other.type_text("/exit");
    other.press_enter();
    assert!(other.wait_exit(), "退出码 0");
    let screen = me.wait_for("未打开，已停止等待", "对方关了：单独一行");
    let lines: Vec<&str> = screen.lines().collect();
    let at = |text: &str| lines.iter().position(|l| l.contains(text));
    let (Some(end), Some(note)) = (at("✻"), at("未打开，已停止等待")) else {
        panic!("收尾行、那一句都在：\n{screen}");
    };
    assert!(end < note, "那一句在收尾行下面：\n{screen}");
    assert!(
        counted(&me).is_none() && !screen.contains("条回报"),
        "收起那一行里没有回报：\n{screen}"
    );
}
