//! `/model`、`/effort`（Miyu 终端 `app/models.rs`、`app/effort.rs`）：开窗列配置里的模型，能搜、标着现在用的，选了当场换这个
//! 会话的模型、也记成新会话的默认；下一次请求就用新的；关了窗口再打开这个会话还是新的。

use crate::replies::says;
use lm_engine::http::testkit::Received;

use crate::support::{Session, served};

/// 配了两个模型的一家（`m1` 是默认的聊天模型），供应商指向 `base_url`。按键模式写明方向键：这几条打字就搜；Vim 模式先按
/// `/` 的在 `vim_lists.rs`（出厂改成 Vim 以后写明的）。
fn config(base_url: &str) -> String {
    format!(
        "[providers.fake]\ndriver = \"openai-chat\"\nbase_url = \"{base_url}\"\nkey = {{ env = \"LITEMIYU_TEST_KEY\" }}\n\n\
         [providers.fake.models.\"m1\"]\nwindow = 128000\n\n[providers.fake.models.\"m2\"]\nwindow = 64000\n\n[models]\nchat = \"fake/m1\"\n\n\
         [tui]\nkeys = \"arrows\"\n"
    )
}

/// 说 `text` 的那一次请求发给哪个模型（不带工具的旁路请求不算）。
fn model_of(requests: &[Received], text: &str) -> String {
    let body = requests
        .iter()
        .rev()
        .map(|r| String::from_utf8_lossy(&r.body).into_owned())
        .find(|body| body.contains(text) && body.contains("\"tools\""))
        .unwrap_or_else(|| panic!("没有说「{text}」的那次请求"));
    let json: serde_json::Value = serde_json::from_str(&body).expect("请求体是 JSON");
    json["model"].as_str().unwrap_or_default().to_string()
}

#[test]
fn picking_a_model_switches_this_session_and_the_next_request_uses_it() {
    let server = served(vec![
        says(&["第一句的回答"]),
        says(&["第二句的回答"]),
        says(&["第三句的回答"]),
    ]);
    let session = Session::launch(Some(&config(&server.base_url)), &[], &[]);
    session.wait_for("❯", "输入框");
    session.type_text("头一句话");
    session.press_enter();
    session.wait_for("第一句的回答", "第一轮答完");
    assert_eq!(model_of(&server.requests(), "头一句话"), "m1");
    // 开窗：两个模型都在，现在用的那个行首写 ●。
    session.type_text("/model");
    session.press_enter();
    let screen = session.wait_for("─ 模型", "窗开了");
    session.wait_until("列出两个模型", |s| s.screen().contains("m2"));
    let screen = session.screen().to_string() + &screen;
    assert!(screen.contains("● m1"), "现在用的标着：\n{screen}");
    // 打字筛：只剩 m2，回车换过去。
    session.type_text("m2");
    session.wait_until("筛成一个", |s| s.screen().contains("1 个"));
    session.press_enter();
    session.wait_until("窗关了", |s| !s.screen().contains("─ 模型"));
    session.wait_until("底栏写成 m2", |s| {
        s.screen().lines().rev().take(3).any(|l| l.contains("m2"))
    });
    session.type_text("第二句话");
    session.press_enter();
    session.wait_for("第二句的回答", "第二轮答完");
    assert_eq!(
        model_of(&server.requests(), "第二句话"),
        "m2",
        "下一次请求用新的"
    );
    let written = std::fs::read_to_string(
        lm_engine::store::paths::Paths::under(session.data()).config_file(),
    )
    .expect("对得上");
    assert!(
        written.contains("chat = \"fake/m2\""),
        "也记成新会话的默认：\n{written}"
    );
    // 关了窗口，同一个数据目录再打开这个会话：还是 m2。
    session.type_text("/exit");
    session.press_enter();
    assert!(session.wait_exit(), "退出码 0");
    let again = session.another(&[]);
    again.wait_for("❯", "输入框");
    again.type_text("/sessions");
    again.press_enter();
    again.wait_for("未打开", "列表交回来了");
    again.press_enter();
    again.wait_for("第二句的回答", "补发了以前的");
    again.type_text("第三句话");
    again.press_enter();
    again.wait_for("第三句的回答", "第三轮答完");
    assert_eq!(
        model_of(&server.requests(), "第三句话"),
        "m2",
        "重开还是新的"
    );
}

#[test]
fn a_model_without_a_key_is_listed_dim_and_cannot_be_picked() {
    let server = served(vec![says(&["好的"])]);
    let config = format!(
        "{}\n[providers.nokey]\ndriver = \"openai-chat\"\nbase_url = \"{}\"\nkey = {{ env = \"LITEMIYU_NO_SUCH_KEY\" }}\n\n\
         [providers.nokey.models.\"x1\"]\nwindow = 32000\n",
        config(&server.base_url),
        server.base_url
    );
    let session = Session::launch(Some(&config), &[], &[]);
    session.wait_for("❯", "输入框");
    session.type_text("/model");
    session.press_enter();
    session.wait_for("未设置密钥", "没有 key 的那一行右边写着");
    session.type_text("x1");
    session.wait_until("筛成一个", |s| s.screen().contains("1 个"));
    session.press_enter();
    session.wait_for("该模型未设置密钥", "不换，提示为什么");
    assert!(session.screen().contains("─ 模型"), "窗还开着");
    session.press(b"\x1b");
    session.wait_until("Esc 关窗", |s| !s.screen().contains("─ 模型"));
}

#[test]
fn effort_switches_this_session_and_the_next_request_carries_it() {
    let server = served(vec![says(&["好的"])]);
    let config = config(&server.base_url).replace(
        "[providers.fake.models.\"m1\"]\nwindow = 128000\n",
        "[providers.fake.models.\"m1\"]\nwindow = 128000\nreasoning = [\"low\", \"medium\", \"high\"]\n",
    );
    let session = Session::launch(Some(&config), &[], &[]);
    session.wait_for("❯", "输入框");
    session.type_text("/effort");
    session.press_enter();
    session.wait_for("─ 思考程度 · m1", "窗开了，标题写模型");
    let screen = session.wait_for("high", "列出几档");
    assert!(
        screen.contains("● 跟随设置"),
        "第一行跟随设置，现在跟着：\n{screen}"
    );
    for _ in 0..3 {
        session.press(b"\x1b[B");
    }
    session.press_enter();
    session.wait_until("窗关了", |s| !s.screen().contains("─ 思考程度"));
    session.type_text("想一想");
    session.press_enter();
    session.wait_for("好的", "答完");
    let body = server
        .received()
        .into_iter()
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .find(|b| b.contains("想一想"))
        .expect("说了的那次请求");
    assert!(
        body.contains("\"reasoning_effort\":\"high\""),
        "带上选的那一档：{body}"
    );
}

#[test]
fn effort_lists_the_levels_of_the_model_in_use_and_follows_settings_first() {
    let server = served(vec![says(&["好的"])]);
    // m1 照目录不知道有哪几档：窗里写这个模型没有可选的思考程度。
    let session = Session::launch(Some(&config(&server.base_url)), &[], &[]);
    session.wait_for("❯", "输入框");
    session.type_text("/effort");
    session.press_enter();
    session.wait_for("─ 思考程度 · m1", "窗开了，标题写模型");
    session.wait_for("该模型没有可选的思考程度", "没有档位的照实说");
    session.press(b"\x1b");
    session.wait_until("Esc 关窗", |s| !s.screen().contains("─ 思考程度"));
}
