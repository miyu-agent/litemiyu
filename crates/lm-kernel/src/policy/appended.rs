//! 接在系统提示词尾部的那几段（Miyu `miyu-policy/src/appended.rs`，施工 X-20、X-39）：每段原文照抄（外壳、转义交来的一方做），
//! 照键排，接在整段 system 的最后，记进快照。只插不改别的原文。Miyu 这里放扩展交的和 MCP 服务的说明；LiteMiyu 没有扩展，
//! 只有 MCP 服务的说明（L5 六，键是 `mcp:<服务名>`）。
//!
//! 工具面换了的那一轮（MCP 服务的工具变了）跟着换这几段：[`Snapshot::reappended`] 拿掉原来接的、接上新的，别的原文不动
//! （Miyu 是照资源重拼整份快照；LiteMiyu 的提示词文件改了只给新会话用，不重读）。

use std::collections::BTreeMap;

use crate::policy::ToolEntry;
use crate::policy::compose::system;
use crate::policy::snapshot::Snapshot;

/// 一段最多几个字。
pub const APPEND_MAX: usize = 4000;

impl Snapshot {
    /// 带上那几段：照键排、块间空一行，接在最后（拼 system 的最后一步）；记进快照。没有的 system 一字不变。
    #[must_use]
    pub fn with_appended(mut self, appended: BTreeMap<String, String>) -> Snapshot {
        for text in appended.values() {
            self.system = system(&[&self.system, text]);
        }
        self.appended = appended;
        self
    }

    /// 换上工具面 `tools`、尾部换成 `appended`（L5 六：回合开始 MCP 服务的工具变了）：先照记着的那几段从 system 末尾一段段
    /// 拿掉，再照 [`Snapshot::with_appended`] 接上新的。system 的末尾和记着的对不上的（不该有：尾部只有它们）没有。
    pub fn reappended(
        self,
        tools: Vec<ToolEntry>,
        appended: BTreeMap<String, String>,
    ) -> Option<Snapshot> {
        let mut base = self.system.as_str();
        for text in self.appended.values().rev() {
            let text = text.trim_end();
            if text.is_empty() {
                continue;
            }
            let rest = base.strip_suffix(text)?;
            base = rest.strip_suffix("\n\n").unwrap_or(rest);
        }
        let system = base.to_string();
        let snapshot = Snapshot {
            system,
            appended: BTreeMap::new(),
            ..self
        };
        Some(snapshot.with_tools(tools).with_appended(appended))
    }
}

#[cfg(test)]
mod tests;
