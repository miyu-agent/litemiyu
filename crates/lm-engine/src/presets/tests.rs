//! 预设：从配置里读出自己定义的那几张名单、挑默认预设、照预设收窄全局开关。

use std::collections::BTreeMap;

use lm_kernel::config::merge::{Layers, merge};
use lm_kernel::config::parse::parse;
use lm_kernel::config::{Layer, Values};

use super::{
    BARE, Chosen, FULL, TOOLS, choose, custom, default_of, listed, narrowed, removed, valid_name,
    written,
};
use crate::skills::switches;

/// 照配置的字 `text` 合出最终值（登记的全部配置项）。
fn values(text: &str) -> Values {
    let items = crate::configured::items();
    let parsed = parse(&items, Layer::System, text).expect("读得懂");
    let layers = Layers {
        system: Some(&parsed),
        ..Layers::default()
    };
    merge(&items, &layers, &|_| None).values()
}

const TWO: &str = r#"
[sessions]
default = "只用命令"

[presets."只用命令"]
unlisted = "off"

[presets."只用命令".features]
shell = true
jobs = true
mcp-files = true
no_such_tool = true

[presets.loose]
unlisted = "on"

[presets.loose.features]
subagent = false
skill-deploy = false

[presets.headless.features]
shell = true
"#;

#[test]
fn custom_presets_are_read_by_name_with_their_lists() {
    let found = custom(&values(TWO));
    let names: Vec<&str> = found.iter().map(|pin| pin.name.as_str()).collect();
    assert_eq!(names, ["loose", "只用命令"], "没写 unlisted 的那一张不算");
    let closed = &found[1];
    assert!(!closed.unlisted);
    assert!(closed.on("shell") && closed.on("jobs") && closed.on("mcp-files"));
    assert!(!closed.on("read"), "没写的照 unlisted 关着");
    assert!(
        closed.on("no_such_tool"),
        "写了现在没有的不报错，照写的记着"
    );
    let open = &found[0];
    assert!(open.unlisted && !open.on("subagent") && open.on("read"));
}

#[test]
fn the_two_built_in_presets_are_always_there_and_cannot_be_redefined() {
    let values = values("[presets.full]\nunlisted = \"off\"\n[presets.bare]\nunlisted = \"on\"\n");
    assert!(custom(&values).is_empty(), "自带的两个名字不能另写");
    assert_eq!(choose(&values, FULL), Some(Chosen::Full));
    assert_eq!(choose(&values, BARE), Some(Chosen::Bare));
    assert_eq!(choose(&values, "nope"), None);
}

#[test]
fn the_default_preset_falls_back_to_full_when_its_name_is_gone() {
    let (chosen, missing) = default_of(&values(TWO));
    assert_eq!(chosen.name(), "只用命令");
    assert_eq!(missing, None);
    let (chosen, missing) = default_of(&values(""));
    assert_eq!((chosen, missing), (Chosen::Full, None), "没写的是全部功能");
    let (chosen, missing) = default_of(&values("[sessions]\ndefault = \"bare\"\n"));
    assert_eq!(
        (chosen, missing),
        (Chosen::Bare, None),
        "LM-29 以前写的照旧认"
    );
    let (chosen, missing) = default_of(&values("[sessions]\ndefault = \"删掉了\"\n"));
    assert_eq!(chosen, Chosen::Full, "写的名字没有了：照全部功能");
    assert_eq!(
        missing.as_deref(),
        Some("删掉了"),
        "交回那个名字，主程序提示"
    );
}

#[test]
fn names_have_no_spaces_dots_or_leading_dash_and_are_not_the_built_in_ones() {
    for good in ["web", "只用命令", "a_b-2"] {
        assert!(valid_name(good), "{good}");
    }
    for bad in [
        "",
        "a b",
        "a.b",
        "-x",
        "full",
        "bare",
        "a\tb",
        &"长".repeat(33),
    ] {
        assert!(!valid_name(bad), "{bad:?}");
    }
}

#[test]
fn global_switches_and_the_preset_must_both_be_on() {
    let found = custom(&values(TWO));
    let (open, closed) = (&found[0], &found[1]);
    let global: BTreeMap<String, bool> = [("mcp-files", false), ("skill-notes", true)]
        .into_iter()
        .map(|(id, on)| (id.to_string(), on))
        .collect();
    // 没写的关着的预设：只有它写了开、全局开关也没关的才开。
    let narrow = narrowed(global.clone(), closed);
    assert!(
        !switches::on(&narrow, "mcp-files"),
        "全局开关关掉的，预设里开了也没有"
    );
    assert!(!switches::on(&narrow, "skill-notes"), "预设没写的关着");
    assert!(!switches::on(&narrow, "mcp-later"), "以后新装的也关着");
    let narrow = narrowed(BTreeMap::new(), closed);
    assert!(switches::on(&narrow, "mcp-files"), "两边都开着的才开");
    // 没写的开着的预设：它写了关的关着，别的照全局开关。
    let narrow = narrowed(global, open);
    assert!(!switches::on(&narrow, "skill-deploy"), "预设写了关的");
    assert!(!switches::on(&narrow, "mcp-files"), "全局开关关掉的");
    assert!(switches::on(&narrow, "skill-notes"));
    assert!(switches::on(&narrow, "mcp-later"), "以后新装的照开");
    assert!(
        !narrow.contains_key("subagent") && !narrow.contains_key("shell"),
        "内置工具不进技能、MCP 的开关表：{narrow:?}"
    );
}

#[test]
fn the_built_in_tools_are_the_shipped_catalog() {
    let mut tools = crate::basesystem::tools().expect("出厂的字读得进");
    tools.extend(crate::schedule::tools().expect("出厂的字读得进"));
    let catalog = crate::tool::Catalog::new(tools).expect("名字不重");
    let mut shipped: Vec<String> = catalog.specs().map(|spec| spec.name.clone()).collect();
    shipped.sort();
    let mut listed: Vec<&str> = TOOLS.to_vec();
    listed.sort_unstable();
    assert_eq!(listed, shipped, "配置界面列的内置工具和工具目录对得上");
}

#[test]
fn the_config_text_alone_tells_the_presets_and_the_default() {
    let read = listed(TWO);
    assert_eq!(read.default, "只用命令");
    assert!(!read.default_missing());
    let names: Vec<&str> = read.names().collect();
    assert_eq!(names, ["full", "bare", "loose", "只用命令"]);
    // 别的项写错了、不认识的不碍事；整份读不懂的照没写。
    let read = listed("[ui]\nlanguage = 7\n[nope]\nx = 1\n[presets.a]\nunlisted = \"on\"\n");
    assert_eq!(read.names().collect::<Vec<_>>(), ["full", "bare", "a"]);
    assert_eq!(read.default, "full");
    let broken = listed("[presets.a\nunlisted = ");
    assert_eq!(broken.names().collect::<Vec<_>>(), ["full", "bare"]);
    // 默认预设写的名字没有了：照全部功能。
    let gone = listed("[sessions]\ndefault = \"x\"\n");
    assert!(gone.default_missing());
    assert_eq!(gone.effective_default(), "full");
}

#[test]
fn a_preset_is_written_and_removed_through_the_settings_service() {
    let scratch = crate::store::test_support::Scratch::new();
    let paths = crate::store::paths::Paths::under(scratch.path());
    std::fs::create_dir_all(&paths.config).expect("建得了");
    let before = "# 我的配置\n[ui]\nlanguage = \"zh\"\n";
    std::fs::write(paths.config_file(), before).expect("写得进");
    let settings = crate::settings::Settings::new(paths.clone(), |_| None);
    let pin = lm_kernel::policy::PresetPin {
        name: "只用命令".to_string(),
        unlisted: false,
        features: [("shell", true), ("mcp-files", true)]
            .into_iter()
            .map(|(id, on)| (id.to_string(), on))
            .collect(),
    };
    let mut edits = written(&pin);
    edits.push(crate::settings::Edit::Set(
        super::DEFAULT.to_string(),
        lm_kernel::config::Value::Text(pin.name.clone().into()),
    ));
    settings.set(&edits).expect("写得进");
    let text = std::fs::read_to_string(paths.config_file()).expect("读得了");
    assert!(text.starts_with(before), "原来的字原样在前面：\n{text}");
    assert!(
        text.contains("[presets.\"只用命令\"]\nunlisted = \"off\"\n"),
        "{text}"
    );
    assert!(
        text.contains("[presets.\"只用命令\".features]\nmcp-files = true\nshell = true\n"),
        "{text}"
    );
    let read = super::read(&paths);
    assert_eq!(read.custom, vec![pin.clone()]);
    assert_eq!(read.default, "只用命令");
    // 删掉：它是默认预设，那一项跟着去掉；别的字照旧。
    settings.set(&removed(&pin, &read.default)).expect("删得了");
    let read = super::read(&paths);
    assert!(read.custom.is_empty());
    assert_eq!(read.default, "full");
    let text = std::fs::read_to_string(paths.config_file()).expect("读得了");
    assert!(text.starts_with(before), "{text}");
    assert!(
        !text.contains("unlisted") && !text.contains("shell"),
        "{text}"
    );
}
