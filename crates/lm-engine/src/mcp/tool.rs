//! 一件 MCP 工具（L5 六，Miyu `miyu-endpoint/src/mcp/tool.rs`，`mcp.md`「怎么走」第四条）：调用时叫那个服务交出连着的客户端
//! （没起的这时起，`server.rs`），发 `tools/call`，照配置的时限等。起不来的、等着时进程退出了的说暂时不可用；到了时限没回的丢掉
//! 这次调用（客户端发取消），说没在 N 秒内答完；被掐掉的（人停了这一轮）同样丢掉、发取消。服务回 JSON-RPC 的错照它的原话交；
//! 写法不对、要补信息的照工具出了内部错误说。回来的内容换块在 `result.rs`。调工具带进度令牌，服务推的进度一条一行交给执行器。
//!
//! 和 Miyu 不一样的：没有列资源、读资源那两件（资源没搬）；进度照原样一行交（Miyu 套成脚本的进度行 `miyu-progress: `，LiteMiyu
//! 没有脚本工具那一套）；替服务写的几句编在程序里（Miyu 从资源目录读）。

use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};

use lm_kernel::kernel::block::{Block, Text};
use lm_kernel::kernel::event::Said;
use lm_kernel::kernel::template::Template;

use crate::mcp::server::Server;
use crate::mcp::{Failed, result};
use crate::tool::{Call, Done, Progress, Running, Spec, Tool};

/// 替服务写的几句的模板。
#[derive(Clone)]
pub(crate) struct Texts {
    /// 暂时不可用（`core/tool-results/unavailable.txt`），字段 `name`。
    pub(crate) unavailable: String,
    /// 没在时限里答完（`core/tool-results/timed-out.txt`），字段 `name`、`seconds`。
    pub(crate) timed_out: String,
    /// 内部错误（`core/tool-results/crashed.txt`），字段 `name`。
    pub(crate) crashed: String,
    /// 不给她看的一样东西（`core/mcp/not-shown.txt`），字段 `mime`、`bytes`。
    pub(crate) not_shown: String,
    /// 文字截掉了（`core/mcp/cut.txt`），字段 `cut`。
    pub(crate) cut: String,
}

impl Texts {
    /// 编在程序里的那几份。
    pub(crate) fn shipped() -> Texts {
        Texts {
            unavailable: include_str!("../../../../resources/core/tool-results/unavailable.txt")
                .to_string(),
            timed_out: include_str!("../../../../resources/core/tool-results/timed-out.txt")
                .to_string(),
            crashed: include_str!("../../../../resources/core/tool-results/crashed.txt")
                .to_string(),
            not_shown: include_str!("../../../../resources/core/mcp/not-shown.txt").to_string(),
            cut: include_str!("../../../../resources/core/mcp/cut.txt").to_string(),
        }
    }
}

/// 一件 MCP 工具。
pub(crate) struct McpTool {
    /// 目录里的规格：名字是 `mcp__<服务名>__<工具名>`。
    pub(crate) spec: Spec,
    /// 服务给的名字，原样，发 `tools/call` 用它。
    pub(crate) original: String,
    /// 服务给的给人看的名字（`title`），没写的、空白的没有。
    pub(crate) title: Option<String>,
    /// 等多久。
    pub(crate) timeout: Duration,
    /// 那个服务。
    pub(crate) server: Arc<Server>,
    /// 替它写的几句。
    pub(crate) texts: Arc<Texts>,
}

impl McpTool {
    /// 暂时不可用：说法同执行器替目录里没有的工具写的那一句。
    fn unavailable(&self) -> Done {
        let name = self.spec.name.as_str();
        Done {
            human: Some(Said::new("core/tool-results/unavailable").with("name", name)),
            ..failed(render(&self.texts.unavailable, &[("name", name)]))
        }
    }

    /// 没在时限里答完：秒数往上取整。
    fn timed_out(&self) -> Done {
        let name = self.spec.name.as_str();
        let seconds = self.timeout.as_millis().div_ceil(1000).to_string();
        Done {
            human: Some(
                Said::new("core/tool-results/timed-out")
                    .with("name", name)
                    .with("seconds", &seconds),
            ),
            ..failed(render(
                &self.texts.timed_out,
                &[("name", name), ("seconds", &seconds)],
            ))
        }
    }

    /// 服务答得不对（写法不对、要补信息）：照工具出了内部错误说。
    fn crashed(&self) -> Done {
        let name = self.spec.name.as_str();
        Done {
            human: Some(Said::new("core/tool-results/crashed").with("name", name)),
            ..failed(render(&self.texts.crashed, &[("name", name)]))
        }
    }
}

impl Tool for McpTool {
    fn spec(&self) -> &Spec {
        &self.spec
    }

    /// 服务给的 `title` 不分语言：记在 `en` 下，别的语言没写的照它（施工 9-8 六补）。
    fn display_name(&self) -> BTreeMap<String, String> {
        self.title
            .iter()
            .map(|title| ("en".to_string(), title.clone()))
            .collect()
    }

    fn run(&self, call: Call, progress: Progress) -> Running<'_> {
        Box::pin(async move {
            // 在用着：闲着的计时不算这一段（`server.rs`）。
            let _using = self.server.using();
            let arguments: Value =
                lm_kernel::kernel::json::from_str(&call.args).unwrap_or_else(|_| json!({}));
            // 起服务、等它答都算在时限里；到点、被掐掉，这个调用的 future 都被丢掉：客户端那一刻发 `notifications/cancelled`。
            let asked = async {
                let client = self.server.client().await?;
                let report =
                    |step: crate::mcp::Progress| progress.push(format!("{}\n", step.said()));
                client
                    .call_reporting(&self.original, arguments, report)
                    .await
            };
            match tokio::time::timeout(self.timeout, asked).await {
                Err(_) => self.timed_out(),
                Ok(Ok(called)) => result::done(called, &self.texts),
                Ok(Err(Failed::Closed)) => self.unavailable(),
                Ok(Err(Failed::Rpc { message, .. })) => failed(message),
                Ok(Err(_)) => self.crashed(),
            }
        })
    }
}

/// 照模板 `template` 换进字段 `fields`；模板坏了的交回空的。
pub(crate) fn render(template: &str, fields: &[(&str, &str)]) -> String {
    let fields: BTreeMap<&str, &str> = fields.iter().copied().collect();
    Template::parse(template)
        .ok()
        .and_then(|template| template.render(&fields).ok())
        .unwrap_or_default()
}

/// 出错，交回一段字。
fn failed(text: String) -> Done {
    Done {
        error: true,
        blocks: vec![Block::Text(Text { text })],
        ..Done::ok("")
    }
}
