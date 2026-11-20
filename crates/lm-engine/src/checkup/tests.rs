//! 被忽略的几样里配置项那一样：引擎整份读配置时顺手交回（LM-3补，原来是起来时另读一遍的 `ignored`）。MCP 服务、技能两样
//! 在会话造好、第一次开回合时记下，测试在 `tests/session/checkup.rs`。以前登记过、LM-2 拆掉的 `net.search.*` 不算被忽略。

use crate::configured::load;
use crate::store::paths::Paths;
use crate::store::test_support::Scratch;

fn none(_: &str) -> Option<String> {
    None
}

#[test]
fn nothing_written_is_nothing_ignored() {
    let scratch = Scratch::new();
    let loaded = load(&Paths::under(scratch.path()), none).expect("读得了");
    assert!(loaded.ignored.is_empty());
}

#[test]
fn the_settings_left_out_are_named_when_the_config_is_read() {
    let scratch = Scratch::new();
    let paths = Paths::under(scratch.path());
    std::fs::create_dir_all(&paths.config).expect("建得了配置那一处");
    std::fs::write(
        paths.config_file(),
        "[models]\nchat = \"a/b\"\nnot_a_key = 1\n\n\
         [ui]\nlanguage = 3\n\n\
         [net.search]\nsource = \"tavily\"\n\n\
         [net.search.tavily]\nkey = { secret = \"net-search-tavily-key\" }\n\n\
         [mcp.Bad]\ncommand = \"x\"\n",
    )
    .expect("写得进");
    std::fs::write(
        paths.secrets_file(),
        "ok = \"sk-1\"\n\"bad name\" = \"sk-2\"\n",
    )
    .expect("写得进");
    let loaded = load(&paths, none).expect("读得了");
    assert_eq!(
        loaded.ignored,
        ["models.not_a_key", "ui.language", "bad name"],
        "config.toml 两项（照文件里的先后）、secrets.toml 一行；退役的搜索键、MCP 服务不算在配置项里"
    );
}

#[test]
fn a_config_that_is_not_toml_is_refused_whole() {
    let scratch = Scratch::new();
    let paths = Paths::under(scratch.path());
    std::fs::create_dir_all(&paths.config).expect("建得了配置那一处");
    std::fs::write(paths.config_file(), "[models\n").expect("写得进");
    assert!(
        load(&paths, none).is_err(),
        "整份读不了的照原话报，不算被忽略"
    );
}

/// LM-8 砍了模型池：以前写的池（自己建的、固定的三个）、冷却、供应商的缓存类别照退役的键读，不算被忽略，最终值里没有；
/// `models.chat` 指着池的写法不对了，算被忽略的一项、最终值里没有（引擎照「没有模型」那条路说：`no model configured`）。
#[test]
fn old_pool_settings_are_retired_and_a_chat_on_a_pool_is_left_out() {
    let scratch = Scratch::new();
    let paths = Paths::under(scratch.path());
    std::fs::create_dir_all(&paths.config).expect("建得了配置那一处");
    std::fs::write(
        paths.config_file(),
        "[providers.relay]\ndriver = \"openai-chat\"\nbase_url = \"http://127.0.0.1:9/v1\"\ncache = \"per_request\"\n\n\
         [providers.relay.models.m]\nwindow = 64000\n\n\
         [models]\nchat = \"@daily\"\nvision = \"relay/m\"\n\n\
         [models.cooldown.auth]\nbase = \"1m\"\n\n\
         [models.cooldown.quota]\nbase = \"1h\"\nmax = { nope = 1 }\n\n\
         [pools.daily]\nmodels = [\"relay/m\", \"gone/x\"]\nstrategy = \"rotate\"\nsubagent = true\ndescription = \"Everyday\"\n\n\
         [pools.flagship]\nsubagent = false\n",
    )
    .expect("写得进");
    let loaded = load(&paths, none).expect("读得了");
    assert_eq!(
        loaded.ignored,
        ["models.chat"],
        "退役的池、冷却、缓存类别不算；指着池的 models.chat 写法不对"
    );
    let values = loaded.resolved.values();
    assert!(values.get("models.chat").is_none(), "最终值里没有默认模型");
    assert!(values.get("models.vision").is_some(), "别的照旧");
    assert!(
        !values.keys().any(|key| key.starts_with("pools.")
            || key.starts_with("models.cooldown")
            || key.ends_with(".cache")),
        "退役的不进最终值"
    );
    assert_eq!(
        lm_kernel::models::provider::chat(&values),
        None,
        "路由照这一刻的 models.chat：没有，说 no model configured"
    );
}
