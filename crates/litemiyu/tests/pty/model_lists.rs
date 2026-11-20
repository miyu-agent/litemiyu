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
