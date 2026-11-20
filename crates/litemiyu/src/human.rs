//! 给人看的字（工具的显示名、说法的模板）：编进程序的表，一种语言内核一份、每个包一份（`resources/core/human/`、
//! `resources/tui/human/<包>/`）。Miyu 的终端连上核心时向它要（`human.get`）；LiteMiyu 第一次开会话时读选中的
//! 那一种，另一种不进内存。没读的（还没开会话）是空的，工具名照原名写。

use std::collections::BTreeMap;

use lm_kernel::kernel::event::Said;
use lm_kernel::kernel::template::Template;
use serde::Deserialize;
use serde_json::Value;

use phrases::Phrases;

/// MCP 的工具名的开头：`mcp__<服务>__<工具>`。
const MCP: &str = "mcp__";

/// 编进程序的表：语言、编号的前缀、原文。
const TABLES: [(&str, &str, &str); 4] = [
    (
        "zh",
        "core",
        include_str!(concat!(env!("OUT_DIR"), "/core/human/zh.json")),
    ),
    (
        "zh",
        "software/basesystem",
        tui_json!("human/basesystem/zh.json"),
    ),
    (
        "en",
        "core",
        include_str!(concat!(env!("OUT_DIR"), "/core/human/en.json")),
    ),
    (
        "en",
        "software/basesystem",
        tui_json!("human/basesystem/en.json"),
    ),
];

/// 一件工具给人看的样子（Miyu `miyu-store` 的 `human::Face`）。
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Face {
    /// 显示名，例如「读取」。
    pub name: String,
    /// 显示名后面跟哪一个参数的值，例如 `file_path`。没有的只写显示名。
    #[serde(default)]
    pub subject: Option<String>,
    /// 写在最前面的符号，例如 `→`。没有的由界面定。
    #[serde(default)]
    pub icon: Option<String>,
    /// 标题下面还印一块什么。没有的只印标题。
    #[serde(default)]
    pub block: Option<FaceBlock>,
}

/// 标题下面的那一块。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FaceBlock {
    /// 执行命令：标题写成 `$ 命令`，不写显示名；下面印工具自己写的结果。
    Command,
    /// 改动：下面印参数 `edits` 里每一处改掉的、改成的。
    Edits,
}

/// 给人看的字。
#[derive(Debug, Clone, Default)]
pub struct Human {
    /// 工具名到它的样子。
    tools: BTreeMap<String, Face>,
    /// 说法的模板原文。
    said: Phrases,
}

impl Human {
    /// 读 `language` 那一种的表；没有这种语言的照英文。
    pub fn load(language: &str) -> Human {
        let (tools, said) = tables(language);
        Human::from_reply(&serde_json::json!({"tools": tools, "said": said}))
    }

    /// 照 `{"tools": …, "said": …}` 造：读不懂的那一件、那一句不要。
    pub(crate) fn from_reply(reply: &Value) -> Human {
        // 一格格放进去，不 `collect`：`collect` 成 `BTreeMap` 要另展开一份稳定排序（对象本来就照名字排好、不重名）。
        let mut tools = BTreeMap::new();
        for (name, face) in reply["tools"].as_object().into_iter().flatten() {
            if let Ok(face) = lm_kernel::kernel::json::from_value(face.clone()) {
                tools.insert(name.clone(), face);
            }
        }
        Human {
            tools,
            said: Phrases::from_said(&reply["said"]),
        }
    }

    /// 一件工具的样子；表里没有的是 `None`。
    pub fn tool(&self, name: &str) -> Option<&Face> {
        self.tools.get(name)
    }

    /// 一件工具给人看的名字：表里有的照表；MCP 的照 `服务 · 工具`；别的照原名。
    pub fn tool_name(&self, name: &str) -> String {
        if let Some(face) = self.tool(name) {
            return face.name.clone();
        }
        name.strip_prefix(MCP)
            .and_then(|rest| rest.split_once("__"))
            .filter(|(service, tool)| !service.is_empty() && !tool.is_empty())
            .map_or_else(
                || name.to_string(),
                |(service, tool)| format!("{service}{}{tool}", crate::sheet::sep()),
            )
    }

    /// 时间线上一步的名字：调用那一刻记下的显示名照界面语言 `lang` 挑，挑不到照英文、中文；都没有的照这一刻的表。
    pub fn step_name(&self, name: &str, display: &BTreeMap<String, String>, lang: &str) -> String {
        [lang, "en", "zh"]
            .iter()
            .find_map(|l| display.get(*l))
            .cloned()
            .unwrap_or_else(|| self.tool_name(name))
    }

    /// 照说法换出字；表里没有这一句、模板坏了、字段不够的是 `None`。
    pub fn say(&self, said: &Said) -> Option<String> {
        let template = Template::parse(self.said.get(&said.key)?).ok()?;
        // 一格格放进去，不 `collect`（同 `from_reply`）。
        let mut fields: BTreeMap<&str, &str> = BTreeMap::new();
        for (field, value) in &said.fields {
            fields.insert(field.as_str(), value.as_str());
        }
        template.fill(&fields, clean).ok()
    }
}

/// `language` 那一种（没有的照英文）的表并成一份：工具名到样子、说法的编号到模板原文。编号照所在的地方往下写
/// （`core/…`、`software/<包>/…`），说法管着的数是 1 时多接一段 `/one`，表里没写单数的照它本身（Miyu `miyu-store` 的
/// `human`）。
fn tables(
    language: &str,
) -> (
    serde_json::Map<String, Value>,
    serde_json::Map<String, Value>,
) {
    let language = if TABLES.iter().any(|(code, ..)| *code == language) {
        language
    } else {
        "en"
    };
    let mut tools = serde_json::Map::new();
    let mut said = serde_json::Map::new();
    for (_, prefix, text) in TABLES.iter().filter(|(code, ..)| *code == language) {
        // 编进程序的表由测试守着读得懂；读不懂的那一份当没有。
        let Ok(table) = lm_kernel::kernel::json::from_str::<Value>(text) else {
            continue;
        };
        if let Some(faces) = table["tools"].as_object() {
            tools.extend(faces.clone());
        }
        for (key, source) in table["said"].as_object().into_iter().flatten() {
            said.insert(format!("{prefix}/{key}"), source.clone());
        }
    }
    // 说法管着的数是 1 时编号多接一段 `/one`：表里没写单数的（中文全是）照它本身（Miyu `miyu-store` 的 `human`）。
    let singular: Vec<(String, Value)> = said
        .iter()
        .filter(|(key, _)| !key.ends_with("/one") && !said.contains_key(&format!("{key}/one")))
        .map(|(key, source)| (format!("{key}/one"), source.clone()))
        .collect();
    said.extend(singular);
    (tools, said)
}

/// 给人看的字段：控制字符换成 `�`，别的照原样（Miyu `miyu-store` 的 `human::clean`）。
fn clean(value: &str) -> String {
    value
        .chars()
        .map(|c| if c.is_control() { '\u{FFFD}' } else { c })
        .collect()
}

mod phrases;
#[cfg(test)]
mod tests;
