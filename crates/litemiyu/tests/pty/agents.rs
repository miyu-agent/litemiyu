//! 后台命令和子代理（Miyu 蓝图 `tui.md`「后台命令、子代理和侧边栏」，L5 界面 二、三）：底栏的后台按钮、后台面板（展开看
//! 输出、停掉一个），正文里结束的那一行；子代理要人确认的高危命令进抽屉，第一行写哪个子代理在问，答了它接着走完。
//!
//! 后台命令只跑无害、自己会结束的（`sh -c 'sleep 30'`；不写 GNU 的 `timeout`，macOS 上没有，LM-7）：测试停不掉它也在 30 秒里
//! 退出，不留进程。

use crate::replies::{calls, says};
use crate::support::{Session, served_by};

/// 子会话的 system 末尾接的那一段（`resources/core/jobs/subagent-venue.txt`）的开头。
const CHILD: &str = "You are a subagent, started by another session";

/// 照请求现回：两个会话的请求谁先到不一定。子会话先要强推（高危，问人），跑完说一句；主会话派子代理，派出去说一句，
/// 看到回报说一句。
fn risky(body: &str) -> String {
    let tool = body.contains(r#""role":"tool""#);
    if body.contains(CHILD) {
        return if tool {
            says(&["child: done"])
        } else {
            calls(
                "shell",
                r#"{"command":"git push --force","description":"Push"}"#,
            )
        };
    }
    if body.contains("<subagent-report") {
        says(&["看到回报了。"])
    } else if tool {
        says(&["派出去了。"])
    } else {
        calls(
            "subagent",
            r#"{"description":"Push it","prompt":"Force-push the branch."}"#,
        )
    }
}

#[test]
fn a_subagents_confirmation_opens_the_drawer_and_the_answer_reaches_it() {
    let base_url = served_by(risky);
    let session = Session::start(&base_url, &[]);
    session.wait_for("❯", "输入框");
    session.type_text("派个子代理推一下");
    session.press_enter();
    // 三样一起等：一帧的字节分两截读到时，屏幕上先有了抽屉第一行，命令那几行和状态行还没到（连跑时红过一遍，
    // 拿到的屏幕上抽屉只画到「高危命令」）。
    session.wait_until(
        "抽屉第一行写哪个子代理在问、下面是要跑的命令，子代理状态行里它在跑",
        |s| {
            let screen = s.screen();
            screen.contains("子代理 Push it 在问")
                && screen.contains("git push --force")
                && screen.contains("○ Push it")
        },
    );
    // 允许本次（第一项）：工作目录是临时目录、不是仓库，git 当场报错退出，推不到哪里去。
    session.press_enter();
    session.wait_for("后台任务完成 · Push it", "子代理接着走完、报回来了");
    session.wait_for("看到回报了。", "主会话被回报叫醒");
    session.wait_until("它报完了，子代理状态行收起", |s| {
        !s.screen().contains("○ Push it")
    });
    // 结束那一行点开看它交回来的报告全文（「后台命令、子代理和侧边栏」第 5 条，界面线第六趟）。
    assert!(
        !session.screen().contains("child: done"),
        "收着看不到报告：\n{}",
        session.screen()
    );
    let row = session
        .row_of(0, "后台任务完成 · Push it")
        .expect("那一行在屏幕上");
    session.click(10, row);
    session.wait_for("child: done", "点开看到报告全文");
    session.shot("report");
}

/// 照请求现回：她派后台命令以后、人停了它叫醒她的（后台命令结束的回报 `<command-ended`）各回各的。
fn background(body: &str) -> String {
    if body.contains("<command-ended") {
        return says(&["停好了。"]);
    }
    if body.contains(r#""role":"tool""#) {
        return says(&["开着了。"]);
    }
    let args = serde_json::json!({
        "command": "echo hello-out; sh -c 'sleep 30'",
        "description": "Wait",
        "run_in_background": true
    });
    calls("shell", &args.to_string())
}

#[test]
fn the_jobs_button_opens_the_panel_which_shows_output_and_stops_a_command() {
    let base_url = served_by(background);
    let session = Session::start(&base_url, &[]);
    session.wait_for("❯", "输入框");
    session.type_text("跑一个后台命令");
    session.press_enter();
    session.wait_for("开着了。", "派出去以后她说的");
    session.wait_for("1 个后台命令", "底栏的后台按钮");
    // 输入框空着按 ↓ 焦点停到后台按钮上，Enter 开后台面板（同 Miyu `tests/job_output.rs`），一条一行写命令和在跑的用时。
    session.press(b"\x1b[B");
    session.press_enter();
    session.wait_for("（运行中", "面板里在跑的那一条");
    session.press_enter();
    session.wait_for("hello-out", "展开看输出");
    session.type_text("x");
    session.wait_for("后台命令已停止 · echo hello-out", "停掉以后正文里那一行");
    session.wait_for("（已停止）", "面板里跟着变");
    session.press(b"\x1b");
    // 等 Esc 办完（面板关了）再点：Esc 和后面点击的头一个 ESC 一起读到时，终端那头的解析把两个 ESC 当成一下 Esc，点击剩下的
    // `[<0;…M` 当字打进了输入框（全仓并发跑时红过，输入框里留着 `[<0;11;12M`）。
    session.wait_until("Esc 关了面板", |s| !s.screen().contains("（已停止）"));
    session.wait_for("停好了。", "回报叫醒她");
    let screen = session.screen();
    assert!(
        !screen.contains("1 个后台命令"),
        "没有在跑的，按钮不在：\n{screen}"
    );
    // 结束那一行点开看命令和它的输出（Miyu `announce` 的 `detail`、`fill_job`，「后台命令、子代理和侧边栏」第 5 条）。
    let row = session
        .row_of(0, "后台命令已停止 · echo hello-out")
        .expect("那一行在屏幕上");
    session.click(10, row);
    session.wait_until("点开看到命令和输出", |s| {
        let screen = s.screen();
        screen.contains("    echo hello-out; sh -c 'sleep 30'") && screen.contains("    hello-out")
    });
}
