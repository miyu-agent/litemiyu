//! 一轮进行中来的回报不切断时间线（Miyu 发版前 check 21，2026-11-09 项目主人从三个样板里挑了「放进时间线里」；照 Miyu
//! `tests/report_in_timeline.rs`）：后台命令在一轮中间完成，收起以后只有一行，带「1 个后台命令完成」；点开能看到那一步。

use crate::replies::{calls, says};
use crate::support::{Session, served_by};

/// 收起那一行认的字：中文、英文的收起行都认（收起行的语言照配置挑，同 Miyu）。
const COUNTED: [&str; 2] = ["1 个后台命令完成", "1 background command finished"];

fn counted(line: &str) -> bool {
    COUNTED.iter().any(|w| line.contains(w))
}

/// 照请求现回：先派一条后台命令（0.3 秒后自己结束），再在前台占两秒（后台那条在这两秒里结束、回报在一轮中间到），再跑一条
/// 收尾，最后说完。回报要是另开了一轮，那一轮照「收到回报。」答。
fn script(body: &str) -> String {
    let tools = body.matches(r#""role":"tool""#).count();
    let command = |command: &str, description: &str, background: bool| {
        let mut args = serde_json::json!({"command": command, "description": description});
        if background {
            args["run_in_background"] = serde_json::json!(true);
        }
        calls("shell", &args.to_string())
    };
    match tools {
        0 => command("sleep 0.3; echo bg-done", "小睡一下", true),
        1 => command("/bin/sleep 2; echo waited", "等两秒", false),
        2 => command("echo after-report", "收尾", false),
        _ if body.contains("都做完了。") => says(&["收到回报。"]),
        _ => says(&["都做完了。"]),
    }
}

#[test]
fn a_report_inside_a_turn_is_a_step_of_the_timeline() {
    let base_url = served_by(script);
    let session = Session::start(&base_url, &[]);
    session.wait_for("❯", "输入框");
    session.type_text("做三件事");
    session.press_enter();
    session.wait_for("都做完了。", "她说完了");
    session.wait_until("带回报数的收起行", |s| {
        s.screen().lines().any(counted)
    });
    let screen = session.screen();
    let lines: Vec<&str> = screen.lines().collect();
    let reply = lines
        .iter()
        .position(|l| l.contains("都做完了。"))
        .expect("回答在");
    let row = lines.iter().position(|l| counted(l)).expect("收起行在");
    // 回答以前只有一行收起行：回报没有单独一行，也没把时间线切成两段。
    let before = lines[..reply]
        .iter()
        .filter(|l| l.contains("后台命令完成") || counted(l))
        .count();
    assert_eq!(before, 1, "只有一行收起行：\n{screen}");
    assert!(row < reply, "收起行在回答上面：\n{screen}");
    // 点开那一段：回报是里面的一步，在后面那条命令上面。
    session.click(12, u16::try_from(row).expect("屏幕不到六万行"));
    session.wait_for("● 后台命令完成", "点开看到回报那一步");
    let screen = session.screen();
    let at = |text: &str| screen.lines().position(|l| l.contains(text));
    let (Some(report), Some(after)) = (at("● 后台命令完成"), at("echo after-report"))
    else {
        panic!("展开以后看得到回报和后面那一步：\n{screen}");
    };
    assert!(report < after, "回报画在它发生的位置：\n{screen}");
}
