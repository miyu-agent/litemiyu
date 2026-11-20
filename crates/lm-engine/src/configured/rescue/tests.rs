//! 应急模式读配置：读不到照全空办、说为什么；给的供应商盖在配置上面；写得不对的说是哪一样；key 不外露。

use lm_kernel::config::Value;
use lm_kernel::config::secret::{Reference, Secret};

use super::{Given, PROVIDER, Refused, load};
use crate::store::paths::Paths;
use crate::store::test_support::Scratch;

/// 假 key。
const KEY: &str = "sk-FAKE-rescue-not-real";

fn given(base_url: &str, api: &str, key: bool) -> Given {
    Given {
        base_url: base_url.to_string(),
        model: "m-1".to_string(),
        api: api.to_string(),
        key: key.then(|| Secret::new(KEY).expect("合写法")),
    }
}

fn no_env(_: &str) -> Option<String> {
    None
}

fn text(value: Option<(&Value, &lm_kernel::config::merge::Origin)>) -> Option<String> {
    match value {
        Some((Value::Text(text), _)) => Some(text.to_string()),
        Some((other, _)) => Some(format!("{other:?}")),
        None => None,
    }
}

#[test]
fn without_any_place_the_given_provider_is_the_whole_config() {
    let given = given("http://127.0.0.1:9/v1", "anthropic", true);
    let rescued = load(None, Some(&given), no_env).expect("给的写得对");
    assert_eq!(rescued.unreadable, None, "没有地方可读，不算读不了");
    let resolved = &rescued.loaded.resolved;
    assert_eq!(
        text(resolved.get("models.chat")).as_deref(),
        Some("rescue/m-1")
    );
    assert_eq!(
        text(resolved.get("providers.rescue.driver")).as_deref(),
        Some("anthropic")
    );
    let source = rescued.loaded.configs.borrow().clone();
    let key = source.secret(&Reference::Secret(PROVIDER.into()));
    assert_eq!(key.as_ref().map(Secret::expose), Some(KEY));
}

#[test]
fn the_given_provider_sits_on_top_of_a_readable_config() {
    let scratch = Scratch::new();
    let paths = Paths::under(scratch.path());
    std::fs::create_dir_all(&paths.config).expect("建得了");
    let config = "[providers.deepseek]\ndriver = \"openai-chat\"\nkey = { secret = \"deepseek\" }\n\n\
                  [providers.rescue]\ncatalog = \"openai\"\n\n[models]\nchat = \"deepseek/deepseek-chat\"\n";
    std::fs::write(paths.config_file(), config).expect("写得进");
    std::fs::write(
        paths.secrets_file(),
        "deepseek = \"sk-FAKE-file-not-real\"\n",
    )
    .expect("写得进");
    let given = given("http://127.0.0.1:9/v1", "openai-chat", false);
    let rescued = load(Some(&paths), Some(&given), no_env).expect("给的写得对");
    assert_eq!(rescued.unreadable, None);
    let resolved = &rescued.loaded.resolved;
    assert_eq!(
        text(resolved.get("models.chat")).as_deref(),
        Some("rescue/m-1"),
        "给的盖过配置里的"
    );
    assert!(
        resolved.get("providers.deepseek.driver").is_some(),
        "配置里别的照旧在"
    );
    assert_eq!(
        resolved.get("providers.rescue.catalog"),
        None,
        "配置里同编号那一家的别的项不混进来"
    );
    let source = rescued.loaded.configs.borrow().clone();
    assert!(
        source
            .secret(&Reference::Secret("deepseek".into()))
            .is_some()
    );
    assert!(
        source.secret(&Reference::Secret(PROVIDER.into())).is_none(),
        "没给 key"
    );
    assert_eq!(
        resolved.get("providers.rescue.key"),
        None,
        "没给 key 就不引用"
    );
}

#[test]
fn a_broken_or_unreadable_config_is_said_and_left_out() {
    let scratch = Scratch::new();
    let paths = Paths::under(scratch.path());
    std::fs::create_dir_all(&paths.config).expect("建得了");
    std::fs::write(paths.config_file(), "[providers.x\nbroken").expect("写得进");
    let given = given("http://127.0.0.1:9/v1", "openai-chat", true);
    let rescued = load(Some(&paths), Some(&given), no_env).expect("给的写得对");
    let why = rescued.unreadable.expect("说了读不了");
    assert!(why.starts_with("config.toml: not valid TOML"), "{why}");
    assert_eq!(
        text(rescued.loaded.resolved.get("models.chat")).as_deref(),
        Some("rescue/m-1")
    );

    std::fs::write(paths.config_file(), "").expect("写得进");
    std::fs::write(paths.secrets_file(), "= broken").expect("写得进");
    let rescued = load(Some(&paths), None, no_env).expect("没给不会错");
    let why = rescued.unreadable.expect("密钥写坏了也说");
    assert!(why.starts_with("secrets.toml"), "{why}");
    assert_eq!(rescued.loaded.resolved.get("models.chat"), None);
}

#[test]
fn a_wrong_address_or_kind_names_the_item_and_never_the_key() {
    let wrong = load(None, Some(&given("not a url", "openai-chat", true)), no_env);
    assert_eq!(wrong.err(), Some(Refused("base_url".to_string())));
    let wrong = load(
        None,
        Some(&given("http://127.0.0.1:9", "gpt", true)),
        no_env,
    );
    assert_eq!(wrong.err(), Some(Refused("driver".to_string())));
    let shown = format!("{:?}", given("http://127.0.0.1:9", "gpt", true));
    assert!(!shown.contains(KEY), "{shown}");
    assert!(shown.contains("key: true"), "{shown}");
}
