//! MCP 的客户端（L5 六，Miyu `miyu-mcp` 里不带登录、提示词、资源的那一半，`docs/blueprint/mcp.md`）：经一对读写管道（服务进程的
//! 标准输出、标准输入）说 MCP，一行一条 JSON-RPC 2.0。不认识工具目录、不起进程：起进程、登记工具、换结果块在会话那一边。
//!
//! - [`Client::connect`]：认这个服务是哪个时代（`docs/blueprint/mcp.md`「怎么走」第一条）：先发 `server/discover`，答得上的
//!   是新时代（2026-07-28 起，每个请求自己带版本）；答别的错、5 秒没答的是旧时代，退回 `initialize` 握手。认出来的时代
//!   交给调的一方缓存，下次照它直接说。
//! - [`Client::tools`]：`tools/list`，翻页列全。
//! - [`Client::call`]：`tools/call`；这个 future 没等到回应就被丢掉（超时、叫停），发 `notifications/cancelled`。
//! - [`Client::changes`]：服务说工具变了（`notifications/tools/list_changed`）的次数。
//! - [`http::open`]：网址的服务（Streamable HTTP）：交回一对读写给 [`Client::connect`]。
//! - 服务反过来要东西（`sampling/createMessage`、`roots/list`、`elicitation/create`……）一律回「没有这个方法」，`ping`
//!   回空的。
//!
//! 会话那一边（Miyu `miyu-endpoint/src/mcp/` 里会话要的那几块）：[`Servers`] 照配置读一个主会话的服务、照缓存先列上工具；一个
//! 服务调到时才起、闲够了 [`IDLE`] 停（`server.rs`，设计第五节第 11 条）；一件工具（`tool.rs`）、名字和规格（`spec.rs`）、结果
//! 换块（`result.rs`）、目录缓存（`cache.rs`）、怎么起（`launch.rs`）、给模型的说明（`instructions.rs`）。会话回合开始换工具面
//! 在 `crate::session` 的 `actor/mcp.rs`。
//!
//! MCP 的工具没有沙盒可套，照「完全放开」跑：高危命令的认法（`shell` 的命令）管不到它们，服务自己做什么由服务定。
//!
//! Miyu 这里另有的没搬：网址的服务登录（`oauth*`）、提示词（`prompt.rs`）、资源
//! （`resource.rs`）。

mod cache;
mod client;
mod content;
mod error;
pub mod http;
mod instructions;
mod launch;
mod link;
mod progress;
mod result;
mod server;
mod servers;
mod spec;
#[cfg(any(test, feature = "testkit"))]
pub mod testkit;
mod tool;
mod wire;

pub use client::{Client, Era, Hello, Waits};
pub use content::{Annotations, Called, Content, Listed, Tool, Unread};
pub use error::Failed;
pub use progress::Progress;
pub use server::IDLE;
pub use servers::Servers;

/// 新时代我们说的版本，挑的时候从新往旧排。
pub const MODERN: &[&str] = &["2026-07-28"];

/// 旧时代我们认得的版本，握手时报第一个。
pub const LEGACY: &[&str] = &["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];

/// 服务 `server` 上次列到的工具有几件：照缓存目录 `cache` 里记着的（起它的命令、环境变量的写法变了的、没起过的没有）。
/// 只读，配置界面的 MCP 页给人看状态用。
pub fn remembered_tools(
    cache: &std::path::Path,
    server: &lm_kernel::config::mcp::McpServer,
) -> Option<usize> {
    cache::read_at(&cache::path(cache, &server.name), server)
        .map(|remembered| remembered.tools.len())
}
