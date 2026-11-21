//! 预设（`docs/00-设计.md` 第六节第 14 条，LM-29）：一张带名字的开关名单，管内置工具、MCP 服务、技能三样。自带「全部功能」
//! （[`FULL`]）、「无功能」（[`BARE`]）两个，不写在配置里、改不了；自己定义的写在 `config.toml` 的 `[presets.<名字>]`：
//!
//! ```toml
//! [presets.web]
//! unlisted = "off"            # 名单里没写的开不开：on、off
//!
//! [presets.web.features]      # 编号 = 开不开
//! shell = true                # 内置工具：工具名
//! mcp-fetch = true            # MCP 服务：mcp-<服务名>
//! skill-notes = true          # 技能：skill-<技能名>
//! ```
//!
//! 照 Miyu 的预设文件（`resources/presets/*.toml`、蓝图 `presets.md`「格式」）：`unlisted` 和 `[features]` 同一个意思，编号的
//! 写法也一样。和 Miyu 不同的：
//!
//! - 没有单独的预设文件、三层叠、软件包、功能分组、`[tools]`、`[limits]`、人格、图标、说明；预设只是配置里的几项，
//!   配置界面照平常的改法改（`crate::settings`）。
//! - 一个预设在不在照 `unlisted` 写没写：只写了 `[presets.<名字>]` 一个表头的不算。
//! - 全局开关（`features.<编号>`，[`crate::skills::switches`]）和预设是「都开着才开」（[`narrowed`]）；Miyu 是预设明写的盖过
//!   全局开关。LiteMiyu 的全局开关就是技能页、MCP 页上的那个开关，关掉的在哪个预设里都没有。
//! - 钉在会话上，中途不换（Miyu 能换，换一次断一次缓存）；预设后来改了、删了，开着的会话照造它时记下的那一份
//!   （`lm_kernel::policy::PresetPin`，在策略快照里）。
//!
//! 名单里写了现在没有的工具、服务、技能不报错：没有的东西开着也没有。

use std::collections::BTreeMap;

use lm_kernel::config::{Applies, Control, Item, Kind, Layer, Ui, Value, Values, key};
use lm_kernel::policy::PresetPin;

/// 「全部功能」的名字（`--preset full`、`/new full`、`sessions.default = "full"`）。
pub const FULL: &str = "full";

/// 「无功能」的名字。
pub const BARE: &str = "bare";

/// 名单里没写的开不开那一项的样子。
pub const UNLISTED: &str = "presets.<name>.unlisted";

/// 名单里一个编号那一项的样子。
pub const FEATURE: &str = "presets.<name>.features.<name>";

/// `unlisted` 能写的两个。
const ON_OFF: &[&str] = &["on", "off"];

/// 内置工具的名字，照配置界面「预设」页列的先后（文件、命令、提问、子代理、定时、会话）。就是基础系统那一组和循环的
/// `schedule_wakeup`；测试守着和工具目录对得上（`the_built_in_tools_are_the_shipped_catalog`）。配置界面照它列，不为列名字把
/// 每件工具的说明都读进来。
pub const TOOLS: [&str; 12] = [
    "read",
    "write",
    "edit",
    "shell",
    "jobs",
    "ask_user",
    "subagent",
    "schedule_wakeup",
    "history",
    "sessions",
    "send_message",
    "session_usage",
];

/// 预设名最多几个字符。
pub const NAME_CHARS: usize = 32;

/// 登记的两项：键里有人起的名字，`settings!` 写不了，手写。配置界面不照清单画，「预设」那一页另画。
pub const ITEMS: &[Item] = &[
    Item {
        key: UNLISTED,
        kind: Kind::Option(ON_OFF),
        default: None,
        layers: &[Layer::System],
        tighten: None,
        env: None,
        applies: Applies::NewSession,
        ui: HIDDEN,
    },
    Item {
        key: FEATURE,
        kind: Kind::Bool,
        default: None,
        layers: &[Layer::System],
        tighten: None,
        env: None,
        applies: Applies::NewSession,
        ui: HIDDEN,
    },
];

/// 这两项的界面提示：不照清单画。
const HIDDEN: Ui = Ui {
    page: "general",
    group: "presets",
    common: false,
    control: Control::Toggle,
    hidden: true,
};

/// 新会话用哪个预设。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Chosen {
    /// 全部功能。
    Full,
    /// 无功能：一件工具都不给，回合开始什么都不注入。
    Bare,
    /// 自己定义的一个：这一刻配置里的名单。
    Custom(PresetPin),
}

impl Chosen {
    /// 快照 `snapshot` 钉着的是哪个：记了名单的是自己定义的，`no_tools` 的是无功能，别的（连同 LM-29 以前造的）是全部功能。
    pub fn of(snapshot: &lm_kernel::policy::Snapshot) -> Chosen {
        match (&snapshot.preset, snapshot.no_tools) {
            (Some(pin), _) => Chosen::Custom(pin.clone()),
            (None, true) => Chosen::Bare,
            (None, false) => Chosen::Full,
        }
    }

    /// 自己定义的那一张名单；自带的两个没有。
    pub fn pin(&self) -> Option<&PresetPin> {
        match self {
            Chosen::Custom(pin) => Some(pin),
            Chosen::Full | Chosen::Bare => None,
        }
    }

    /// 它的名字：自带的是 [`FULL`]、[`BARE`]。
    pub fn name(&self) -> &str {
        match self {
            Chosen::Full => FULL,
            Chosen::Bare => BARE,
            Chosen::Custom(pin) => &pin.name,
        }
    }
}

/// 预设名合不合写法：1 到 [`NAME_CHARS`] 个字符，没有空白、控制字符和 `.`，不以 `-` 开头，不是自带的两个名字。
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.chars().count() <= NAME_CHARS
        && !name.starts_with('-')
        && !name
            .chars()
            .any(|c| c == '.' || c.is_whitespace() || c.is_control())
        && name != FULL
        && name != BARE
}

/// 配置 `values` 里自己定义的预设，照名字排。写了 `unlisted` 的才算；名字不合写法的不算。
pub fn custom(values: &Values) -> Vec<PresetPin> {
    let mut found: BTreeMap<String, (Option<bool>, BTreeMap<String, bool>)> = BTreeMap::new();
    for real in values.keys() {
        if !real.starts_with("presets.") {
            continue;
        }
        let Some(segments) = key::split(real) else {
            continue;
        };
        match (key::fit(UNLISTED, &segments), values.get(real)) {
            (key::Fit::Yes(mut names), Some(Value::Text(text))) => {
                if let Some(name) = names.pop() {
                    found.entry(name).or_default().0 = Some(text.as_ref() == "on");
                }
                continue;
            }
            (key::Fit::Yes(_), _) => continue,
            _ => {}
        }
        if let (key::Fit::Yes(names), Some(Value::Bool(on))) =
            (key::fit(FEATURE, &segments), values.get(real))
            && let [name, feature] = names.as_slice()
        {
            found
                .entry(name.clone())
                .or_default()
                .1
                .insert(feature.clone(), *on);
        }
    }
    found
        .into_iter()
        .filter(|(name, _)| valid_name(name))
        .filter_map(|(name, (unlisted, features))| {
            Some(PresetPin {
                name,
                unlisted: unlisted?,
                features,
            })
        })
        .collect()
}

/// 配置 `values` 里叫 `name` 的预设：自带的两个，或者自己定义的。没有的是空的。
pub fn choose(values: &Values, name: &str) -> Option<Chosen> {
    match name {
        FULL => Some(Chosen::Full),
        BARE => Some(Chosen::Bare),
        _ => custom(values)
            .into_iter()
            .find(|pin| pin.name == name)
            .map(Chosen::Custom),
    }
}

/// 配置 `values` 里的默认预设（`sessions.default`）：没写的是全部功能。交回它和「写的那个名字现在没有」时的那个名字
/// （照全部功能开，主程序提示一次）。
pub fn default_of(values: &Values) -> (Chosen, Option<String>) {
    let name = match values.get(DEFAULT) {
        Some(Value::Text(name)) => name.to_string(),
        _ => FULL.to_string(),
    };
    match choose(values, &name) {
        Some(chosen) => (chosen, None),
        None => (Chosen::Full, Some(name)),
    }
}

/// 配置文件里和预设有关的那几样：主程序列预设（`/preset`、`/new <名字>` 的补全、`--preset`）时照它，不整份读配置。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Listed {
    /// 默认预设的名字（`sessions.default`）：没写的是 [`FULL`]。写的名字现在没有的也原样交回（[`Listed::default_missing`]）。
    pub default: String,
    /// 自己定义的预设，照名字排。
    pub custom: Vec<PresetPin>,
}

impl Listed {
    /// 有没有叫 `name` 的预设：自带的两个一直有。
    pub fn has(&self, name: &str) -> bool {
        name == FULL || name == BARE || self.custom.iter().any(|pin| pin.name == name)
    }

    /// 默认预设写的名字现在没有（照全部功能开）。
    pub fn default_missing(&self) -> bool {
        !self.has(&self.default)
    }

    /// 新会话没另指定时实际用的那个预设的名字：默认预设在的照它，不在的是 [`FULL`]。
    pub fn effective_default(&self) -> &str {
        match self.default_missing() {
            true => FULL,
            false => &self.default,
        }
    }

    /// 全部预设的名字：自带的两个在前，自己定义的照名字排。
    pub fn names(&self) -> impl Iterator<Item = &str> {
        [FULL, BARE]
            .into_iter()
            .chain(self.custom.iter().map(|pin| pin.name.as_str()))
    }
}

/// 照配置的字 `text` 读预设和默认预设：只认这几项，别的项不看；整份读不懂的照没写。
pub fn listed(text: &str) -> Listed {
    use lm_kernel::config::merge::{Layers, merge};
    let items = [ITEMS, crate::settings::declared::SessionDefaults::ITEMS].concat();
    let parsed = lm_kernel::config::parse::parse(&items, Layer::System, text).unwrap_or_default();
    let layers = Layers {
        system: Some(&parsed),
        ..Layers::default()
    };
    let values = merge(&items, &layers, &|_| None).values();
    let default = match values.get(DEFAULT) {
        Some(Value::Text(name)) => name.to_string(),
        _ => FULL.to_string(),
    };
    Listed {
        default,
        custom: custom(&values),
    }
}

/// 读数据在 `paths` 那一处的配置文件里的预设（[`listed`]）：没有文件、读不了的照没写。照原样读字节（同主程序启动时读配置，
/// 不算整份的版本）。
pub fn read(paths: &crate::store::paths::Paths) -> Listed {
    let text = std::fs::read(paths.config_file())
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .unwrap_or_default();
    listed(text.trim_start_matches('\u{feff}'))
}

/// 默认预设那一项的键。
pub const DEFAULT: &str = "sessions.default";

/// 预设 `name` 的 `unlisted` 那一项的真的键。
pub fn unlisted_key(name: &str) -> String {
    key::fill(UNLISTED, &[name])
}

/// 预设 `name` 的名单里编号 `feature` 那一项的真的键。
pub fn feature_key(name: &str, feature: &str) -> String {
    key::fill(FEATURE, &[name, feature])
}

/// 把预设 `pin` 整张写进配置要改的几项（新建、另存成别的名字）。
pub fn written(pin: &PresetPin) -> Vec<crate::settings::Edit> {
    use crate::settings::Edit;
    let unlisted = if pin.unlisted { "on" } else { "off" };
    std::iter::once(Edit::Set(
        unlisted_key(&pin.name),
        Value::Text(unlisted.into()),
    ))
    .chain(
        pin.features
            .iter()
            .map(|(feature, on)| Edit::Set(feature_key(&pin.name, feature), Value::Bool(*on))),
    )
    .collect()
}

/// 从配置里去掉预设 `pin` 要改的几项；`default` 是这时的默认预设，正好是它的连那一项也去掉（回到全部功能）。
pub fn removed(pin: &PresetPin, default: &str) -> Vec<crate::settings::Edit> {
    use crate::settings::Edit;
    let mut edits: Vec<Edit> = pin
        .features
        .keys()
        .map(|feature| Edit::Unset(feature_key(&pin.name, feature)))
        .collect();
    edits.push(Edit::Unset(unlisted_key(&pin.name)));
    if default == pin.name {
        edits.push(Edit::Unset(DEFAULT.to_string()));
    }
    edits
}

/// 全局开关 `switches`（[`crate::skills::switches::of`]）照预设 `pin` 收窄：全局开关和预设都开着的才开。预设里没写的照它的
/// `unlisted`：关着的记成 [`crate::skills::switches::REST`] 那一格，以后新装的服务、新加的技能也关着。
pub fn narrowed(mut switches: BTreeMap<String, bool>, pin: &PresetPin) -> BTreeMap<String, bool> {
    for (feature, on) in &mut switches {
        *on = *on && pin.on(feature);
    }
    for (feature, on) in &pin.features {
        if crate::skills::switches::switchable(feature) {
            switches.entry(feature.clone()).or_insert(*on);
        }
    }
    if !pin.unlisted {
        switches.insert(crate::skills::switches::REST.to_string(), false);
    }
    switches
}

#[cfg(test)]
mod tests;
