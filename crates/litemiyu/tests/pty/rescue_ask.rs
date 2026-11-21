//! 应急模式下的提问（LM-25）：全屏界面里模型调 `ask_user`，抽屉弹出来，按数字作答，答的交回给模型；题目和回答只记在内存里，
//! 整棵树一个字节没变、程序没开着能写的文件。没人看着的一次运行（`litemiyu ask --rescue`）工具面上没有 `ask_user`。

use crate::cli::{Home, Input, serve, stream};
use crate::replies::{calls, says};
use crate::rescue::{Root, given, launch, writable_files};
use crate::support::{DONE, served};

/// 请求里的工具名，排好。
fn tools(body: &[u8]) -> Vec<String> {
    let json: serde_json::Value = serde_json::from_slice(body).expect("请求是 JSON");
    let mut names: Vec<String> = json["tools"]
        .as_array()
        .expect("有工具")
        .iter()
        .filter_map(|tool| tool["function"]["name"].as_str().map(str::to_string))
        .collect();
    names.sort_unstable();
    names
}

#[test]
fn a_question_in_rescue_mode_opens_the_drawer_reaches_the_model_and_writes_nothing() {
    let root = Root::new("ask-user");
    let asked = r#"{"questions":[{"question":"先清哪个目录？","header":"目录","options":[{"label":"缓存"},{"label":"日志"}]}]}"#;
    let server = served(vec![calls("ask_user", asked), says(&["好，先清日志。"])]);
    let env = given(&server, vec![("HOME", root.at("nohome").into())]);
    let before = root.bytes();
    let session = launch(&root, env);
    session.wait_for("应急模式", "输入框上边框的标记");
    session.type_text("磁盘满了");
    session.press_enter();
    let screen = session.wait_for("先清哪个目录？", "提问的抽屉");
    assert!(screen.contains("2. 日志"), "选项列在抽屉里：\n{screen}");
    assert_eq!(server.received().len(), 1, "没答之前不往下走：\n{screen}");
    let pid = session.pid().expect("有进程号");
    assert_eq!(writable_files(pid), Vec::<String>::new(), "开着能写的文件");
    assert_eq!(root.bytes(), before, "等回答时一个字节没变");
    session.type_text("2");
    session.wait_for("好，先清日志。", "答了以后模型接着说");
    session.wait_for(DONE, "回合结束的收尾行");
    let received = server.received();
    assert!(
        tools(&received[0]).contains(&"ask_user".to_string()),
        "全屏的应急模式工具面上有 ask_user"
    );
    let second = String::from_utf8_lossy(&received[1]);
    assert!(
        second.contains("先清哪个目录？") && second.contains("= 日志"),
        "题目和答的交回给模型：{second}"
    );
    assert!(
        !second.contains("\\\"缓存\\\" =") && !second.contains("= 缓存"),
        "没选的不算答了：{second}"
    );
    assert_eq!(writable_files(pid), Vec::<String>::new(), "开着能写的文件");
    assert_eq!(root.bytes(), before, "答完以后一个字节没变");
    session.press(b"\x04");
    assert!(session.wait_exit(), "Ctrl+D 好好退出");
    assert_eq!(root.bytes(), before, "退出以后也没变");
}

#[test]
fn ask_rescue_has_no_ask_user_on_its_tool_face() {
    let server = serve(vec![stream(says(&["好。"]))]);
    let home = Home::new(None);
    let mut command = home.command(&["ask", "--rescue", "在吗"]);
    command
        .env("LITEMIYU_RESCUE_BASE_URL", &server.base_url)
        .env("LITEMIYU_RESCUE_MODEL", "m-rescue");
    let out = crate::cli::run(&mut command, Input::Null);
    assert_eq!(out.code, Some(0), "{out:?}");
    assert_eq!(out.stdout, "好。\n");
    let talks = server.talks();
    assert_eq!(
        tools(talks[0].as_bytes()),
        ["edit", "jobs", "read", "shell", "write"],
        "没人看着的应急模式只有那五件"
    );
}
