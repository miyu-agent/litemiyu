//! 模型的资料（施工 8-7）：每一格各查各的，价格整份不拼，第 3、4 层借哪些，本机的当免费，倍率谁盖谁，来源一字不差。

use crate::config::Layer;
use crate::config::merge::{Layers, Resolved, merge};
use crate::config::parse::parse;
use crate::kernel::time::Timestamp;
use serde_json::json;

use super::*;
use crate::models::observed::{ListedModel, ProviderList};
use crate::models::provider::provider;
use crate::models::test_support::{Held, items, resolved};

/// 档案只认 OpenAI 兼容的包，带上裁出来的目录。
fn held() -> Held {
    Held::new(
        json!({"npm": {"@ai-sdk/openai-compatible": "openai-chat"}}),
        true,
    )
}

fn at(text: &str) -> Timestamp {
    Timestamp::parse(text).expect("时刻写法对")
}

/// 照配置 `source` 查 `id` 这一家的 `model`。
fn facts_of(held: &Held, source: &str, id: &str, model: &str) -> (Facts, Found) {
    let resolved = resolved(source);
    let knowledge = held.knowledge();
    let provider = provider(&resolved.values(), &knowledge, id).expect("配了");
    facts(&resolved, &knowledge, &provider, model)
}

/// 手写的写成配置文件（施工 SU-5 上：只有数据根下那一份）。
fn file(_layer: Layer) -> String {
    "config.toml".to_string()
}

const DEEPSEEK: &str = "[providers.deepseek]\nlocal = false\n";

#[test]
fn every_fact_from_the_catalog_carries_its_entry_layer_and_date() {
    let (facts, found) = facts_of(&held(), DEEPSEEK, "deepseek", "deepseek-flash");
    assert!(matches!(found, Found::Matched(_)));
    let from = json!({"from": "catalog", "entry": "deepseek/deepseek-flash", "layer": 2, "fetched": "2026-10-01T03:25:54.000Z"});
    let with = |value: serde_json::Value| {
        let mut fact = from.clone();
        fact["value"] = value;
        fact
    };
    assert_eq!(
        facts.json(&file),
        json!({
            "window": with(json!(1_000_000)),
            "max_output": with(json!(393_216)),
            "inputs": with(json!(["text", "image"])),
            "tools": with(json!(true)),
            "reasoning": with(json!(["low", "high", "max"])),
            "effort": {"value": null, "from": "default"},
            "takes_temperature": with(json!(true)),
            "temperature": {"value": null, "from": "default"},
            "name": with(json!("DeepSeek V4.1 Flash")),
            "status": {"value": null, "from": "default"},
        })
    );
    assert_eq!(
        facts.driver_inputs(),
        Inputs {
            images: true,
            pdf: false
        }
    );
}

/// 每一格各查各的：手写了窗口的，最大输出照样从目录借；手写的来源写文件和行。
#[test]
fn each_fact_is_looked_up_on_its_own() {
    let source = "[providers.deepseek]\nlocal = false\n\n[providers.deepseek.models.\"deepseek-flash\"]\nwindow = 60000\ninputs = [\"text\"]\ntools = false\nreasoning = [\"high\"]\n";
    let (facts, _) = facts_of(&held(), source, "deepseek", "deepseek-flash");
    assert_eq!(facts.window.value, Some(60_000));
    assert_eq!(
        facts.window.source.json(&file),
        json!({"from": "config", "file": "config.toml", "line": 5, "layer": "system"})
            .as_object()
            .cloned()
            .expect("对象")
    );
    assert_eq!(facts.max_output.value, Some(393_216));
    assert!(matches!(facts.max_output.source, Source::Catalog { .. }));
    assert_eq!(facts.inputs.value, ["text"]);
    assert_eq!(facts.tools.value, Some(false));
    assert_eq!(facts.reasoning.value, Some(vec!["high".to_string()]));
}

/// 思考强度（施工 8-18）：目录的开关只在档案写了开关时算，多一档 `off`；手写的几档规整过、盖过目录的；默认的那一档只认
/// 写在档位里的，`none` 读成 `off`，来源写文件和行；不在档位里的照没写。
#[test]
fn reasoning_levels_follow_the_profile_and_the_default_must_be_one_of_them() {
    let toggled = Held::new(
        json!({
            "npm": {"@ai-sdk/openai-compatible": "openai-chat"},
            "providers": {"deepseek": {"compat": {"toggle": {"field": "thinking", "on": true, "off": false}}}}
        }),
        true,
    );
    let (facts, _) = facts_of(&toggled, DEEPSEEK, "deepseek", "deepseek-flash");
    assert_eq!(
        facts.reasoning.value,
        Some(["off", "low", "high", "max"].map(str::to_string).to_vec()),
        "档案写了开关：多一档 off"
    );
    let (plain, _) = facts_of(&held(), DEEPSEEK, "deepseek", "deepseek-flash");
    assert_eq!(plain.levels(), ["low", "high", "max"], "档案没写开关：不加");
    let written = "[providers.deepseek]\nlocal = false\n\n[providers.deepseek.models.\"deepseek-flash\"]\neffort = \"none\"\n";
    let (facts, _) = facts_of(&toggled, written, "deepseek", "deepseek-flash");
    assert_eq!(facts.effort.value.as_deref(), Some("off"));
    assert_eq!(
        Json::Object(facts.effort.source.json(&file)),
        json!({"from": "config", "file": "config.toml", "line": 5, "layer": "system"})
    );
    let (facts, _) = facts_of(&held(), written, "deepseek", "deepseek-flash");
    assert_eq!(
        (facts.effort.value, facts.effort.source),
        (None, Source::Default),
        "没有开关就没有 off：照没写"
    );
    let own = "[providers.deepseek]\nlocal = false\n\n[providers.deepseek.models.\"deepseek-flash\"]\nreasoning = [\"disabled\", \"turbo\"]\neffort = \"turbo\"\n";
    let (facts, _) = facts_of(&toggled, own, "deepseek", "deepseek-flash");
    assert_eq!(facts.levels(), ["off", "turbo"], "手写的盖过目录，照样规整");
    assert_eq!(facts.effort.value.as_deref(), Some("turbo"));
}

/// 系统配置 `system` 合出来的最终值（施工 SU-3 上：只剩这一层）。
fn one_layer(system: &str) -> Resolved {
    let item_list = items();
    let system = parse(&item_list, Layer::System, system).expect("写法对");
    let layers = Layers {
        system: Some(&system),
        ..Layers::default()
    };
    merge(&item_list, &layers, &|_| None)
}

/// 照配置查 `deepseek-flash` 的默认思考强度：值和来源。
fn effort_of(held: &Held, system: &str) -> (Option<String>, Source) {
    let resolved = one_layer(system);
    let knowledge = held.knowledge();
    let provider = provider(&resolved.values(), &knowledge, "deepseek").expect("配了");
    let (facts, _) = facts(&resolved, &knowledge, &provider, "deepseek-flash");
    (facts.effort.value, facts.effort.source)
}

/// 写了、没写：默认的思考强度（施工 8-18（补）去掉会话那一层；SU-3 上起配置只剩系统那一层，「怎么走」第十一条第 2 条）。
#[test]
fn effort_default_is_whatever_the_config_writes() {
    let held = held();
    let system =
        format!("{DEEPSEEK}\n[providers.deepseek.models.\"deepseek-flash\"]\neffort = \"low\"\n");
    let (value, source) = effort_of(&held, &system);
    assert_eq!(value.as_deref(), Some("low"), "写了");
    assert!(matches!(
        source,
        Source::Config {
            layer: Layer::System,
            ..
        }
    ));
    let (value, source) = effort_of(&held, DEEPSEEK);
    assert_eq!(value, None, "不写：没有默认");
    assert_eq!(source, Source::Default);
}

/// 来源是配置的，`layer` 格说是哪一层（施工 8-7（补），照 `config.get` 说的来源一样写法）：只剩系统配置。
#[test]
fn facts_json_says_which_config_layer_a_value_came_from() {
    let held = held();
    let system = "[providers.deepseek]\nlocal = false\n\n[providers.deepseek.models.\"deepseek-flash\"]\nwindow = 60000\n";
    let resolved = one_layer(system);
    let knowledge = held.knowledge();
    let provider = provider(&resolved.values(), &knowledge, "deepseek").expect("配了");
    let (facts, _) = facts(&resolved, &knowledge, &provider, "deepseek-flash");
    assert_eq!(facts.window.value, Some(60_000));
    assert_eq!(
        Json::Object(facts.window.source.json(&file)),
        json!({"from": "config", "file": "config.toml", "line": 5, "layer": "system"})
    );
}

/// 别的来源不带 `layer`：学来的、供应商列表、本机的、驱动默认都没有这一格，只有手写的配置带。
#[test]
fn non_config_sources_carry_no_layer_field() {
    let sources = [
        Source::Learned {
            at: at("2026-10-01T08:12:30.000Z"),
        },
        Source::Provider {
            fetched: at("2026-10-01T03:00:00.000Z"),
        },
        Source::Default,
    ];
    for source in sources {
        assert!(
            !source.json(&file).contains_key("layer"),
            "{source:?} 不该带 layer"
        );
    }
}

/// 窗口：手写的、用出来的、供应商的列表、目录，先有的算。
#[test]
fn the_window_goes_written_learned_listed_catalog() {
    let mut held = held();
    let learned_at = at("2026-10-01T08:12:30.000Z");
    let fetched = at("2026-10-01T03:00:00.000Z");
    held.lists.insert(
        "deepseek".to_string(),
        ProviderList {
            fetched,
            models: vec![ListedModel {
                id: "deepseek-flash".to_string(),
                window: Some(500_000),
                ..ListedModel::default()
            }],
        },
    );
    let (facts, _) = facts_of(&held, DEEPSEEK, "deepseek", "deepseek-flash");
    assert_eq!(facts.window.value, Some(500_000));
    assert_eq!(facts.window.source, Source::Provider { fetched });
    held.learned
        .learn("deepseek", "deepseek-flash", 65_536, learned_at);
    let (facts, _) = facts_of(&held, DEEPSEEK, "deepseek", "deepseek-flash");
    assert_eq!(facts.window.value, Some(65_536));
    assert_eq!(facts.window.source, Source::Learned { at: learned_at });
    assert_eq!(
        facts.window.source.json(&file),
        json!({"from": "learned", "at": "2026-10-01T08:12:30.000Z"})
            .as_object()
            .cloned()
            .expect("对象")
    );
    let written =
        format!("{DEEPSEEK}\n[providers.deepseek.models.\"deepseek-flash\"]\nwindow = 70000\n");
    let (facts, _) = facts_of(&held, &written, "deepseek", "deepseek-flash");
    assert_eq!(facts.window.value, Some(70_000), "手写的盖过用出来的");
    // 列表、用出来的只管这一家：别家同名的不算。
    let (facts, _) = facts_of(
        &held,
        "[providers.other]\ndriver = \"openai-chat\"\nbase_url = \"https://x.invalid\"\n",
        "other",
        "deepseek-flash",
    );
    assert!(matches!(
        facts.window.source,
        Source::Catalog { layer: 3, .. }
    ));
}

/// 扩展驱动的列表还报最大输出、能不能调工具、看不看得了图（施工 MD-4，11-07 定）：没写的照列表，来源是这一份列表；手写的
/// 照旧压过它；看不了图、没说的不改输入种类。
#[test]
fn a_list_can_tell_max_output_tools_and_vision() {
    let mut held = held();
    let fetched = at("2026-11-07T03:00:00.000Z");
    held.lists.insert(
        "deepseek".to_string(),
        ProviderList {
            fetched,
            models: vec![
                ListedModel {
                    id: "deepseek-flash".to_string(),
                    max_output: Some(9_000),
                    tools: Some(false),
                    vision: Some(true),
                    ..ListedModel::default()
                },
                ListedModel {
                    id: "deepseek-text".to_string(),
                    vision: Some(false),
                    ..ListedModel::default()
                },
            ],
        },
    );
    let (facts, _) = facts_of(&held, DEEPSEEK, "deepseek", "deepseek-flash");
    let listed = Source::Provider { fetched };
    assert_eq!(
        (facts.max_output.value, &facts.max_output.source),
        (Some(9_000), &listed)
    );
    assert_eq!(
        (facts.tools.value, &facts.tools.source),
        (Some(false), &listed)
    );
    assert_eq!(facts.inputs.value, ["text", "image"]);
    assert_eq!(facts.inputs.source, listed);
    let (facts, _) = facts_of(&held, DEEPSEEK, "deepseek", "deepseek-text");
    assert_ne!(facts.inputs.source, listed, "看不了图的不改");
    let written = format!(
        "{DEEPSEEK}\n[providers.deepseek.models.\"deepseek-flash\"]\nmax_output = 100\ntools = true\ninputs = [\"text\"]\n"
    );
    let (facts, _) = facts_of(&held, &written, "deepseek", "deepseek-flash");
    assert_eq!(
        (
            facts.max_output.value,
            facts.tools.value,
            facts.inputs.value
        ),
        (Some(100), Some(true), vec!["text".to_string()]),
        "手写的压过列表"
    );
}

/// 第 3、4 层：能力、窗口、名字照借。
#[test]
fn name_matches_borrow_the_capabilities() {
    let relay = "[providers.newapi]\ndriver = \"openai-chat\"\nbase_url = \"https://relay.example.invalid/v1\"\n";
    let (facts, _) = facts_of(&held(), relay, "newapi", "deepseek-v4.1-flash");
    let from = |source: &Source| match source {
        Source::Catalog { entry, layer, .. } => Some((entry.clone(), *layer)),
        _ => None,
    };
    let above = Some(("above/deepseek-v4.1-flash".to_string(), 3));
    assert_eq!(facts.window.value, Some(1_000_000));
    assert_eq!(from(&facts.window.source), above);
    assert_eq!(from(&facts.tools.source), above);
}

/// 手写指定的条目不存在：不借目录，交回标出来的那一条；没有目录的什么都不借。
#[test]
fn a_missing_hand_pick_borrows_nothing() {
    let source = format!(
        "{DEEPSEEK}\n[providers.deepseek.models.\"deepseek-flash\"]\ncatalog = \"deepseek/nope\"\n"
    );
    let (facts, found) = facts_of(&held(), &source, "deepseek", "deepseek-flash");
    assert_eq!(found, Found::Missing("deepseek/nope".to_string()));
    assert_eq!(facts.window.value, None);
    assert_eq!(facts.name.value, "deepseek-flash");
    assert_eq!(facts.name.source, Source::Default);
    assert_eq!(facts.inputs.value, ["text"], "驱动的保守默认：只有文字");
    let mut empty = held();
    empty.catalog = None;
    let resolved = resolved(DEEPSEEK);
    let deepseek = crate::models::provider::Provider {
        id: "deepseek".to_string(),
        driver: crate::models::provider::Driver::OpenAiChat,
        base_url: crate::config::Address::Literal("https://api.deepseek.com".to_string()),
        compat: crate::drivers::openai_chat::Compat::default(),
        key: None,
        images: None,
        catalog: "deepseek".to_string(),
        recognized: None,
        local: false,
        driver_written: false,
        driver_from: crate::models::provider::DriverFrom::Profile,
        reasoning_written: false,
        headers: std::collections::BTreeMap::new(),
        placeholders: Vec::new(),
    };
    let (facts, found) = super::facts(&resolved, &empty.knowledge(), &deepseek, "deepseek-flash");
    assert_eq!(found, Found::Nothing);
    assert_eq!(facts.window.source, Source::Default);
}

/// 施工 8-12：`anthropic` 的开关是接口自带的，目录里有开关的模型多一档 `off`；同一个模型走 openai-chat、档案没写开关的不加。
#[test]
fn anthropic_can_switch_thinking_off_without_a_profile() {
    let catalog = json!({"anthropic": {"id": "anthropic", "name": "Anthropic", "npm": "@ai-sdk/anthropic",
        "env": ["ANTHROPIC_API_KEY"], "models": {"claude-sonnet-5": {"id": "claude-sonnet-5", "name": "Claude Sonnet 5",
            "reasoning": true, "reasoning_options": [{"type": "toggle"}, {"type": "effort", "values": ["low", "high"]}],
            "limit": {"context": 1_000_000, "output": 128_000}, "modalities": {"input": ["text", "image", "pdf"], "output": ["text"]}}}}});
    let mut held = Held::new(json!({}), false);
    held.catalog = Some(crate::models::catalog::Loaded {
        catalog: crate::models::catalog::Catalog::parse(&catalog.to_string())
            .expect("读得进")
            .catalog,
        source: crate::models::catalog::CatalogSource::Snapshot,
        fetched: "2026-10-03T00:00:00.000Z".to_string(),
    });
    let source = "[providers.anthropic]\ndriver = \"anthropic\"\nbase_url = \"https://api.anthropic.com/v1\"\n\n\
        [providers.relay]\ndriver = \"openai-chat\"\nbase_url = \"https://relay.invalid/v1\"\ncatalog = \"anthropic\"\n\n\
        [providers.gpt]\ndriver = \"openai-responses\"\nbase_url = \"https://gpt.invalid/v1\"\ncatalog = \"anthropic\"\n";
    let (claude, _) = facts_of(&held, source, "anthropic", "claude-sonnet-5");
    assert_eq!(claude.levels(), ["off", "low", "high"]);
    assert_eq!(claude.max_output.value, Some(128_000));
    let (relayed, _) = facts_of(&held, source, "relay", "claude-sonnet-5");
    assert_eq!(
        relayed.levels(),
        ["low", "high"],
        "openai-chat 照档案的开关"
    );
    // 施工 8-13：openai-responses 没有开关，目录的开关不算。
    let (responses, _) = facts_of(&held, source, "gpt", "claude-sonnet-5");
    assert_eq!(responses.levels(), ["low", "high"]);
}

mod wire;
