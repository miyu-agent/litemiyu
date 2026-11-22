//! 存下来的列表：写了读得回；只记名字，模型资料（窗口、最大输出、输入、工具）和没有列表时一模一样，所以发给模型的请求不变。

use std::collections::BTreeMap;

use lm_kernel::models::observed::ProviderList;

use super::{Claims, REFETCH, due, fetched_within, store, stored};
use crate::store::paths::Paths;
use crate::store::test_support::Scratch;

#[test]
fn a_stored_list_reads_back_by_provider() {
    let scratch = Scratch::new();
    let paths = Paths::under(scratch.path());
    assert!(stored(&paths).is_empty());
    let names = vec!["opencode-go/deepseek-v4-pro".to_string(), "m2".to_string()];
    store(&paths, "router", &names).unwrap();
    let got = stored(&paths);
    assert_eq!(got.get("router"), Some(&names));
    // 会话的路由照同一处读（`read_observed`）。
    let text = std::fs::read_to_string(paths.model_cache().join("providers/router.json")).unwrap();
    let list = ProviderList::parse(&text).unwrap();
    assert!(list.models.iter().all(|m| m.window.is_none()
        && m.max_output.is_none()
        && m.tools.is_none()
        && m.vision.is_none()));
    assert!(store(&paths, "../x", &names).is_err(), "编号不能出目录");
}

#[test]
fn a_names_only_list_leaves_the_model_facts_and_so_the_request_alone() {
    use lm_kernel::config::Layer;
    use lm_kernel::config::merge::{Layers, merge};
    use lm_kernel::config::parse::parse;
    use lm_kernel::models::Knowledge;
    use lm_kernel::models::facts::facts;
    use lm_kernel::models::observed::Learned;

    let scratch = Scratch::new();
    let paths = Paths::under(scratch.path());
    let items = crate::configured::items();
    let text = "[providers.deepseek]\nkey = { env = \"X\" }\n";
    let parsed = parse(&items, Layer::System, text).unwrap();
    let layers = Layers {
        system: Some(&parsed),
        ..Layers::default()
    };
    let resolved = merge(&items, &layers, &|_| None);
    let catalog = crate::settings::catalog::Catalog::load(&paths).unwrap();
    let models: Vec<String> = catalog
        .models("deepseek")
        .into_iter()
        .map(|m| m.id)
        .chain(["not-in-the-catalog".to_string()])
        .collect();
    store(&paths, "deepseek", &models).unwrap();
    let text =
        std::fs::read_to_string(paths.model_cache().join("providers/deepseek.json")).unwrap();
    let mut lists = BTreeMap::new();
    lists.insert("deepseek".to_string(), ProviderList::parse(&text).unwrap());
    let empty = BTreeMap::new();
    let learned = Learned::default();
    let (profiles, vendors) = crate::models::shipped().unwrap();
    let loaded = lm_kernel::models::catalog::Loaded {
        catalog: lm_kernel::models::catalog::Catalog::parse(include_str!(
            "../../../../lm-kernel/testdata/models/models-dev-trimmed.json"
        ))
        .unwrap()
        .catalog,
        source: lm_kernel::models::catalog::CatalogSource::Snapshot,
        fetched: "2026-11-01T00:00:00Z".to_string(),
    };
    let knowing = |lists| Knowledge {
        profiles: &profiles,
        vendors: &vendors,
        catalog: Some(&loaded),
        lookup: Some(lm_kernel::models::catalog::CatalogLookup::Full(&loaded)),
        learned: &learned,
        lists,
    };
    let values = resolved.values();
    let with = knowing(&lists);
    let without = knowing(&empty);
    let provider = lm_kernel::models::provider::provider(&values, &without, "deepseek").unwrap();
    for model in &models {
        let (a, _) = facts(&resolved, &with, &provider, model);
        let (b, _) = facts(&resolved, &without, &provider, model);
        assert_eq!(format!("{a:?}"), format!("{b:?}"), "{model} 的资料不变");
    }
}

/// 照 Miyu `claim_list`：同一家一分钟里只拉一次，别的家照拉，过了间隔照拉。
#[test]
fn a_provider_is_claimed_once_a_minute() {
    let claims = Claims::default();
    assert!(claims.claim("deepseek", REFETCH));
    assert!(!claims.claim("deepseek", REFETCH), "一分钟里不再拉");
    assert!(claims.claim("openai", REFETCH), "别的家照拉");
    assert!(
        claims.claim("deepseek", std::time::Duration::ZERO),
        "过了间隔照拉"
    );
    let shared = claims.clone();
    assert!(!shared.claim("openai", REFETCH), "拷一份共用同一张");
}

/// 刚存过列表的（别的窗口、上一次运行一分钟以内拉过）不拉，也不占这一回的「拉过」；没存过的拉一次，一分钟里不再拉。
#[test]
fn a_fresh_stored_list_is_not_fetched_again() {
    let scratch = Scratch::new();
    let paths = Paths::under(scratch.path());
    let claims = Claims::default();
    assert!(!fetched_within(&paths, "router", REFETCH), "没存过");
    assert!(due(&paths, &claims, "router"));
    assert!(!due(&paths, &claims, "router"), "这一回拉过了");
    store(&paths, "relay", &["m1".to_string()]).unwrap();
    assert!(fetched_within(&paths, "relay", REFETCH));
    assert!(!fetched_within(&paths, "relay", std::time::Duration::ZERO));
    assert!(!due(&paths, &claims, "relay"), "刚存过的不拉");
    assert!(claims.claim("relay", REFETCH), "没占这一回的「拉过」");
}

/// LM-50（Miyu 核心 FX-19，2026-11-22 项目主人：「上游的模型列表是什么样子显示就应该是什么样子」）：配置界面的一家，报得出
/// 列表的就照它列，配置里写了资料的、主对话指到的、目录里的，列表里没有都不列；报不出的照旧。
#[test]
fn the_settings_page_offers_exactly_what_the_provider_lists() {
    use lm_kernel::config::Layer;
    use lm_kernel::config::merge::{Layers, merge};
    use lm_kernel::config::parse::parse;

    let scratch = Scratch::new();
    let paths = Paths::under(scratch.path());
    let items = crate::configured::items();
    let text = "[providers.deepseek]\nkey = { env = \"X\" }\n\n[providers.deepseek.models.\"written-only\"]\nwindow = 64000\n\n\
                [models]\nchat = \"deepseek/in-use\"\n";
    let parsed = parse(&items, Layer::System, text).unwrap();
    let layers = Layers {
        system: Some(&parsed),
        ..Layers::default()
    };
    let resolved = merge(&items, &layers, &|_| None);
    let values = resolved.values();
    let catalog = crate::settings::catalog::Catalog::load(&paths).unwrap();
    let names = |listed: &[String]| -> Vec<(String, &'static str)> {
        catalog
            .offer(&values, &resolved, "deepseek", listed)
            .models
            .into_iter()
            .map(|model| (model.name, model.from))
            .collect()
    };

    let without = names(&[]);
    assert!(without.contains(&("written-only".to_string(), "config")));
    assert!(without.contains(&("in-use".to_string(), "config")));
    // 这个临时目录里没有模型目录，所以这里看不到目录里的那些；有目录时它们照旧接在后面。

    let listed = ["only-listed".to_string(), "another".to_string()];
    assert_eq!(
        names(&listed),
        [
            ("only-listed".to_string(), "provider"),
            ("another".to_string(), "provider")
        ],
        "报得出列表的一个不多一个不少，先后照它的"
    );
}
