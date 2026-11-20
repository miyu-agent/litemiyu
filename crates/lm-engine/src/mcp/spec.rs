//! 服务列出来的工具换成目录里的工具（L5 六，Miyu `miyu-endpoint/src/mcp/spec.rs`，`mcp.md`「对外的样子」第 1 到 4 条）：名字
//! `mcp__<服务名>__<工具名>`，说明照原文截到 2048 个字符，参数照原样，访问类别照注解，时限照配置。截断后撞名的后一件、参数
//! 读不成的不登记，记一行 `WARN`。
//!
//! 和 Miyu 不一样的：没有给哪种会话（`venues`，LiteMiyu 只有本机这一种，子会话也给）、脚本调工具（`clicall`）；不在这里换目录
//! （会话照 [`crate::mcp::Servers::catalog`] 拼自己那一份）；没登记的不记进状态（给头看的那一套没搬）。

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

use lm_kernel::kernel::raw::RawJson;
use lm_kernel::kernel::tool::Access;

use crate::mcp::server::Server;
use crate::mcp::tool::{McpTool, Texts};
use crate::tool::{Scope, Spec, Tool, mcp_prefix};

/// 工具名最长几个字符：同目录（各家供应商对函数名的限制）。
const NAME_LIMIT: usize = 64;

/// 说明最长几个字符（同 Claude Code）。
const DESCRIPTION_LIMIT: usize = 2048;

/// 服务 `service` 的工具 `tool` 在目录里叫什么：不合 `[A-Za-z0-9_-]` 的字换成 `_`，超过 64 个字符的截到 64 个。
pub(crate) fn name(service: &str, tool: &str) -> String {
    let cleaned: String = tool
        .chars()
        .map(
            |c| match c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                true => c,
                false => '_',
            },
        )
        .collect();
    let mut name = format!("{}{cleaned}", mcp_prefix(service));
    name.truncate(NAME_LIMIT);
    name
}

/// 说明：服务给的原文，超过 [`DESCRIPTION_LIMIT`] 个字符的截掉后面；没给的是空的。
pub(crate) fn description(text: Option<&str>) -> String {
    let text = text.unwrap_or_default();
    match text.char_indices().nth(DESCRIPTION_LIMIT) {
        Some((at, _)) => text[..at].to_string(),
        None => text.to_string(),
    }
}

/// 访问类别：服务说只读的算 `read`，别的一律算 `network`（`mcp.md`「对外的样子」第 3 条）。
pub(crate) fn access(tool: &crate::mcp::Tool) -> Access {
    match tool.annotations.read_only {
        Some(true) => Access::Read,
        _ => Access::Network,
    }
}

/// 服务 `server` 列出来的 `tools` 换成目录里的几件：截断后撞名的后一件、参数读不成的不要，各记一行 `WARN mcp tool skipped`。
pub(crate) fn tools(
    server: &Arc<Server>,
    tools: &[crate::mcp::Tool],
    texts: &Arc<Texts>,
) -> Vec<Arc<dyn Tool>> {
    let service = server.name();
    let mut seen = BTreeSet::new();
    let mut built: Vec<Arc<dyn Tool>> = Vec::new();
    for tool in tools {
        let name = name(service, &tool.name);
        if !seen.insert(name.clone()) {
            continue;
        }
        let Ok(parameters) =
            lm_kernel::kernel::json::from_str::<RawJson>(&tool.input_schema.to_string())
        else {
            continue;
        };
        built.push(Arc::new(McpTool {
            spec: Spec {
                name,
                description: description(tool.description.as_deref()),
                parameters,
                access: access(tool),
                role: None,
                scope: Scope::Any,
            },
            original: tool.name.clone(),
            title: tool
                .title
                .as_deref()
                .map(str::trim)
                .filter(|title| !title.is_empty())
                .map(str::to_string),
            timeout: Duration::from_millis(server.timeout_ms()),
            server: Arc::clone(server),
            texts: Arc::clone(texts),
        }));
    }
    built
}

#[cfg(test)]
mod tests;
