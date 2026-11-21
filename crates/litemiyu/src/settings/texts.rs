//! 配置界面的字（Miyu 终端 `text/<语言>.json` 的 `settings`，蓝图「配置页」第 26 条）：编进程序，中、英两份，进配置界面时
//! 只读选中的那一种。`{name}` 这样的照名字换。

use std::collections::BTreeMap;

use crate::config::texts;

/// 编进程序的字：语言、原文。
const TEXTS: [(&str, &str); 2] = [
    ("zh", tui_json!("settings/zh.json")),
    ("en", tui_json!("settings/en.json")),
];

texts! {
    /// 一种语言的字。
    #[derive(Debug, Clone)]
    pub struct Texts {
        /// 面包屑的头一段。
        pub title: String,
        /// 值从哪来：`default`、`system`、`env`。
        pub origins: BTreeMap<String, String>,
        /// 开关的开、关。
        pub toggle: [String; 2],
        /// 没有值。
        pub empty: String,
        /// 几个名字连起来的标点（主菜单里一页的几组）。
        pub separator: String,
        /// 存成了：照这一项什么时候生效。
        pub applies: BTreeMap<String, String>,
        /// 恢复默认先问的：标题（`{name}`）、一句（`{from}`）、两个按钮。
        pub unset_ask: [String; 4],
        /// 恢复默认时配置里没写这一项。
        pub not_set: String,
        /// 没存成：`{why}`。
        pub failed: String,
        /// 配置文件读不懂：`{line}`、`{column}`。
        pub broken: String,
        /// 填的值不收：`{name}`。
        pub bad_value: String,
        /// 两行一组的按键提示：主菜单、一页、选择窗、编辑窗。
        pub key_hints: KeyHints,
        /// 勾选窗（从 Miyu 导入）。
        pub pools: PoolTexts,
        /// 通用页最后一行「提示词」（`prompt.md`）。
        pub prompt: PromptTexts,
        /// 「从 Miyu 导入」。
        pub import: ImportTexts,
        /// 试一家供应商通不通（`r`）。
        pub probe: ProbeTexts,
        /// 技能页、MCP 页。
        pub extras: ExtraTexts,
        /// 「预设」页。
        pub presets: PresetTexts,
        /// 第一次运行的开头。
        pub opening: super::opening::OpeningTexts,
        /// 「供应商和模型」那一页。
        pub models: super::models::texts::ModelTexts,
    }
}

texts! {
    /// 技能页、MCP 页的字（`extras.rs`）。
    #[derive(Debug, Clone)]
    pub struct ExtraTexts {
        /// 技能页在主菜单里的名字和底下那一行。
        pub skills: [String; 2],
        /// MCP 页在主菜单里的名字和底下那一行。
        pub mcp: [String; 2],
        /// 一个都没有：技能、MCP。
        pub empty: [String; 2],
        /// 自带的技能。
        pub shipped: String,
        /// 读不成的技能：`{what}` 是路径。
        pub unreadable: String,
        /// 配置里写错的服务：`{line}`、`{why}`。
        pub mcp_broken: String,
        /// 怎么起：命令（`{what}`）、网址（`{what}`）。
        pub command: String,
        /// 见上。
        pub url: String,
        /// 上次列到 `{n}` 件工具。
        pub tools: String,
        /// 还没起过（没有缓存）。
        pub never: String,
        /// 按键提示。
        pub hints: Vec<[String; 2]>,
    }
}

texts! {
    /// 「预设」页的字（`presets.rs`，LM-29）。
    #[derive(Debug, Clone)]
    pub struct PresetTexts {
        /// 在主菜单里的名字和底下那一行。
        pub entry: [String; 2],
        /// 自带的两个预设的名字：全部功能、无功能。
        pub built_in: [String; 2],
        /// 列表里名字后面的记号：自带、默认预设。
        pub marks: [String; 2],
        /// 列表的按键提示。
        pub hints: Vec<[String; 2]>,
        /// 预设的窗的按键提示；`h` `l` 换列那一条排成两列时才插在第二条（`column_hint`）。
        pub window_hints: Vec<[String; 2]>,
        /// 见上：换列。
        pub column_hint: [String; 2],
        /// 预设的窗里名字那一行的名字；改名时编辑窗的标题也是它。
        pub name_row: String,
        /// 窗里的几组的名字：内置工具、MCP 服务、技能（已经没有的那一组不写组名，同 Miyu）。
        pub groups: [String; 3],
        /// 一组一项都没有时写的：MCP 服务、技能各一句（内置工具一直有，已经没有的那一组没有项目就不画）。
        pub empty: [String; 2],
        /// 一项后面写的：MCP 页里关着。
        pub mcp_off: String,
        /// 见上：技能页里关着。
        pub skill_off: String,
        /// 见上：已经没有，`Enter` 去掉。
        pub missing: String,
        /// 新建：填名字的窗的标题。
        pub new: String,
        /// 以自带的一个为底新建：窗的标题，`{name}`。
        pub new_from: String,
        /// 名字是空的。
        pub name_empty: String,
        /// 名字和自带的重了。
        pub name_built_in: String,
        /// 名字不合写法，`{max}`。
        pub name_bad: String,
        /// 已经有这个名字的预设。
        pub name_taken: String,
        /// 建好了，`{name}`。
        pub created: String,
        /// 改了一项、存成了。
        pub saved: String,
        /// 自带的不能删。
        pub built_in_fixed: String,
        /// 删除先问的：标题（`{name}`）、一句、两个按钮。
        pub delete_ask: [String; 4],
        /// 删掉了，`{name}`。
        pub deleted: String,
    }
}

texts! {
    /// 「从 Miyu 导入」的字（`import.rs`）。
    #[derive(Debug, Clone)]
    pub struct ImportTexts {
        /// 勾选窗的标题。
        pub title: String,
        /// 没写地址、照目录推的那一家，地址那一格写的。
        pub by_catalog: String,
        /// key 的几种：没写、取到了、照环境变量（`{name}`）、密钥里没有（`{name}`）、密钥读不了（`{why}`）。
        pub key_none: String,
        /// 见上。
        pub key_found: String,
        /// 见上。
        pub key_env: String,
        /// 见上。
        pub key_missing: String,
        /// 见上。
        pub key_unread: String,
        /// LiteMiyu 已经有同名的。
        pub taken: String,
        /// 有 `{n}` 项写得不对、没抄。
        pub dropped: String,
        /// 读不了的原因：`denied`、`too_big`、`not_utf8`、`broken`、`other`。
        pub unread: BTreeMap<String, String>,
        /// 旧版的数据根。
        pub old_layout: String,
        /// 没有配置：`{path}`。
        pub no_config: String,
        /// 配置读不了：`{why}`。
        pub unreadable: String,
        /// 配置里没有供应商。
        pub empty: String,
        /// 写坏了、一家都没读出来：`{line}`、`{column}`。
        pub broken: String,
        /// 写坏了、读出了前面的几家：`{line}`、`{column}`。
        pub partly_broken: String,
        /// 认不出的几家：`{list}`。
        pub skipped: String,
        /// 认不出的一家：`{id}`、`{why}`（LM-9补：中文全角括号，英文半角括号加空格）。
        pub skipped_item: String,
        /// 一家能导的都没有：`{list}`。
        pub nothing: String,
        /// 认不出的原因：编号不合写法。
        pub bad_name: String,
        /// 认不出的原因：没写地址，目录里也认不出。
        pub no_url: String,
        /// 一家都没勾。
        pub none_ticked: String,
        /// 已经有同名的：`{names}`。
        pub clash: String,
        /// 问同名的三个按钮：覆盖、跳过、取消。
        pub clash_buttons: [String; 3],
        /// 取消了。
        pub cancelled: String,
        /// 选的都已经有了、都跳过了。
        pub all_taken: String,
        /// 导好了：`{n}`、`{names}`。
        pub done: String,
        /// 接在后面：跳过的已有的 `{names}`。
        pub passed: String,
    }
}

texts! {
    /// 试一家通不通的字（`probe.rs`）：`{name}` 是这一家的名字。
    #[derive(Debug, Clone)]
    pub struct ProbeTexts {
        /// 列不出模型、拿 `{model}` 发了一句也没成：`{n}` 是原话。
        pub request: String,
        /// 认证错：`{n}` 是状态码。
        pub auth: String,
        /// 404。
        pub not_found: String,
        /// 别的状态码：`{n}`。
        pub status: String,
        /// 超时：`{n}` 秒。
        pub timeout: String,
        /// 连不上：`{n}` 是原话。
        pub connect: String,
        /// 回的不是模型列表。
        pub garbled: String,
        /// 回到一半断了。
        pub cut: String,
        /// 地址写得不对。
        pub bad_url: String,
        /// key 写不进请求头。
        pub bad_key: String,
        /// 推不出这一家、key 取不到：`{n}` 是原话。
        pub config: String,
        /// 别的：`{n}` 是原话。
        pub other: String,
    }
}

texts! {
    /// 通用页的「提示词」那一行：名字，和写在右边的这份文件是什么样的。
    #[derive(Debug, Clone)]
    pub struct PromptTexts {
        /// 名字。
        pub name: String,
        /// 说明：光标停在这一行时写在列表下面（LM-38）。
        pub description: String,
        /// 还没有这份文件：用出厂的那一段。
        pub shipped: String,
        /// 自己写过。
        pub custom: String,
        /// 空文件：什么都不加。
        pub blank: String,
        /// 编辑器退出以后状态行那一句。
        pub saved: String,
    }
}

texts! {
    /// 勾选窗的字。
    #[derive(Debug, Clone)]
    pub struct PoolTexts {
        /// 勾选窗的按键提示（从 Miyu 导入）。
        pub ticks_hints: Vec<[String; 2]>,
    }
}

texts! {
    /// 按键提示：一个键一个说明。
    #[derive(Debug, Clone)]
    pub struct KeyHints {
        /// 主菜单。
        pub menu: Vec<[String; 2]>,
        /// 照清单画的一页。
        pub page: Vec<[String; 2]>,
        /// 选择窗。
        pub choose: Vec<[String; 2]>,
        /// 编辑窗。
        pub edit: Vec<[String; 2]>,
    }
}

impl Texts {
    /// 读 `language` 那一种（不认识的照英文）。
    ///
    /// # Errors
    ///
    /// 字表写错了（安装坏了，测试守着出厂的两份）。
    pub fn load(language: &str) -> Result<Texts, String> {
        let text = TEXTS
            .iter()
            .find(|(code, _)| *code == language)
            .or_else(|| TEXTS.iter().find(|(code, _)| *code == "en"))
            .map_or("", |(_, text)| text);
        crate::config::read::parse("settings", text)
            .map_err(|error| format!("settings texts: {error}"))
    }
}

/// 照 `fields` 换掉 `{名字}`。
pub fn fill(template: &str, fields: &[(&str, &str)]) -> String {
    fields
        .iter()
        .fold(template.to_string(), |text, (name, value)| {
            text.replace(&format!("{{{name}}}"), value)
        })
}

#[cfg(test)]
mod tests {
    use super::{Texts, fill};

    #[test]
    fn both_shipped_languages_read() {
        for language in ["zh", "en"] {
            let texts = Texts::load(language).expect("出厂的读得进");
            for origin in ["default", "system", "env"] {
                assert!(texts.origins.contains_key(origin), "{language} {origin}");
            }
            for applies in ["now", "next_turn", "new_session", "head_start"] {
                assert!(texts.applies.contains_key(applies), "{language} {applies}");
            }
        }
        assert_eq!(Texts::load("ja").expect("照英文").title, "Settings");
    }

    #[test]
    fn fields_are_filled_by_name() {
        assert_eq!(fill("{a} 和 {b}", &[("a", "1"), ("b", "2")]), "1 和 2");
    }
}
