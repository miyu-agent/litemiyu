//! 引用的写法（`models.md`「守着它的」第一行，施工 8-6）：切在第一个 `/`。施工 8-8：造会话记下的是查过的模型；一个引用这一轮
//! 指到哪。施工 8-8 补：以前的挡位名哪里都不认。LM-8：砍了模型池，以前的 `@池` 也不认。

use serde_json::json;

use super::*;
use crate::models::provider::Driver;
use crate::models::test_support::{Held, resolved};

fn model(provider: &str, model: &str) -> Reference {
    Reference {
        provider: provider.to_string(),
        model: model.to_string(),
    }
}

#[test]
fn a_model_is_cut_at_the_first_slash() {
    assert_eq!(
        Reference::parse("deepseek/deepseek-flash"),
        Ok(model("deepseek", "deepseek-flash"))
    );
    assert_eq!(
        Reference::parse("openrouter/deepseek/deepseek-v4"),
        Ok(model("openrouter", "deepseek/deepseek-v4"))
    );
    assert_eq!(
        Reference::parse("newapi/DeepSeek V4 Flash"),
        Ok(model("newapi", "DeepSeek V4 Flash"))
    );
    assert_eq!(Reference::parse("dev/v4.1"), Ok(model("dev", "v4.1")));
    for bad in [
        "", "deepseek", "/m", "dev/", "Dev/m", "dev m/x", "@", "@Free", "dev/a\nb",
    ] {
        assert_eq!(
            Reference::parse(bad),
            Err(Bad::NotAReference(bad.to_string())),
            "{bad:?}"
        );
    }
    assert_eq!(model("openrouter", "a/b").to_string(), "openrouter/a/b");
}

/// 以前的写法（LM-8 砍了模型池、8-8 补去掉了挡位）：照写法不对。
#[test]
fn old_pools_and_tiers_are_not_references() {
    for old in [
        "@free",
        "@lite",
        "@flagship",
        "lite",
        "cheap",
        "standard",
        "Lite",
    ] {
        assert_eq!(
            Reference::parse(old),
            Err(Bad::NotAReference(old.to_string())),
            "{old}"
        );
    }
}

#[test]
fn the_error_says_what_it_is_not() {
    assert_eq!(
        Bad::NotAReference("x".to_string()).to_string(),
        r#""x" is not a model"#
    );
}

/// 配置里引用的写法（`crate::config::Kind::Reference`）和这里认的一样：配置收下的，这里也认得（LM-8 起 `@池` 两边都不收）。
#[test]
fn the_config_accepts_what_is_read_here() {
    for kind in [crate::config::Kind::Reference, crate::config::Kind::Model] {
        for text in [
            "dev/m",
            "a/b/c",
            "dev/DeepSeek V4",
            "@free",
            "lite",
            "m",
            "",
            "Dev/m",
            "@",
            "dev/",
        ] {
            let value = crate::config::Value::Text(text.to_string().into());
            assert_eq!(
                kind.accepts(&value),
                Reference::parse(text).is_ok(),
                "{kind:?} {text:?}"
            );
        }
    }
}

/// 两家，`models.chat` 是 `a/main`、`models.vision` 是 `b/big`。
const CONFIG: &str = "[providers.a]\ndriver = \"openai-chat\"\nbase_url = \"https://a.invalid\"\n\n[providers.b]\ndriver = \"openai-chat\"\nbase_url = \"https://b.invalid\"\n\n[models]\nchat = \"a/main\"\nvision = \"b/big\"\n";

fn values(source: &str) -> Values {
    resolved(source).values()
}

/// 用途里点名的模型（`model.list` 照它列）：照 `chat`、`vision` 的先后。
#[test]
fn named_models_come_from_the_uses() {
    let named = named(&values(CONFIG));
    let named: Vec<String> = named
        .into_iter()
        .map(|(provider, model)| format!("{provider}/{model}"))
        .collect();
    assert_eq!(named, ["a/main", "b/big"]);
}

#[test]
fn a_session_records_the_model_a_reference_resolves_to_now() {
    let values = values(CONFIG);
    let record = |text: &str| record(&values, text);
    assert_eq!(
        record("b/anything").as_deref(),
        Ok("b/anything"),
        "模型名不查"
    );
    for (text, why) in [
        ("c/m", r#"no provider "c""#),
        ("@free", r#""@free" is not a model"#),
        ("nope", r#""nope" is not a model"#),
        ("lite", r#""lite" is not a model"#),
        ("flagship", r#""flagship" is not a model"#),
    ] {
        assert_eq!(record(text), Err(NoModel(why.to_string())), "{text}");
    }
}

#[test]
fn a_reference_resolves_to_a_model_this_turn() {
    let values = values(CONFIG);
    let held = Held::new(json!({}), false);
    let knowledge = held.knowledge();
    let target = resolve(&values, &knowledge, "b/big").expect("解析得出");
    assert_eq!(
        (target.provider.id.as_str(), target.model.as_str()),
        ("b", "big")
    );
    assert_eq!(target.provider.driver, Driver::OpenAiChat);
    assert_eq!(
        resolve(&values, &knowledge, "cheap").map(|_| ()),
        Err(NoModel(r#""cheap" is not a model"#.to_string())),
        "以前的挡位名不认"
    );
    assert_eq!(
        resolve(&values, &knowledge, "@free").map(|_| ()),
        Err(NoModel(r#""@free" is not a model"#.to_string())),
        "以前的池不认"
    );
    assert_eq!(
        resolve(&values, &knowledge, "c/m").map(|_| ()),
        Err(NoModel(r#"no provider "c""#.to_string()))
    );
    assert_eq!(
        crate::models::provider::NOT_CONFIGURED,
        "no model configured: set models.chat"
    );
}
