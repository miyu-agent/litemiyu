//! 钉在快照里的预设（`docs/00-设计.md` 第六节第 14 条，LM-29；Miyu `miyu-policy/src/preset.rs` 的 `PresetPin`）：一张带名字的
//! 开关名单，管内置工具、MCP 服务、技能三样。造会话时照这一刻配置里的那一张记下，会话中途不换；预设后来改了、删了，
//! 已经开始的会话照记下的这一份走。
//!
//! 编号的写法同 Miyu 的功能编号：内置工具就是工具名（`shell`），MCP 服务是 `mcp-<服务名>`，技能是 `skill-<技能名>`。
//! 一个编号开不开：名单里写了的照写的，没写的照 [`PresetPin::unlisted`]（同 Miyu 预设文件的 `unlisted`）。
//!
//! 自带的两个预设不记这一份：「全部功能」没有它，「无功能」是 [`crate::policy::Snapshot::no_tools`]。以前造的快照里没有，
//! 读成没有；没有的不写，旧快照的字节不变。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// 快照里记的预设。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PresetPin {
    /// 预设的名字：造会话那一刻配置里的名字。
    pub name: String,
    /// 名单里没写的开不开：以后新装的 MCP 服务、新加的技能照它。
    pub unlisted: bool,
    /// 名单：编号到开不开。
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub features: BTreeMap<String, bool>,
}

impl PresetPin {
    /// 编号 `feature` 在这个预设里开不开：写了的照写的，没写的照 `unlisted`。
    pub fn on(&self, feature: &str) -> bool {
        self.features.get(feature).copied().unwrap_or(self.unlisted)
    }
}

#[cfg(test)]
mod tests;
