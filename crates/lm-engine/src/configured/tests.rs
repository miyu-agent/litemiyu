//! 照配置文件取配置：读系统配置、密钥文件，合出最终值；密钥照 `{ secret }`、`{ env }` 取。

use lm_kernel::config::Value;
use lm_kernel::config::secret::Reference;

use super::{LoadError, load};
use crate::store::paths::Paths;
use crate::store::test_support::Scratch;

/// 临时目录下的四处，写好 `config` 和 `secrets` 两份（`None` 的不写）。
fn paths(scratch: &Scratch, config: Option<&str>, secrets: Option<&str>) -> Paths {
    let paths = Paths::under(scratch.path());
    std::fs::create_dir_all(&paths.config).expect("建得了配置那一处");
    if let Some(text) = config {
        std::fs::write(paths.config_file(), text).expect("写得进");
    }
    if let Some(text) = secrets {
        std::fs::write(paths.secrets_file(), text).expect("写得进");
    }
    paths
}

/// 一份环境：只有 `TEST_KEY`。
fn env(name: &str) -> Option<String> {
    (name == "TEST_KEY").then(|| "sk-from-env".to_string())
}

#[test]
fn no_files_is_an_empty_config() {
    let scratch = Scratch::new();
    let loaded = load(&paths(&scratch, None, None), env).expect("读得了");
    assert_eq!(loaded.resolved.get("models.chat"), None, "没配默认的模型");
    let source = loaded.configs.borrow().clone();
    assert!(source.secret(&Reference::Secret("x".into())).is_none());
}

#[test]
fn the_chat_model_and_both_kinds_of_key_are_read() {
    let scratch = Scratch::new();
    let config = "[providers.deepseek]\ndriver = \"openai-chat\"\nkey = { secret = \"deepseek\" }\n\n\
                  [models]\nchat = \"deepseek/deepseek-v4\"\n";
    let paths = paths(
        &scratch,
        Some(config),
        Some("deepseek = \"sk-from-file\"\n"),
    );
    let loaded = load(&paths, env).expect("读得了");
    let chat = loaded
        .resolved
        .get("models.chat")
        .map(|(value, _)| value.clone());
    assert_eq!(chat, Some(Value::Text("deepseek/deepseek-v4".into())));
    let source = loaded.configs.borrow().clone();
    let from_file = source.secret(&Reference::Secret("deepseek".into()));
    assert_eq!(
        from_file.map(|s| s.expose().to_string()).as_deref(),
        Some("sk-from-file")
    );
    let from_env = source.secret(&Reference::Env("TEST_KEY".into()));
    assert_eq!(
        from_env.map(|s| s.expose().to_string()).as_deref(),
        Some("sk-from-env")
    );
    assert!(
        source.secret(&Reference::Env("UNSET".into())).is_none(),
        "没设的是空的"
    );
    // 没有项目配置：在哪个目录都一样。
    assert_eq!(source.with_project("/elsewhere"), loaded.resolved);
}

#[test]
fn a_wrong_entry_is_ignored_and_the_rest_is_kept() {
    let scratch = Scratch::new();
    let config = "[models]\nchat = \"a/b\"\nnot_a_key = 1\n";
    let loaded = load(&paths(&scratch, Some(config), None), env).expect("照收");
    assert!(loaded.resolved.get("models.chat").is_some());
}

#[test]
fn broken_toml_is_refused_with_where() {
    let scratch = Scratch::new();
    let refused = load(&paths(&scratch, Some("[models\n"), None), env).expect_err("写法不对");
    assert!(
        matches!(refused, LoadError::Syntax("config.toml", Some((1, _)))),
        "{refused:?}"
    );
    let refused = load(&paths(&scratch, Some(""), Some("= x\n")), env).expect_err("写法不对");
    assert!(
        matches!(refused, LoadError::Syntax("secrets.toml", _)),
        "{refused:?}"
    );
}

/// 以前存过搜索来源、几家的 key、SearXNG 地址的配置（LM-2 砍了 `web_search`，`net.search.*` 这几项跟着没了）：照旧读得进，
/// 别的项照收；改别的项照旧存得进，那一张表原样留在文件里（人自己删）。
#[test]
fn an_old_config_with_search_settings_still_loads_and_saves() {
    let scratch = Scratch::new();
    let config = "[models]\nchat = \"a/b\"\n\n\
                  [net.search]\nsource = \"tavily\"\n\n\
                  [net.search.tavily]\nkey = { secret = \"net-search-tavily-key\" }\n\n\
                  [net.search.searxng]\nurl = \"http://127.0.0.1:8888\"\n";
    let paths = paths(
        &scratch,
        Some(config),
        Some("net-search-tavily-key = \"tvly-old\"\n"),
    );
    let loaded = load(&paths, env).expect("照收");
    let chat = loaded
        .resolved
        .get("models.chat")
        .map(|(value, _)| value.clone());
    assert_eq!(chat, Some(Value::Text("a/b".into())));

    let settings = crate::settings::Settings::new(paths.clone(), |_| None);
    settings
        .set(&[crate::settings::Edit::Set(
            "models.chat".to_string(),
            Value::Text("c/d".into()),
        )])
        .expect("改别的项照旧存得进");
    let saved = std::fs::read_to_string(paths.config_file()).expect("读得回");
    assert!(saved.contains("chat = \"c/d\""), "{saved}");
    assert!(
        saved.contains("[net.search.tavily]"),
        "旧的那张表原样留着：{saved}"
    );
    let again = load(&paths, env).expect("存过以后照旧读得进");
    assert_eq!(
        again
            .resolved
            .get("models.chat")
            .map(|(value, _)| value.clone()),
        Some(Value::Text("c/d".into()))
    );
}
