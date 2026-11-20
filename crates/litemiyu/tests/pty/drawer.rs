//! 确认、提问的抽屉整条走通：模型调 `ask_user`、跑一条高危命令，抽屉弹出来，按数字作答，答的交回会话、模型下一次请求里看得到。

use crate::replies::{calls, says};
use crate::support::{Session, served};

#[test]
fn a_question_opens_the_drawer_and_the_answer_reaches_the_model() {
    let asked = r#"{"questions":[{"question":"用哪种颜色？","header":"颜色","options":[{"label":"紫色"},{"label":"绿色"}]}]}"#;
    let server = served(vec![calls("ask_user", asked), says(&["好，用紫色。"])]);
    let session = Session::start(&server.base_url, &[]);
    session.wait_for("❯", "输入框");
    session.type_text("挑个颜色");
    session.press_enter();
    let screen = session.wait_for("用哪种颜色？", "提问的抽屉");
    assert!(screen.contains("1. 紫色"), "选项列在抽屉里：\n{screen}");
    session.type_text("1");
    session.wait_for("好，用紫色。", "答了以后模型接着说");
    let received = server.received();
    let second = String::from_utf8_lossy(&received[1]);
    assert!(second.contains("紫色"), "答的交回给模型：{second}");
}

#[test]
fn a_hazardous_command_waits_for_approval_and_runs_once_allowed() {
    let run = r#"{"command":"git push --force","description":"Push"}"#;
    let server = served(vec![
        calls("shell", run),
        says(&["推不上去：这里不是仓库。"]),
    ]);
    let session = Session::start(&server.base_url, &[]);
    session.wait_for("❯", "输入框");
    session.type_text("推一下");
    session.press_enter();
    let screen = session.wait_for("git push --force", "确认的抽屉写着要跑的命令");
    assert_eq!(server.received().len(), 1, "没答之前不往下走：\n{screen}");
    session.type_text("1");
    session.wait_for("推不上去：这里不是仓库。", "允许以后跑了、模型接着说");
    let received = server.received();
    let second = String::from_utf8_lossy(&received[1]);
    assert!(
        second.contains("Exit code 128"),
        "命令真的跑了（不是仓库，git 退出码 128），结果交回给模型：{second}"
    );
}
