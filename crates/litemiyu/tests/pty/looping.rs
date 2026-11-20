//! 循环（设计第五节第 9 条，L5 界面 二）：`/loop 5m <话>` 开一个，那句话当场交进去、她回了；回执弹提示，输入框上面那一行
//! 写下一次几点（侧边栏没有「循环」那一段，2026-11-20 项目主人）；`/loop off` 停掉，标记跟着没了；请求里带着 `schedule_wakeup`。

use crate::replies::{calls, says};
use crate::support::{Session, served};

#[test]
fn a_loop_runs_once_at_once_shows_its_next_time_and_stops() {
    // 开的那一刻交进去的那一句她回一句。
    let server = served(vec![says(&["CI 还在跑。"])]);
    let session = Session::start(&server.base_url, &[]);
    session.wait_for("❯", "输入框");
    session.type_text("/loop 5m 看看 CI");
    session.press_enter();
    session.wait_for("已开始循环：每 5m · 看看 CI", "开了的回执");
    session.wait_for("CI 还在跑。", "交进去的那一句她回了");
    let screen = session.screen();
    assert!(
        screen.contains("看看 CI"),
        "交进去的那句照你说的话画：{screen}"
    );
    // 回执的提示框收了以后（它盖着输入框上面那一行）。
    session.wait_for("\u{f017} ", "输入框上面那一行的记号");
    // 侧边栏画着（有「跳转消息」），里面没有「循环」那一段（2026-11-20 项目主人：「移除右侧边栏的循环部分」）。
    let screen = session.screen();
    assert!(screen.contains("跳转消息"), "侧边栏画着：{screen}");
    assert!(
        !screen.contains("每 5 分钟") && !screen.contains("下次 "),
        "侧边栏没有循环那一段：{screen}"
    );
    session.type_text("/loop off");
    session.press_enter();
    session.wait_for("已停止此会话的循环和定时触发。", "停了的回执");
    // 回执的提示框盖着输入框上面那一行，等提示框收了，那一行的记号也没了。
    session.wait_until("回执收了", |s| {
        !s.screen().contains("已停止此会话的循环和定时触发。")
    });
    assert!(!session.screen().contains('\u{f017}'), "标记没了");
    let first = String::from_utf8_lossy(&server.received()[0]).into_owned();
    assert!(
        first.contains("\"schedule_wakeup\""),
        "请求里带着 schedule_wakeup：{first}"
    );
}

#[test]
fn stopping_without_a_session_says_there_is_none() {
    let server = served(Vec::new());
    let session = Session::start(&server.base_url, &[]);
    session.wait_for("❯", "输入框");
    session.type_text("/loop off");
    session.press_enter();
    session.wait_for("此会话没有循环。", "没开会话也当场答");
    session.type_text("/loop 5m");
    session.press_enter();
    session.wait_for("bad_params", "只写了间隔：参数不对");
    assert!(
        server.received().is_empty(),
        "没为停一个不存在的循环去开会话、发请求"
    );
}

/// LM-20：没开 `/loop` 的会话里她调 `schedule_wakeup`。时间线那一步写显示名「定时触发」和理由、多久以后，不印原名；输入框上面
/// 那一行写到点的时刻；`/loop off` 停得掉，标记跟着没了。
#[test]
fn a_wakeup_she_schedules_shows_its_name_and_time_and_stops() {
    let server = served(vec![
        calls(
            "schedule_wakeup",
            r#"{"delay_seconds":600,"prompt":"看看 CI 过了没有","reason":"CI 大约十分钟"}"#,
        ),
        says(&["十分钟以后我再看。"]),
    ]);
    let session = Session::start(&server.base_url, &[]);
    session.wait_for("❯", "输入框");
    session.type_text("CI 好了告诉我");
    session.press_enter();
    session.wait_for("十分钟以后我再看。", "她回了");
    session.wait_for("\u{f017} ", "输入框上面那一行的记号和时刻");
    // 回答出来以后那一段收起成一行：点开看那一步的标题。
    session.wait_for("Used 1 tool", "时间线上收起的那一步");
    let row = session.row_of(0, "Used 1 tool").expect("收起的那一行");
    session.click(12, row);
    let screen = session.wait_for("定时触发", "那一步写显示名");
    let step = screen
        .lines()
        .find(|line| line.contains("定时触发"))
        .expect("有那一行");
    assert!(
        step.contains("定时触发 · CI 大约十分钟 · 10m 后（"),
        "显示名、理由、多久以后：{step}"
    );
    assert!(!screen.contains("schedule_wakeup"), "不印工具的原名");
    session.type_text("/loop off");
    session.press_enter();
    session.wait_for("已停止此会话的循环和定时触发。", "停了的回执");
    session.wait_until("回执收了", |s| {
        !s.screen().contains("已停止此会话的循环和定时触发。")
    });
    // 点开的那一步的图标也是这个钟：只看后面跟着时刻的那一截。
    let tagged = |screen: &str| {
        screen.match_indices("\u{f017} ").any(|(at, mark)| {
            screen[at + mark.len()..]
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_digit())
        })
    };
    assert!(!tagged(&session.screen()), "输入框上面的标记没了");
}
