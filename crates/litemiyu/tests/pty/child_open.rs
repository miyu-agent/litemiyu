//! `-s <子会话编号>` 直接打开子代理的会话（LM-25）：派出子代理、等它报完、退出；主会话的窗口还开着时不让开，照实说是哪个会话
//! 开着；主会话关了以后照编号打开，正文是子会话自己的（交代的活、它答的），接着说话时模型的请求里带着应有的上文（普通子代理
//! 是交代，fork 子代理是主会话分叉以前的对话）；`litemiyu ask -s <子会话编号>` 一样接得上。列会话照旧只有主会话。
//!
//! 假服务器照请求现回（几个会话的请求谁先到不一定）：接着说的那一句答什么，照请求里还有没有该有的上文定，测试看屏幕上的字就
//! 知道上文在不在。打的字只用 ASCII：请求体里的字照原样找。

use crate::cli::Input;
use crate::replies::{calls, says};
use crate::support::{DONE, Session, served_by};

/// 普通子会话的 system 末尾接的那一段（`resources/core/jobs/subagent-venue.txt`）的开头。
const CHILD: &str = "You are a subagent, started by another session";

/// fork 的外壳（`resources/core/jobs/fork-directive.txt`）里的一句。
const FORK: &str = "You are a fork of the agent above.";

/// 重开以后在子会话里说的那一句。
const MORE: &str = "go on in the child";

fn answer(body: &str) -> String {
    let more = body.contains(MORE);
    if body.contains(FORK) {
        return match (more, body.contains("BLUEWHALE")) {
            (false, _) => says(&["fork: ", "做完了第一步。"]),
            (true, true) => says(&["fork: ", "暗号还在，接着做完了。"]),
            (true, false) => says(&["fork: 暗号丢了。"]),
        };
    }
    if body.contains(CHILD) {
        return match (more, body.contains("TASK-CHILD")) {
            (false, _) => says(&["child: ", "做完了第一步。"]),
            (true, true) => says(&["child: ", "交代还在，接着做完了。"]),
            (true, false) => says(&["child: 交代丢了。"]),
        };
    }
    if body.contains("<subagent-report") {
        says(&["看到回报了。"])
    } else if body.contains(r#""role":"tool""#) {
        says(&["派出去了。"])
    } else if body.contains("spawn a fork") {
        calls(
            "subagent",
            r#"{"description":"Fork it","prompt":"TASK-FORK: do the first step.","fork":true}"#,
        )
    } else if body.contains("spawn a plain one") {
        calls(
            "subagent",
            r#"{"description":"Do it","prompt":"TASK-CHILD: do the first step."}"#,
        )
    } else {
        says(&["记下了。"])
    }
}

/// 数据目录 `data` 里的主会话和它派的那一个子会话的编号。
fn ids(data: &std::path::Path) -> (String, String) {
    let paths = lm_engine::store::paths::Paths::under(data);
    let listed = lm_engine::session::list_sessions(&paths).expect("列得出");
    assert_eq!(listed.len(), 1, "列会话只列主会话：{listed:?}");
    let main = listed[0].id.as_str().to_string();
    let mut others: Vec<String> = std::fs::read_dir(paths.sessions())
        .expect("读得了会话目录")
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name != &main)
        .collect();
    assert_eq!(others.len(), 1, "只派了一个子代理：{others:?}");
    (main, others.remove(0))
}

/// 派一个子代理（`fork` 的先说一句带暗号的话）、退出、照编号重开它、接着说一句；`first`、`kept` 是它第一步答的、接着说时
/// 上文还在才答的。
fn reopens(fork: bool, first: &str, kept: &str) {
    let base_url = served_by(answer);
    let session = Session::start(&base_url, &[]);
    session.wait_for("❯", "输入框");
    let mut turns = 2;
    if fork {
        session.type_text("the code word is BLUEWHALE");
        session.wait_for("the code word is BLUEWHALE", "打的字进了输入框");
        session.press_enter();
        session.wait_for("记下了。", "主会话答了第一句");
        turns = 3;
    }
    let order = match fork {
        true => "spawn a fork",
        false => "spawn a plain one",
    };
    session.type_text(order);
    session.wait_for(order, "打的字进了输入框");
    session.press_enter();
    session.wait_for("看到回报了。", "子代理报完了，主会话看到了回报");
    session.wait_until("主会话的几轮都收了尾", |s| {
        s.screen().matches(DONE).count() >= turns
    });
    let (main, child) = ids(session.data());

    // 主会话的窗口还开着：子会话跟着它在那个进程里，不让别的窗口开，照实说是哪个会话开着。
    let refused = session.another(&["-s", &child]);
    let said = refused.wait_for("已在其他窗口打开", "说主会话开着");
    assert!(
        said.contains("是子代理的会话") && said.contains(&main),
        "写明是子代理的会话、哪个会话开着：\n{said}"
    );
    assert!(!refused.wait_exit(), "退出码不是 0");

    session.press(b"\x04");
    assert!(session.wait_exit(), "Ctrl+D 好好退出");

    // 主会话关了：照编号打开子会话，正文是它自己的。
    let again = session.another(&["-s", &child]);
    let screen = again.wait_for(first, "子会话第一步答的");
    assert!(
        screen.contains("收到消息"),
        "交代的活照发来的话画：\n{screen}"
    );
    assert!(
        !screen.contains("派出去了。") && !screen.contains("看到回报了。"),
        "没有主会话的正文：\n{screen}"
    );
    again.type_text(MORE);
    again.wait_for(MORE, "打的字进了输入框");
    again.press_enter();
    again.wait_for(kept, "接着说：请求里带着应有的上文");
    again.wait_until("两轮都收了尾", |s| {
        s.screen().matches(DONE).count() >= 2
    });
    again.press(b"\x04");
    assert!(again.wait_exit(), "Ctrl+D 好好退出");

    // `ask -s <子会话编号>` 一样接得上；`-c` 只认主会话。
    let run = |args: &[&str]| {
        let home = session.data().parent().expect("数据目录在临时目录里");
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_litemiyu"));
        command
            .args(args)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("LANG", "zh_CN.UTF-8")
            .env("LITEMIYU_HOME", session.data())
            .env("HOME", home.join("home"))
            .env("LITEMIYU_TEST_KEY", "sk-test")
            .env("LITEMIYU_CATALOG_UPDATE", "false")
            .current_dir(session.work());
        crate::cli::run(&mut command, Input::Null)
    };
    let out = run(&["ask", "-s", &child, MORE]);
    assert_eq!(out.code, Some(0), "{out:?}");
    assert_eq!(out.stdout.trim_end(), kept, "{out:?}");
    let out = run(&["ask", "-c", "--format", "json", "hello again"]);
    assert_eq!(out.code, Some(0), "{out:?}");
    assert_eq!(
        crate::cli::json(out.stdout.trim())["session"],
        main.as_str(),
        "-c 打开的是主会话，不是刚说过话的子会话"
    );
    assert_eq!(ids(session.data()), (main, child), "列会话照旧只有主会话");
}

#[test]
fn a_subagents_session_reopens_by_its_id_and_goes_on_with_its_task() {
    reopens(
        false,
        "child: 做完了第一步。",
        "child: 交代还在，接着做完了。",
    );
}

#[test]
fn a_fork_subagents_session_reopens_by_its_id_and_still_sees_the_conversation() {
    reopens(true, "fork: 做完了第一步。", "fork: 暗号还在，接着做完了。");
}
