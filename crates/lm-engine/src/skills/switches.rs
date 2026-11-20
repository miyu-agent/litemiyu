//! 技能、MCP 服务的全局开关在配置里的那一项（Miyu `miyu-session/src/switches.rs`，施工 X-27，`docs/blueprint/presets.md`
//! 「全局开关」）：`features.<功能编号>`，功能编号是 `skill-<名字>`、`mcp-<服务名>`。没写的算开。
//!
//! 和 Miyu 不同：Miyu 照预设叠，预设没写的才照它；LiteMiyu 没有预设，只看它。Miyu 还认脚本的 `script-`，LiteMiyu 没有脚本工具。
//! 造会话时照这一刻的配置读（[`of`]）。

use std::collections::BTreeMap;

use lm_kernel::config::{Applies, Control, Item, Kind, Layer, Ui, Value, Values, key};

/// 那一项的样子：最后一段是功能编号。
pub const FEATURE: &str = "features.<name>";

/// 认的几类功能编号的开头。
const SWITCHABLE: [&str; 2] = ["skill-", "mcp-"];

/// 登记的一项：最后一段是人起的名字，`settings!` 写不了，手写。
pub const ITEMS: &[Item] = &[Item {
    key: FEATURE,
    kind: Kind::Bool,
    default: None,
    layers: &[Layer::System],
    tighten: None,
    env: None,
    applies: Applies::NextTurn,
    ui: Ui {
        page: "general",
        group: "features",
        common: false,
        control: Control::Toggle,
        hidden: true,
    },
}];

/// 配置 `values` 里写了的全局开关：功能编号到开不开，只留认的那几类。
pub fn of(values: &Values) -> BTreeMap<String, bool> {
    let mut switches = BTreeMap::new();
    for real in values.keys() {
        let Some(Value::Bool(on)) = values.get(real) else {
            continue;
        };
        let Some(segments) = key::split(real) else {
            continue;
        };
        if let key::Fit::Yes(names) = key::fit(FEATURE, &segments) {
            switches.extend(
                names
                    .into_iter()
                    .filter(|feature| SWITCHABLE.iter().any(|kind| feature.starts_with(kind)))
                    .map(|feature| (feature, *on)),
            );
        }
    }
    switches
}

/// 功能 `feature` 的全局开关这时开不开：没写的算开。
pub fn on(switches: &BTreeMap<String, bool>, feature: &str) -> bool {
    switches.get(feature).copied().unwrap_or(true)
}
