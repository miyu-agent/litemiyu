//! 给人看的字：编进程序的表读得出工具的显示名、填得出说法。

use serde_json::json;

use super::Human;
use lm_kernel::kernel::event::Said;

fn said(key: &str, fields: &[(&str, &str)]) -> Said {
    Said {
        key: key.into(),
        fields: fields
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect(),
    }
}

#[test]
fn the_reply_gives_tool_faces_and_fills_phrases() {
    let human = Human::from_reply(&json!({
        "language": "zh",
        "said": {"software/basesystem/read/lines": "{count} 行", "core/bad": "{没收尾"},
        "tools": {"read": {"name": "读取", "subject": "file_path"}, "odd": 3}
    }));
    let face = human.tool("read").unwrap();
    assert_eq!(
        (face.name.as_str(), face.subject.as_deref()),
        ("读取", Some("file_path"))
    );
    assert!(human.tool("odd").is_none(), "读不懂的那一件不要");
    assert_eq!(
        human
            .say(&said("software/basesystem/read/lines", &[("count", "3")]))
            .as_deref(),
        Some("3 行")
    );
    assert_eq!(human.say(&said("core/bad", &[])), None, "坏模板那一句不要");
    assert_eq!(Human::default().tool("read"), None, "没要到的是空的");
}

#[test]
fn an_mcp_tool_without_a_face_is_named_by_its_service_and_tool() {
    let human = Human::default();
    assert_eq!(
        human.tool_name("mcp__github__create_issue"),
        "github · create_issue"
    );
    assert_eq!(
        human.tool_name("mcp__fs__read__file"),
        "fs · read__file",
        "工具名里带 __ 的照原样"
    );
    assert_eq!(human.tool_name("mcp____x"), "mcp____x", "服务名空着的不认");
    assert_eq!(human.tool_name("read"), "read");
    let named = Human::from_reply(&serde_json::json!({"tools": {"read": {"name": "读取"}}}));
    assert_eq!(named.tool_name("read"), "读取");
}

#[test]
fn a_step_takes_the_name_recorded_at_call_time_in_the_shown_language() {
    // 核心 9-8 六补：照界面语言挑，挑不到照 en、zh 的先后；都没有的照这一刻的工具表。
    let human = Human::from_reply(&json!({"tools": {"read": {"name": "读取"}}}));
    let names = |pairs: &[(&str, &str)]| -> std::collections::BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(l, n)| ((*l).to_string(), (*n).to_string()))
            .collect()
    };
    let both = names(&[("en", "Dice"), ("zh", "掷骰子")]);
    assert_eq!(human.step_name("roll", &both, "zh"), "掷骰子");
    assert_eq!(
        human.step_name("roll", &both, "ja"),
        "Dice",
        "没有那种语言的照英文"
    );
    assert_eq!(
        human.step_name("roll", &names(&[("zh", "掷骰子")]), "ja"),
        "掷骰子",
        "没有英文照中文"
    );
    assert_eq!(
        human.step_name("read", &names(&[]), "zh"),
        "读取",
        "没记下的照这一刻的表"
    );
}

#[test]
fn the_shipped_tables_name_the_tools_and_fill_their_phrases() {
    for language in ["zh", "en"] {
        let human = Human::load(language);
        assert_ne!(
            human.tool_name("read"),
            "read",
            "{language}：基础系统的表读进来了"
        );
    }
    let human = Human::load("zh");
    assert_eq!(human.tool_name("read"), "读取");
    // 内核那一份、包那一份的说法都在，编号照所在的地方往下写；单数那一句没写的照它本身。
    assert!(
        human
            .said
            .get("core/tool-results/cancelled-before")
            .is_some()
    );
    let lines = |n: &str| said("software/basesystem/read/lines/one", &[("count", n)]);
    assert_eq!(
        human.say(&lines("1")).as_deref(),
        Some("1 行"),
        "中文没写单数，照它本身"
    );
    let english = Human::load("en");
    assert_eq!(
        english.say(&lines("1")).as_deref(),
        Some("1 line"),
        "英文写了单数"
    );
    assert_eq!(
        Human::load("fr").tool_name("read"),
        "Read",
        "没有的语言照英文"
    );
}

#[test]
fn sayings_about_the_program_itself_call_it_litemiyu() {
    // 说到程序自己的几句（数据目录不让碰、高危命令拒了、配置参考的头一行）写 LiteMiyu，不写 Miyu。
    let keys = [
        "core/permissions/forbidden",
        "core/config/reference-header",
        "software/basesystem/shell/hazard-delete_data_root",
    ];
    for language in ["zh", "en"] {
        let human = Human::load(language);
        for key in keys {
            let text = human
                .say(&said(key, &[("path", "/x")]))
                .unwrap_or_else(|| panic!("{language} 有 {key}"));
            assert!(text.contains("LiteMiyu"), "{language} {key}：{text}");
        }
    }
}

#[test]
fn every_phrase_the_cross_window_tools_say_has_words_in_both_languages() {
    // 窗口之间的几件（`send_message` 连同「空了告诉我」、`sessions`、`history`）结果给人看的说法：引擎交的编号
    // （`lm-engine/src/basesystem/{send_message,sessions,history}.rs`）两种语言都有字，不照编号原样写。
    let keys = [
        "send_message/sent",
        "send_message/held",
        "send_message/no-parent",
        "send_message/not-yours",
        "send_message/stopped",
        "send_message/not-sent",
        "send_message/too-many",
        "send_message/duplicate",
        "send_message/inbox-full",
        "send_message/watching",
        "send_message/watch-peers-only",
        "send_message/no-session",
        "send_message/ambiguous",
        "send_message/self",
        "send_message/not-here",
        "send_message/too-long",
        "sessions/listed",
        "sessions/none",
        "sessions/past-end",
        "sessions/failed",
        "history/found",
        "history/read",
        "history/none",
        "history/no-log",
        "history/no-session",
        "history/ambiguous",
        "history/not-here",
    ];
    for language in ["zh", "en"] {
        let human = Human::load(language);
        for key in keys {
            let fields = [
                ("to", "abcd1234"),
                ("count", "2"),
                ("total", "3"),
                ("error", "e"),
                ("session", "s"),
                ("from", "1"),
                ("chars", "9"),
                ("limit", "8"),
            ];
            let full = format!("software/basesystem/{key}");
            let said = human.say(&said(&full, &fields));
            assert!(
                said.as_deref().is_some_and(|s| !s.contains('{')),
                "{language} {key}：{said:?}"
            );
        }
    }
}

/// 内核那一份给人看的说法（`resources/core/human/{zh,en}.json`）每一句都有人用（LM-8，照 LM-3 给命令行字表加的
/// `every_key_in_both_languages_is_used`）：Miyu 搬来的软件包、扩展、人格、预设、网页那些 LiteMiyu 走不到的句子删了，以后不再留。
/// 「有人用」照源码（不算测试）里的字串字面量认：写成编号本身、带 `core/` 的完整说法；另认三类拼出来的：
/// `tool-results/<名字>`（`kernel/tool/texts.rs` 照 `resources/core/tool-results/<名字>.txt` 拼）、`config/layer/<层>`、
/// `config/applies/<什么时候生效>`（`config/words.rs` 照枚举拼）；英文单数的 `…/one` 照去掉它的那一句算。
#[test]
fn every_core_saying_is_used() {
    use std::collections::BTreeSet;
    use std::path::Path;

    fn walk(dir: &Path, into: &mut BTreeSet<String>) {
        for entry in std::fs::read_dir(dir).expect("读得了源码目录").flatten() {
            let path = entry.path();
            let name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default();
            if path.is_dir() {
                if !matches!(name, "tests" | "testkit" | "test_support" | "target") {
                    walk(&path, into);
                }
            } else if name.ends_with(".rs")
                && !name.ends_with("tests.rs")
                && !matches!(name, "test_support.rs" | "testkit.rs")
            {
                let text = std::fs::read_to_string(&path).expect("读得了源码");
                into.extend(text.split('"').skip(1).step_by(2).map(str::to_string));
            }
        }
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut literals = BTreeSet::new();
    walk(&root.join("crates"), &mut literals);
    let tool_results: BTreeSet<String> =
        std::fs::read_dir(root.join("resources/core/tool-results"))
            .expect("读得了出厂的工具结果")
            .flatten()
            .filter_map(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                name.strip_suffix(".txt").map(str::to_string)
            })
            .collect();
    for code in ["zh", "en"] {
        let path = root.join(format!("resources/core/human/{code}.json"));
        let table: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).expect("读得了"))
                .expect("是 JSON");
        let said = table["said"].as_object().expect("有 said");
        let unused: Vec<&String> = said
            .keys()
            .filter(|key| {
                let base = key.strip_suffix("/one").unwrap_or(key);
                let named = literals.contains(base) || literals.contains(&format!("core/{base}"));
                let built = base
                    .strip_prefix("tool-results/")
                    .is_some_and(|name| tool_results.contains(name))
                    || base.starts_with("config/layer/")
                    || base.starts_with("config/applies/");
                !(named || built)
            })
            .collect();
        assert!(unused.is_empty(), "{code} 里没人用的：{unused:?}");
        assert!(said.len() > 50, "{code} 读到了：{}", said.len());
    }
}

/// 出厂的每件工具（`resources/software/<包>/tools/` 下的每一份说明）两种语言都有显示名：时间线里不直接印工具的原名（LM-20，
/// 2026-11-20 项目主人：`schedule_wakeup` 那一步只印了原名）。
#[test]
fn every_shipped_tool_has_a_name_people_see() {
    let software =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../resources/software");
    let mut shipped: Vec<String> = std::fs::read_dir(&software)
        .unwrap()
        .filter_map(|package| std::fs::read_dir(package.ok()?.path().join("tools")).ok())
        .flatten()
        .filter_map(|tool| tool.ok()?.path().file_stem()?.to_str().map(str::to_string))
        .collect();
    shipped.sort();
    assert!(shipped.contains(&"schedule_wakeup".to_string()) && shipped.len() >= 12);
    for language in ["zh", "en"] {
        let human = Human::load(language);
        let unnamed: Vec<&String> = shipped
            .iter()
            .filter(|tool| human.tool(tool).is_none())
            .collect();
        assert!(unnamed.is_empty(), "{language}：没有显示名的 {unnamed:?}");
    }
}

#[test]
fn a_scheduled_continuation_reads_in_both_languages() {
    let set = said(
        "software/loop/wakeup/set",
        &[
            ("next", "2026-11-20 13:07"),
            ("reason", "CI 大约十分钟"),
            ("delay", "10m"),
        ],
    );
    let zh = Human::load("zh");
    let face = zh.tool("schedule_wakeup").expect("有显示名");
    assert_eq!(
        (face.name.as_str(), face.subject.as_deref()),
        ("定时触发", Some("reason"))
    );
    assert_eq!(zh.say(&set).as_deref(), Some("10m 后（2026-11-20 13:07）"));
    assert_eq!(
        zh.say(&said("software/loop/wakeup/stopped", &[]))
            .as_deref(),
        Some("已停止")
    );
    let en = Human::load("en");
    assert_eq!(en.tool_name("schedule_wakeup"), "Schedule");
    assert_eq!(en.say(&set).as_deref(), Some("in 10m (2026-11-20 13:07)"));
    assert_eq!(
        en.say(&said("software/loop/wakeup/stopped", &[]))
            .as_deref(),
        Some("Stopped")
    );
    for key in [
        "wakeup/no-prompt",
        "wakeup/no-loop",
        "wakeup/no-delay",
        "common/off",
    ] {
        let key = format!("software/loop/{key}");
        assert!(zh.say(&said(&key, &[])).is_some(), "中文有 {key}");
        assert!(en.say(&said(&key, &[])).is_some(), "英文有 {key}");
    }
}
