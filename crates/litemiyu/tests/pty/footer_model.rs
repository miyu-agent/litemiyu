//! 底栏的模型那一格（Miyu 终端 `transcript/model.rs::show_default`、`ui/footer.rs::left`，对照表 A49③）：还没开会话时写默认的
//! 聊天模型（`models.chat`，照启动时已经读进来的 `config.toml`，引擎线程不为它起来）；说了话照推来的写；`/model` 换了当场变；
//! `/new` 回到默认的；窄了先收模型编号前面几段、最后才收供应商名；没配默认模型的空着（同 Miyu）。

use crate::replies::says;
use crate::support::{Options, Session, served};

/// 一家两个模型，默认的聊天模型是 `chat`。按键模式写明方向键（`/model` 窗里打字就搜）。
fn config(base_url: &str, chat: &str) -> String {
    format!(
        "[providers.fake]\ndriver = \"openai-chat\"\nbase_url = \"{base_url}\"\nkey = {{ env = \"LITEMIYU_TEST_KEY\" }}\n\n\
         [providers.fake.models.\"m1\"]\nwindow = 128000\n\n[providers.fake.models.\"m2\"]\nwindow = 64000\n\n\
         [providers.fake.models.\"clinepass/cline-pass/deepseek-v4.1-flash\"]\nwindow = 64000\n\n\
         [models]\nchat = \"{chat}\"\n\n[tui]\nkeys = \"arrows\"\n"
    )
}

/// 底栏那一行：输入框下边框（`╰`）下面那一行（首页它下面还有一行工作区）。
fn footer(session: &Session) -> String {
    let screen = session.screen();
    let lines: Vec<&str> = screen.lines().collect();
    lines
        .iter()
        .rposition(|l| l.contains('╰'))
        .and_then(|at| lines.get(at + 1))
        .map(|l| (*l).to_string())
        .unwrap_or_default()
}

#[test]
fn the_home_page_footer_shows_the_default_model_and_it_follows_turns_model_and_new() {
    let server = served(vec![says(&["第一句的回答"]), says(&["第二句的回答"])]);
    let session = Session::launch(Some(&config(&server.base_url, "fake/m1")), &[], &[]);
    session.wait_for("❯", "输入框");
    // 首页、还没开会话：底栏写默认的聊天模型，模型编号接暗色的供应商名。
    session.wait_until("首页底栏写默认模型", |s| {
        footer(s).trim_start().starts_with("m1 fake")
    });
    assert!(
        server.requests().is_empty(),
        "只是起来，一个请求都没发：\n{}",
        footer(&session)
    );
    // 说一句：照推来的写，还是 m1。
    session.type_text("头一句话");
    session.press_enter();
    session.wait_for("第一句的回答", "答完");
    session.wait_until("说过话还在", |s| {
        let line = footer(s);
        line.trim_start().starts_with("m1 fake") && line.contains("Σ")
    });
    // `/model` 换成 m2：当场变。
    session.type_text("/model");
    session.press_enter();
    session.wait_until("列出两个模型", |s| s.screen().contains("m2"));
    session.type_text("m2");
    session.wait_until("筛成一个", |s| s.screen().contains("1 个"));
    session.press_enter();
    session.wait_until("底栏换成 m2", |s| {
        footer(s).trim_start().starts_with("m2 fake")
    });
    // `/new`：回到首页，写默认的——`/model` 选的也记成了新会话的默认，是 m2。
    session.type_text("/new");
    session.press_enter();
    session.wait_until("新会话的底栏照默认的写", |s| {
        let line = footer(s);
        line.trim_start().starts_with("m2 fake") && !line.contains("Σ")
    });
}

#[test]
fn a_narrow_window_drops_the_front_of_the_model_id_before_the_provider() {
    let server = served(vec![says(&["第一句的回答"])]);
    let chat = "fake/clinepass/cline-pass/deepseek-v4.1-flash";
    let wide = Session::launch(Some(&config(&server.base_url, chat)), &[], &[]);
    wide.wait_for("❯", "输入框");
    wide.wait_until("宽的写全", |s| {
        footer(s)
            .trim_start()
            .starts_with("clinepass/cline-pass/deepseek-v4.1-flash fake")
    });
    // 40 列：全的放不下，先只写编号最后一段、供应商名照写（Miyu `split` 的 `Model::Short`）。
    let options = Options {
        cols: Some(40),
        ..Options::default()
    };
    let narrow = Session::launch_with(Some(&config(&server.base_url, chat)), &[], &[], &options);
    narrow.wait_for("❯", "输入框");
    narrow.wait_until("窄的先收前面几段", |s| {
        let line = footer(s);
        line.trim_start().starts_with("deepseek-v4.1-flash fake") && !line.contains("clinepass")
    });
    // 22 列（首页的输入框跟着窄，底栏只剩十几列）：连供应商名也收掉（`Model::Bare`），还放不下的截掉写 `…`（Miyu `left_line`）。
    let options = Options {
        cols: Some(22),
        ..Options::default()
    };
    let narrower = Session::launch_with(Some(&config(&server.base_url, chat)), &[], &[], &options);
    narrower.wait_for("❯", "输入框");
    narrower.wait_until("最窄的收掉供应商名", |s| {
        let line = footer(s);
        line.trim() == "deepseek-v4.1-…"
    });
}

#[test]
fn no_default_model_leaves_the_cell_empty_like_miyu() {
    let server = served(vec![says(&["第一句的回答"])]);
    let config = format!(
        "[providers.fake]\ndriver = \"openai-chat\"\nbase_url = \"{}\"\nkey = {{ env = \"LITEMIYU_TEST_KEY\" }}\n\n\
         [providers.fake.models.\"m1\"]\nwindow = 128000\n",
        server.base_url
    );
    let session = Session::launch(Some(&config), &[], &[]);
    session.wait_for("❯", "输入框");
    let line = footer(&session);
    assert!(!line.contains("m1"), "没配默认的不猜：{line}");
}

/// LM-8 砍了模型池：旧配置里 `models.chat` 指着池（`[pools.*]` 还写着）的，首页底栏不写它（不写一个用不了的名字）；说一句
/// 照「没有可用的模型」说、一个请求都不发，那条「已忽略」的提示点名 `models.chat`（退役的池不算在里面）。
#[test]
fn a_chat_on_an_old_pool_is_not_shown_and_says_there_is_no_model() {
    let server = served(vec![says(&["不该有这句"])]);
    let text = format!(
        "{}\n[pools.duo]\nmodels = [\"fake/m1\", \"fake/m2\"]\nstrategy = \"rotate\"\n",
        config(&server.base_url, "@duo")
    );
    let session = Session::launch(Some(&text), &[], &[]);
    session.wait_for("❯", "输入框");
    std::thread::sleep(std::time::Duration::from_millis(500));
    assert!(
        !footer(&session).contains("@duo"),
        "首页底栏不写池：\n{}",
        session.screen()
    );
    session.type_text("头一句话");
    session.press_enter();
    session.wait_for("没有可用的模型", "照没有模型说");
    session.wait_for("配置项 models.chat", "提示点名写错的那一项");
    let screen = session.screen();
    assert!(!screen.contains("pools"), "退役的池不算被忽略：\n{screen}");
    assert!(server.requests().is_empty(), "一个请求都没发");
}
