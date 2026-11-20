//! 循环（设计第五节第 9 条，L5 界面 二）：`/loop 5m <话>` 开一个，那句话当场交进去、她回了；回执弹提示，输入框上面那一行
//! 写下一次几点，侧边栏多一段「循环」；`/loop off` 停掉，标记跟着没了；请求里带着 `schedule_wakeup`。

use crate::replies::says;
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
    session.wait_for("每 5 分钟 · 下次", "侧边栏那一段");
    let screen = session.screen();
    assert!(
        screen.contains("看看 CI"),
        "交进去的那句照你说的话画：{screen}"
    );
    // 回执的提示框收了以后（它盖着输入框上面那一行）。
    session.wait_for("\u{f017} ", "输入框上面那一行的记号");
    session.type_text("/loop off");
    session.press_enter();
    session.wait_for("已停止此会话的循环。", "停了的回执");
    // 侧边栏那一段先没（回执的提示框还盖着输入框上面那一行），等提示框收了，那一行的记号也没了。
    session.wait_until("侧边栏那一段没了", |s| {
        !s.screen().contains("每 5 分钟")
    });
    session.wait_until("回执收了", |s| {
        !s.screen().contains("已停止此会话的循环。")
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
