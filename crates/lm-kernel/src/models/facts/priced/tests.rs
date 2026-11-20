//! 价格和倍率（Miyu 施工 8-7、8-15 的 `facts/tests.rs` 价格那几条、`price/tests.rs` 出处那几条，照 [`tariff`] 改写）：价格整份
//! 不拼，第 3、4 层照挑的那家借不借，本机的当免费，倍率谁盖谁，出处照来源写。

use serde_json::json;

use super::*;
use crate::config::merge::Resolved;
use crate::models::provider::provider;
use crate::models::test_support::{Held, resolved};

/// 档案只认 OpenAI 兼容的包，带上裁出来的目录。
fn held() -> Held {
    Held::new(
        json!({"npm": {"@ai-sdk/openai-compatible": "openai-chat"}}),
        true,
    )
}

/// 手写的写成配置文件。
fn file(_layer: Layer) -> String {
    "config.toml".to_string()
}

/// 照配置 `source` 算 `id` 这一家的 `model` 的价格。
fn tariff_of(held: &Held, source: &str, id: &str, model: &str) -> Option<Tariff> {
    let resolved: Resolved = resolved(source);
    let knowledge = held.knowledge();
    let provider = provider(&resolved.values(), &knowledge, id).expect("配了");
    tariff(&resolved, &knowledge, &provider, model, &file)
}

const DEEPSEEK: &str = "[providers.deepseek]\nlocal = false\n";

/// 第 1、2 层对上的目录条目：价格照借，出处写目录里的那一条（Miyu `facts/tests.rs` 第一条的 `price`、`price/tests.rs` 的
/// `the_source_names_the_catalog_entry`）。
#[test]
fn the_catalog_price_is_borrowed_and_named() {
    let tariff = tariff_of(&held(), DEEPSEEK, "deepseek", "deepseek-flash").expect("有价");
    assert_eq!(
        tariff.price.json(),
        json!({"input": 0.15, "output": 0.6, "cache_read": 0.003, "reasoning": 0.6, "currency": "USD"})
    );
    assert_eq!(tariff.source, "catalog:deepseek/deepseek-flash");
    assert_eq!(tariff.multiplier, 1.0, "都没写倍率是 1");
}

/// 价格是一整格：手写了一项就整份用手写的，币种不写是美元，写了照写的。
#[test]
fn a_written_price_is_taken_whole() {
    let source = format!(
        "{DEEPSEEK}\n[providers.deepseek.models.\"deepseek-flash\".price]\ninput = 1\ncurrency = \"CNY\"\n"
    );
    let tariff = tariff_of(&held(), &source, "deepseek", "deepseek-flash").expect("有价");
    assert_eq!(
        tariff.price.json(),
        json!({"input": 1.0, "currency": "CNY"}),
        "输出价不从目录拼"
    );
    assert_eq!(tariff.source, "config:config.toml:5", "第一项写在哪一行");
    let source = format!(
        "{DEEPSEEK}\n[providers.deepseek.models.\"deepseek-flash\"]\nprice = {{ output = 2.5 }}\n"
    );
    let tariff = tariff_of(&held(), &source, "deepseek", "deepseek-flash");
    assert_eq!(
        tariff.map(|tariff| tariff.price.json()),
        Some(json!({"output": 2.5, "currency": "USD"}))
    );
}

/// 第 3、4 层：价格照挑的那家借不借。
#[test]
fn name_matches_borrow_all_but_an_unofficial_price() {
    let relay = "[providers.newapi]\ndriver = \"openai-chat\"\nbase_url = \"https://relay.example.invalid/v1\"\n";
    assert_eq!(
        tariff_of(&held(), relay, "newapi", "deepseek-v4.1-flash"),
        None,
        "照字节序挑的，不借价"
    );
    let tariff = tariff_of(&held(), relay, "newapi", "claude-sonnet-4-5").expect("原厂的价");
    assert_eq!(tariff.price.rates.input, Some(3.0));
}

/// 本机的服务：价格只认手写的，没写是 0，出处 `local`；倍率照样算。
#[test]
fn a_local_service_is_free_unless_priced_by_hand() {
    let local = "[providers.ollama]\ndriver = \"openai-chat\"\nbase_url = \"http://127.0.0.1:11434/v1\"\n\n[providers.ollama.models.\"deepseek-v4.1-flash\"]\nprice_multiplier = 3\n";
    let tariff = tariff_of(&held(), local, "ollama", "deepseek-v4.1-flash").expect("有价");
    assert_eq!(tariff.price, Price::free());
    assert_eq!(tariff.source, "local");
    let cost = tariff
        .cost(&crate::kernel::event::Usage {
            uncached: 5000,
            cache_read: 100,
            cache_write: 7,
            output: 900,
            reasoning: None,
        })
        .expect("算得出");
    assert_eq!(
        serde_json::to_string(&cost).expect("写得出"),
        r#"{"amount":0,"currency":"USD","price":{"input":0,"output":0,"cache_read":0,"cache_write":0},"multiplier":3,"source":"local"}"#
    );
    let priced =
        format!("{local}\n[providers.ollama.models.\"deepseek-v4.1-flash\".price]\ninput = 0.5\n");
    let tariff = tariff_of(&held(), &priced, "ollama", "deepseek-v4.1-flash").expect("有价");
    assert_eq!(tariff.price.rates.input, Some(0.5));
}

/// 倍率：模型上写的，没写是 1（Miyu 另认供应商这一层的，LiteMiyu 没登记）。
#[test]
fn the_model_multiplier_applies_to_that_model_only() {
    let source = "[providers.deepseek]\nlocal = false\n\n[providers.deepseek.models.\"deepseek-v4-pro\"]\nprice_multiplier = 2\n";
    let flash = tariff_of(&held(), source, "deepseek", "deepseek-flash").expect("有价");
    assert_eq!(flash.multiplier, 1.0);
    let pro = tariff_of(&held(), source, "deepseek", "deepseek-v4-pro").expect("有价");
    assert_eq!(pro.multiplier, 2.0);
}

/// 没有价格的没有（Miyu `price/tests.rs` 的 `no_price_no_tariff`）：目录里没有这个模型、也没手写的。
#[test]
fn no_price_no_tariff() {
    assert_eq!(
        tariff_of(&held(), DEEPSEEK, "deepseek", "no-such-model"),
        None
    );
}
