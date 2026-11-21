//! 引擎已有、界面接上的命令：`!` 跑命令（不经模型、不判权限），`/stop` 打断这一轮。

use crate::replies::{calls, says};
use crate::support::{Session, served};

#[test]
fn a_bang_runs_the_command_and_shows_its_output_under_it() {
    // 跑完了照一条消息进历史，模型接着回一句。
    let server = served(vec![says(&["看到了。"])]);
    let session = Session::start(&server.base_url, &[]);
    session.wait_for("❯", "输入框");
    session.type_text("!");
    session.wait_for("输入要运行的命令", "跑命令的模式");
    session.type_text("echo purple-cat");
    session.press_enter();
    let screen = session.wait_for("退出码 0", "跑完了那一行");
    assert!(
        screen.contains("! echo purple-cat"),
        "命令行写在正文里：\n{screen}"
    );
    assert!(
        screen.matches("purple-cat").count() >= 2,
        "命令行一处、输出一处：\n{screen}"
    );
    session.wait_for("看到了。", "模型看了结果接着说");
    let first = String::from_utf8_lossy(&server.received()[0]).into_owned();
    assert!(
        first.contains("purple-cat"),
        "命令的输出交给了模型：{first}"
    );
}

#[test]
fn stop_interrupts_the_running_turn() {
    // 模型调一条一直不退出的命令（光是 `sleep` 的引擎不让跑）；跑着的时候 `/stop`。
    let server = served(vec![
        calls(
            "shell",
            r#"{"command":"tail -f /dev/null","description":"Wait","timeout":600000}"#,
        ),
        says(&["不该走到这里。"]),
    ]);
    let session = Session::start(&server.base_url, &[]);
    session.wait_for("❯", "输入框");
    session.type_text("等一分钟");
    session.press_enter();
    session.wait_for("tail -f /dev/null", "命令跑起来了");
    session.type_text("/stop");
    session.press_enter();
    let screen = session.wait_for(crate::support::DONE, "这一轮停下的收尾行");
    assert!(
        !screen.contains("不该走到这里。"),
        "停了就不再请求模型：\n{screen}"
    );
    assert_eq!(server.received().len(), 1, "只请求过一次模型");
}

#[test]
fn compact_summarises_the_session_and_says_so() {
    // 每一份都回同一句：对话、摘要的请求谁先谁后都对得上。
    let server = served(vec![says(&["摘要：说过一句话。"]); 4]);
    let session = Session::start(&server.base_url, &[]);
    session.wait_for("❯", "输入框");
    // 说的话要够长，压缩才有能替代的（短的全在保留的尾巴里，`nothing_to_compact`）：照引擎的
    // `manual_compaction_log`，七万个字，粘进来收成一块。
    session.type_text("记住紫色");
    session.paste(&"x".repeat(70_000));
    session.press_enter();
    session.wait_for(crate::support::DONE, "第一轮结束");
    // 带上要求（LM-3 删了命令行的 `litemiyu compact`，它那条测试查的「摘要请求带着要求」挪到这里查）。
    session.type_text("/compact 只留颜色");
    session.press_enter();
    session.wait_for("上下文已压缩", "压完那一行");
    let asked = server.received().iter().any(|body| {
        let body = String::from_utf8_lossy(body);
        body.contains("记住紫色") && body.contains("只留颜色")
    });
    assert!(asked, "摘要请求里带着要求和说过的话");
}

#[test]
fn stop_before_any_session_says_nothing_is_running() {
    // miyu-tui `app/session.rs` 的 `nothing_yet`：还没开会话打 `/stop`，当场说「没有运行中的任务」，不去开一个会话。
    let server = served(vec![says(&["不该走到这里。"])]);
    let session = Session::start(&server.base_url, &[]);
    session.wait_for("❯", "输入框");
    session.type_text("/stop");
    session.press_enter();
    session.wait_for("没有运行中的任务", "还没开会话没有要停的");
    assert!(server.received().is_empty(), "没有请求模型");
}
