//! 配置界面画什么（Miyu `config.schema`，`miyu-endpoint/src/config/methods.rs` 的 `schema`）：几页、几组、一项项，名字和说明
//! 照界面语言。
//!
//! - 项照登记的清单（`crate::configured::items`），先后照登记的；一项是什么类型、在哪一组、用什么控件都照它的声明。
//! - 页照 Miyu 终端主菜单的先后（通用、外观、供应商和模型）：照 Miyu 声明的页和组合过来（[`PAGES`]），一项的 `ui.page`
//!   换成这里的页。Miyu 的「隐私与安全」「高级」两页在这里只有一项、两项，并进了通用页（2026-11-24 项目主人定）。
//! - 登记了、界面不画的照 [`HIDDEN`]：引擎里还没人读的不许出现在界面上，每一项写着为什么。
//! - 字编进程序（`resources/settings/<语言>.json`，中、英两份），只读选中的那一种。

use lm_kernel::config::words::check;
use lm_kernel::config::{ConfigWords, Item, ItemWords};

/// 一页。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    /// 编号：`general`、`appearance`、`models`。
    pub id: &'static str,
    /// 名字。
    pub name: String,
}

/// 一组。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    /// 编号。
    pub id: &'static str,
    /// 名字。
    pub name: String,
    /// 在哪一页。
    pub page: &'static str,
}

/// 画出来的一项：声明（`ui.page` 换成了这里的页）和字。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shown {
    /// 声明。
    pub item: Item,
    /// 名字、说明、选项名。
    pub words: ItemWords,
}

/// 界面画的全部。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Schema {
    /// 页，照 [`ORDER`]。
    pub pages: Vec<Page>,
    /// 组，照 [`PAGES`] 的先后。
    pub groups: Vec<Group>,
    /// 项，照登记的先后。
    pub items: Vec<Shown>,
}

impl Schema {
    /// 键（可能带占位）是 `key` 的那一项。
    pub fn item(&self, key: &str) -> Option<&Shown> {
        self.items.iter().find(|shown| shown.item.key == key)
    }
}

/// 页的先后（照 Miyu 终端的主菜单：通用、外观、供应商和模型）。Miyu 另有「隐私与安全」「高级」两页：LiteMiyu 这两页
/// 一共只有三项，单列一页太空，并进通用页（2026-11-24 项目主人定），画在通用页的最后两组。
pub const ORDER: [&str; 3] = ["general", "appearance", "models"];

/// Miyu 声明的页、组 → 这里的页；组照这个先后画（通用的几组照 Miyu 终端通用页的先后：显示、新会话、会话、终端界面、
/// 时间线、交互）。（LM-2 砍了 `web_search`，`net` 的搜索来源、几家的 key 两组跟着拿掉。）Miyu「隐私与安全」页的「权限」组
/// LiteMiyu 只有确认超时一项（Miyu 施工 CD-1；那一页依赖权限级别、沙盒的项 LiteMiyu 没有，不搬），Miyu「高级」页的「压缩」组
/// 只有两项：这两组画在通用页的最后（2026-11-24 项目主人定）。
pub const PAGES: [(&str, &str, &str); 12] = [
    ("general", "display", "general"),
    ("general", "new_session", "general"),
    ("general", "sessions", "general"),
    ("general", "tui", "general"),
    ("general", "timeline", "general"),
    ("general", "keys", "general"),
    ("appearance", "theme", "appearance"),
    ("appearance", "icons", "appearance"),
    ("models", "providers", "models"),
    ("models", "uses", "models"),
    ("privacy", "permission", "general"),
    ("advanced", "compaction", "general"),
];

/// 登记了、界面不画的项，和为什么。
pub const HIDDEN: [(&str, &str); 20] = [
    (
        "features.<name>",
        "技能、MCP 的全局开关（L5 五）：设置页的技能、MCP 两页另画（Miyu 的 `feature.set`），界面线接",
    ),
    (
        "presets.<name>.unlisted",
        "预设（LM-29）：设置页的「预设」一页另画（主程序 `settings/presets.rs`），不照清单画",
    ),
    (
        "presets.<name>.features.<name>",
        "预设（LM-29）：设置页的「预设」一页另画（主程序 `settings/presets.rs`），不照清单画",
    ),
    (
        "providers.<id>.models.<model>.window",
        "画在「供应商和模型」的模型窗（主程序 `settings/models/forms.rs`），不照清单画",
    ),
    (
        "providers.<id>.models.<model>.catalog",
        "Miyu 的模型窗也不画（手写指定照目录里的哪一个），写在 config.toml 里",
    ),
    (
        "providers.<id>.models.<model>.max_output",
        "画在「供应商和模型」的模型窗（主程序 `settings/models/forms.rs`），不照清单画",
    ),
    (
        "providers.<id>.models.<model>.inputs",
        "画在「供应商和模型」的模型窗（主程序 `settings/models/forms.rs`），不照清单画",
    ),
    (
        "providers.<id>.models.<model>.tools",
        "只给 Miyu 的 model.list 看，不进请求",
    ),
    (
        "providers.<id>.models.<model>.reasoning",
        "Miyu 的模型窗也不画（几档照目录），写在 config.toml 里",
    ),
    (
        "providers.<id>.models.<model>.effort",
        "画在「供应商和模型」的模型窗（主程序 `settings/models/forms.rs`），不照清单画",
    ),
    (
        "providers.<id>.models.<model>.temperature",
        "画在「供应商和模型」的模型窗（主程序 `settings/models/forms.rs`），不照清单画",
    ),
    (
        "providers.<id>.models.<model>.price_multiplier",
        "手写的价格、倍率只写在 config.toml 里（核心线第二趟：会话花费，主人 11-14 只要底栏的花费），界面不画",
    ),
    (
        "providers.<id>.models.<model>.price.input",
        "手写的价格、倍率只写在 config.toml 里（核心线第二趟：会话花费，主人 11-14 只要底栏的花费），界面不画",
    ),
    (
        "providers.<id>.models.<model>.price.output",
        "手写的价格、倍率只写在 config.toml 里（核心线第二趟：会话花费，主人 11-14 只要底栏的花费），界面不画",
    ),
    (
        "providers.<id>.models.<model>.price.cache_read",
        "手写的价格、倍率只写在 config.toml 里（核心线第二趟：会话花费，主人 11-14 只要底栏的花费），界面不画",
    ),
    (
        "providers.<id>.models.<model>.price.cache_write",
        "手写的价格、倍率只写在 config.toml 里（核心线第二趟：会话花费，主人 11-14 只要底栏的花费），界面不画",
    ),
    (
        "providers.<id>.models.<model>.price.currency",
        "手写的价格、倍率只写在 config.toml 里（核心线第二趟：会话花费，主人 11-14 只要底栏的花费），界面不画",
    ),
    (
        "models.catalog.update",
        "取模型目录（LM-9）的开关、地址、间隔只写在 config.toml 里，界面不画",
    ),
    (
        "models.catalog.url",
        "取模型目录（LM-9）的开关、地址、间隔只写在 config.toml 里，界面不画",
    ),
    (
        "models.catalog.every",
        "取模型目录（LM-9）的开关、地址、间隔只写在 config.toml 里，界面不画",
    ),
];

/// 编进程序的字：语言、原文。
const WORDS: [(&str, &str); 2] = [
    ("zh", include_str!("../../../../resources/settings/zh.json")),
    ("en", include_str!("../../../../resources/settings/en.json")),
];

/// 字表读不进来（安装坏了，测试守着出厂的两份）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BadWords(pub String);

impl std::fmt::Display for BadWords {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "settings words unreadable: {}", self.0)
    }
}

impl std::error::Error for BadWords {}

/// 画的是不是这一项：不在 [`HIDDEN`] 里、有这里的页。
fn page_of(item: &Item) -> Option<&'static str> {
    if HIDDEN.iter().any(|(key, _)| *key == item.key) {
        return None;
    }
    PAGES
        .iter()
        .find(|(page, group, _)| *page == item.ui.page && *group == item.ui.group)
        .map(|(_, _, here)| *here)
}

/// 画的那几项：`ui.page` 换成这里的页。
pub fn shown(items: &[Item]) -> Vec<Item> {
    items
        .iter()
        .filter_map(|item| {
            let page = page_of(item)?;
            let mut item = item.clone();
            item.ui.page = page;
            Some(item)
        })
        .collect()
}

/// 读 `language` 那一种的字（不认识的照英文），照 `items` 出要画的全部。
///
/// # Errors
///
/// 字表读不进来，或者和画的项对不上（少了名字、多了项）。
pub fn schema(items: &[Item], language: &str) -> Result<Schema, BadWords> {
    let text = WORDS
        .iter()
        .find(|(code, _)| *code == language)
        .or_else(|| WORDS.iter().find(|(code, _)| *code == "en"))
        .map_or("", |(_, text)| text);
    let mut words: ConfigWords =
        lm_kernel::kernel::json::from_str(text).map_err(|error| BadWords(error.to_string()))?;
    let shown = shown(items);
    if let Some(problem) = check(&shown, &words).into_iter().next() {
        return Err(BadWords(problem));
    }
    let pages = ORDER
        .iter()
        .filter(|page| shown.iter().any(|item| item.ui.page == **page))
        .map(|page| Page {
            id: page,
            name: words.pages.get(*page).cloned().unwrap_or_default(),
        })
        .collect();
    let mut groups: Vec<Group> = Vec::new();
    for item in &shown {
        if !groups.iter().any(|group| group.id == item.ui.group) {
            groups.push(Group {
                id: item.ui.group,
                name: words.groups.get(item.ui.group).cloned().unwrap_or_default(),
                page: item.ui.page,
            });
        }
    }
    // 组照 [`PAGES`] 的先后：登记的先后是代码里的，不是给人看的。
    groups.sort_by_key(|group| PAGES.iter().position(|(_, id, _)| *id == group.id));
    let items = shown
        .into_iter()
        .filter_map(|item| {
            let said = words.items.remove(item.key)?;
            Some(Shown { item, words: said })
        })
        .collect();
    Ok(Schema {
        pages,
        groups,
        items,
    })
}

#[cfg(test)]
mod tests;
