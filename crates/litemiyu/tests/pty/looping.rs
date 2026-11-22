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

/// LM-44（2026-11-22 项目主人：「我 /loop 会提示参数错误而不是直接发一个 /loop 出去」）：光打 `/loop` 开一个由 AI 自己
/// 决定的循环。回执照 Miyu 的字，正文里照你敲的画 `/loop`，交给模型的是出厂的那一句；她这一轮没定下一次，循环结束，
/// 输入框上面那一行不留标记。
#[test]
fn a_bare_loop_lets_the_ai_decide_and_ends_when_she_schedules_nothing() {
    let server = served(vec![says(&["先看了一眼，没有要做的。"])]);
    let session = Session::start(&server.base_url, &[]);
    session.wait_for("❯", "输入框");
    session.type_text("/loop");
    session.press_enter();
    session.wait_for(
        "已开始循环：由 AI 决定执行的内容和下一次运行的时间",
        "光打的回执",
    );
    session.wait_for("先看了一眼，没有要做的。", "交进去的那一句她回了");
    let screen = session.screen();
    assert!(
        !screen.contains("命令的参数不正确") && !screen.contains("No new instructions"),
        "不是参数错误，正文里也不露那句内置的英文：{screen}"
    );
    session.wait_until("回执收了", |s| !s.screen().contains("已开始循环"));
    assert!(
        !session.screen().contains('\u{f017}'),
        "她没定下一次：循环结束，标记不留着"
    );
    let first = String::from_utf8_lossy(&server.received()[0]).into_owned();
    assert!(
        first.contains("No new instructions. Continue on your own judgment"),
        "交给模型的是出厂的那一句：{first}"
    );
}

/// LM-44（2026-11-22 项目主人：「开启 loop 后，我没法 /stop 清理掉」）：`/stop` 一并停掉循环，输入框上面那一行的标记跟着
/// 没了；再 `/loop off` 答的是「没有循环」。
#[test]
fn stop_also_stops_the_loop() {
    let server = served(vec![says(&["CI 还在跑。"])]);
    let session = Session::start(&server.base_url, &[]);
    session.wait_for("❯", "输入框");
    session.type_text("/loop 5m 看看 CI");
    session.press_enter();
    session.wait_for("CI 还在跑。", "交进去的那一句她回了");
    session.wait_for("\u{f017} ", "输入框上面那一行的记号");
    session.type_text("/stop");
    session.press_enter();
    session.wait_until("标记没了", |s| !s.screen().contains('\u{f017}'));
    session.type_text("/loop off");
    session.press_enter();
    session.wait_for("此会话没有循环。", "循环已经被 /stop 停掉了");
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
    session.wait_for("命令的参数不正确", "只写了间隔：参数不对，不露原因码");
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

/// LM-34：说一句、`/undo`，再敲 `/loop 做一下`：人敲 `/loop` 算人的一次操作，循环的第一句马上交进去、她回了；回执照常是
/// 「已开始循环」；撤掉的那一轮从此恢复不了，`/restore` 说没有。正文里那一句照人敲的原文画（「敲什么显示什么」），和人平常
/// 说的话一个样子；请求里是那句话，不是人敲的那一行。重开以后照样是原文。
#[test]
fn a_loop_typed_after_an_undo_runs_at_once_and_reads_as_typed() {
    let server = served(vec![
        says(&["第一轮的回答。"]),
        says(&["循环这一轮的回答。"]),
        says(&["后来这一句的回答。"]),
    ]);
    let session = Session::start(&server.base_url, &[]);
    session.wait_for("❯", "输入框");
    session.type_text("说一句");
    session.wait_for("❯ 说一句", "打进去了");
    session.press_enter();
    session.wait_for("第一轮的回答。", "这一轮做完");
    session.type_text("/undo");
    session.wait_for("❯ /undo", "打进去了");
    session.press_enter();
    session.wait_for("↶ 已撤回", "撤了");
    session.wait_until("那一句放回输入框", |s| {
        s.screen().lines().any(|l| l.contains("❯ 说一句"))
    });
    // 放回来的那句全选着：打字换掉它。
    session.type_text("/loop 做一下");
    session.wait_for("❯ /loop 做一下", "打进去了");
    session.press_enter();
    session.wait_for("已开始循环：由 AI 定下一次 · 做一下", "回执照常");
    session.wait_for("循环这一轮的回答。", "第一句马上交进去、她回了");
    let screen = session.screen();
    assert!(
        screen.contains("/loop 做一下"),
        "正文里照人敲的原文画：{screen}"
    );
    assert!(!screen.contains("循环 · "), "没有替代的说法：{screen}");
    // 回执的提示框收了以后再敲（它盖着输入框上面那一行）。
    session.wait_until("回执收了", |s| !s.screen().contains("已开始循环"));
    session.type_text("/restore");
    session.wait_for("❯ /restore", "打进去了");
    session.press_enter();
    session.wait_for("没有可恢复的撤回", "撤掉的那一轮恢复不了");
    session.type_text("后来这一句");
    session.wait_for("❯ 后来这一句", "打进去了");
    session.press_enter();
    session.wait_for("后来这一句的回答。", "人说的下一句照常");
    // 和人平常说的话一个样子：两行开头几格的底色、字色一样，打头那两格（记号）的字也一样。
    let looped = session.row_of(0, "/loop 做一下").expect("那一句在屏幕上");
    let said = session
        .row_of(0, "后来这一句")
        .expect("人说的那一句在屏幕上");
    let look = |row: u16| {
        (0..4)
            .map(|col| {
                let cell = session.cell(row, col);
                let mark = cell
                    .as_ref()
                    .filter(|_| col < 2)
                    .map(|(text, _)| text.clone());
                (mark, cell.map(|(_, bg)| bg), session.fg(row, col))
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(look(looped), look(said), "样式和人说的话一样");
    let requests = server.received();
    let looped = String::from_utf8_lossy(&requests[1]).into_owned();
    assert!(looped.contains("做一下"), "请求里有那句话：{looped}");
    assert!(
        !looped.contains("/loop 做一下"),
        "人敲的那一行不进请求：{looped}"
    );
    // 重开：照样是原文。
    session.type_text("/exit");
    session.wait_for("❯ /exit", "打进去了");
    session.press_enter();
    assert!(session.wait_exit(), "/exit 退出");
    let again = session.another(&["-c"]);
    again.wait_for("后来这一句的回答。", "重开了那个会话");
    let screen = again.screen();
    assert!(
        screen.contains("/loop 做一下"),
        "重开以后照样是原文：{screen}"
    );
}
