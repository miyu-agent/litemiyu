//! 对话里开 `/model` 时在后台拉供应商的模型列表（配置 五，照 Miyu：开选模型的框时 `model.list` 列整份的，每家最多一分钟一次）：框先照
//! 手头的列，列到了框里直接多出来、存下；一分钟里再开不再拉。只列模型（GET），不发说话的请求。

use crate::support::{Session, served_listing};

fn config(base_url: &str) -> String {
    format!(
        "[providers.fake]\ndriver = \"openai-chat\"\nbase_url = \"{base_url}\"\nkey = {{ env = \"LITEMIYU_TEST_KEY\" }}\n\n\
         [providers.fake.models.\"m1\"]\n\n[models]\nchat = \"fake/m1\"\n\n[tui]\nkeys = \"arrows\"\n"
    )
}

#[test]
fn opening_the_model_box_fetches_the_list_in_the_background_once() {
    let server = served_listing(r#"{"data":[{"id":"m1"},{"id":"only-listed"}]}"#, vec![]);
    let session = Session::launch(Some(&config(&server.base_url)), &[], &[]);
    session.wait_for("❯", "输入框");
    session.type_text("/model");
    session.press_enter();
    session.wait_for("─ 模型", "窗开了");
    session.wait_until("列到的模型出现在框里", |s| {
        s.screen().contains("only-listed")
    });
    let listed = |server: &crate::support::Served| {
        server
            .requests()
            .iter()
            .filter(|r| r.method == "GET" && r.path.ends_with("/models"))
            .count()
    };
    assert_eq!(listed(&server), 1);
    assert!(
        server.requests().iter().all(|r| r.method == "GET"),
        "只列模型，不发说话的请求"
    );
    let stored = lm_engine::store::paths::Paths::under(session.data())
        .model_cache()
        .join("providers/fake.json");
    session.wait_until("存下了", |_| stored.exists());
    // 关了再开：一分钟里不再拉，框里照存下的列。
    session.press(b"\x1b");
    session.wait_until("窗关了", |s| !s.screen().contains("─ 模型"));
    session.type_text("/model");
    session.press_enter();
    session.wait_for("only-listed", "照存下的列");
    assert_eq!(listed(&server), 1, "一分钟里不再拉");
}

/// LM-47（Miyu 核心 FX-18，2026-11-22 项目主人：「有这种配置但是模型已经不存在了的话，不应该再出现在列表里」）：供应商报得出
/// 自己的列表时以它为准——配置里只写了资料、列表里没有的模型不再列；正在用的（默认的聊天模型）列表里没有也照旧列，右边标上
/// 「供应商已经没有这个模型」。
#[test]
fn models_the_provider_no_longer_lists_are_dropped_unless_in_use() {
    let server = served_listing(r#"{"data":[{"id":"only-listed"}]}"#, vec![]);
    let config = format!(
        "[providers.fake]\ndriver = \"openai-chat\"\nbase_url = \"{}\"\nkey = {{ env = \"LITEMIYU_TEST_KEY\" }}\n\n\
         [providers.fake.models.\"m1\"]\n\n[providers.fake.models.\"written-only\"]\nwindow = 64000\n\n\
         [models]\nchat = \"fake/m1\"\n\n[tui]\nkeys = \"arrows\"\n",
        server.base_url
    );
    let session = Session::launch(Some(&config), &[], &[]);
    session.wait_for("❯", "输入框");
    session.type_text("/model");
    session.press_enter();
    session.wait_for("─ 模型", "窗开了");
    session.wait_until("列到了：照供应商的列表", |s| {
        let screen = s.screen();
        screen.contains("only-listed") && !screen.contains("written-only")
    });
    let screen = session.screen();
    assert!(
        screen.contains("m1") && screen.contains("供应商已经没有这个模型"),
        "正在用的照旧列、标出来：{screen}"
    );
}

/// 报不出列表的供应商（这里 `/models` 回的不是列表）照旧：配置里写的都列。
#[test]
fn a_provider_without_a_list_still_shows_what_the_config_names() {
    let server = served_listing("not a list", vec![]);
    let config = format!(
        "[providers.fake]\ndriver = \"openai-chat\"\nbase_url = \"{}\"\nkey = {{ env = \"LITEMIYU_TEST_KEY\" }}\n\n\
         [providers.fake.models.\"m1\"]\n\n[providers.fake.models.\"written-only\"]\nwindow = 64000\n\n\
         [models]\nchat = \"fake/m1\"\n\n[tui]\nkeys = \"arrows\"\n",
        server.base_url
    );
    let session = Session::launch(Some(&config), &[], &[]);
    session.wait_for("❯", "输入框");
    session.type_text("/model");
    session.press_enter();
    session.wait_for("─ 模型", "窗开了");
    session.wait_for("written-only", "配置里写的照旧列");
    session.wait_until("拉过了", |_| {
        server
            .requests()
            .iter()
            .any(|r| r.method == "GET" && r.path.ends_with("/models"))
    });
    let screen = session.screen();
    assert!(
        screen.contains("written-only") && !screen.contains("供应商已经没有这个模型"),
        "没有列表的照旧：{screen}"
    );
}
