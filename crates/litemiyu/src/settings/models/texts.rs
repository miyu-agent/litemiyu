//! 「供应商和模型」那一页的字（Miyu 终端 `text/<语言>.json` 的 `settings` 里这一页的那几格，蓝图「配置页」第 26 条）：照 Miyu
//! 原文。语义模型、价格那几格随砍掉的功能没了；`catalog` 多一格「从 Miyu 导入…」，`fields`、`hints` 多「最大输出」。

use std::collections::BTreeMap;

use crate::config::texts;

texts! {
    /// 这一页的字。
    #[derive(Debug, Clone)]
    pub struct ModelTexts {
        /// 主菜单里这一页的名字。
        pub entry: String,
        /// 主菜单里这一页底下那一行。
        pub entry_note: String,
        /// 两个分页。
        pub pages: [String; 2],
        /// 几栏的栏头：供应商、组织、模型、用途。
        pub cols: [String; 4],
        /// 组织栏的「全部」「其他」。
        pub orgs: [String; 2],
        /// 两种用途。
        pub uses: [String; 2],
        /// 两种用途的一句说明。
        pub use_notes: [String; 2],
        /// 默认模型页右边的名字：当前、窗口、输入、用途。
        pub detail: [String; 4],
        /// 能看图的「图」、自己加的「自定义」。
        pub tags: [String; 2],
        /// 引用的东西不在了。
        pub gone: String,
        /// 选项值的名字。
        pub options: BTreeMap<String, String>,
        /// 编辑窗里一项的名字。
        pub fields: BTreeMap<String, String>,
        /// 编辑窗里没写时暗着写的。
        pub hints: BTreeMap<String, String>,
        /// 资料从哪来。
        pub sources: BTreeMap<String, String>,
        /// 编辑窗的标题：加供应商、改供应商、加模型。
        pub titles: [String; 3],
        /// 编辑窗的两个按钮。
        pub buttons: [String; 2],
        /// 两个按钮的快捷键。
        pub button_keys: [String; 2],
        /// 悬浮窗的按键提示：编辑窗、正在改一项、选的窗、问一句。
        pub popup_hints: [Vec<[String; 2]>; 4],
        /// 选模型的窗：视觉的副标题、「/ 搜索」。
        pub pick: [String; 2],
        /// 目录窗：更多供应商、自定义、正在读取、无匹配、从 Miyu 导入。
        pub catalog: [String; 5],
        /// 删之前问的：两种标题、谁在用、按钮、连接的标点。
        pub confirm: [String; 5],
        /// 状态行的话。
        pub status: BTreeMap<String, String>,
        /// 没取到模型目录的原因（LM-9补）：`connect`、`timeout`、`status`、`too_large`、`cut`、`not_catalog`、`write`、`other`，
        /// `{n}` 是原话最里面那一层、状态码或上限。
        pub catalog_why: BTreeMap<String, String>,
        /// 存不成的原因。
        pub errors: BTreeMap<String, String>,
        /// 一栏空着时写的：无项目、无匹配。
        pub empty: [String; 2],
        /// 这一页的按键提示：主菜单、供应商、默认模型、换页那一行、在筛。
        pub key_hints: [Vec<[String; 2]>; 5],
    }
}

impl ModelTexts {
    /// 编辑窗里一项的名字。
    pub fn field<'a>(&'a self, name: &'a str) -> &'a str {
        self.fields.get(name).map_or(name, String::as_str)
    }

    /// 选项值的名字。
    pub fn option<'a>(&'a self, value: &'a str) -> &'a str {
        self.options.get(value).map_or(value, String::as_str)
    }

    /// 资料从哪来的名字（`config:system` 照 `system`）。
    pub fn source<'a>(&'a self, from: &'a str) -> &'a str {
        let key = from.strip_prefix("config:").unwrap_or(from);
        self.sources.get(key).map_or(key, String::as_str)
    }

    /// 状态行的一句。
    pub fn status(&self, name: &str) -> String {
        self.status
            .get(name)
            .cloned()
            .unwrap_or_else(|| name.to_string())
    }

    /// 状态行里带个数的一句（LM-9补）：`n` 是 1 的照 `<name>_one`（英文的单数），没有那一句的照 `name`；`{n}` 换成个数。
    pub fn counted(&self, name: &str, n: usize) -> String {
        let one = format!("{name}_one");
        let key = if n == 1 && self.status.contains_key(&one) {
            one.as_str()
        } else {
            name
        };
        self.status(key).replace("{n}", &n.to_string())
    }

    /// 存不成的原因。
    pub fn error(&self, name: &str) -> String {
        self.errors
            .get(name)
            .cloned()
            .unwrap_or_else(|| name.to_string())
    }

    /// 没写时暗着写的。
    pub fn hint(&self, name: &str) -> Option<&str> {
        self.hints.get(name).map(String::as_str)
    }
}
