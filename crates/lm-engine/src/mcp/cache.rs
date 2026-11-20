//! 目录缓存（L5 六，Miyu `miyu-endpoint/src/mcp/cache.rs`，`mcp.md`「怎么走」第一条第 4 条、第三条第 1 条）：列完的写进缓存
//! 目录的 `mcp/<服务名>.json`（设计第六节，Miyu 在 `state/mcp/`），记着起它的命令、环境变量的写法、认出来的时代、每件工具的
//! 原文、给模型的说明；一样的不写。造会话时命令和环境变量的写法没变的照它先列上工具（设计第五节第 11 条：调到时才起），连的
//! 时候照记着的时代直接说。派生的数据：读不成、写不成的记一行 `WARN`，等服务自己列。Miyu 另记的「声明了资源」、卸包时清掉
//! 缓存（`forget`）没搬。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use lm_kernel::config::mcp::{EnvValue, McpServer, Reach};

use crate::mcp::{Era, Tool};
use crate::store::generated;

/// 缓存的写法。
#[derive(Debug, PartialEq, Serialize, Deserialize)]
struct Cached {
    /// 起它的命令，原样。
    command: Vec<String>,
    /// 环境变量的写法：`名字=字` 或者 `名字={ secret = … }` 这样的引用，不是取出来的值。
    env: Vec<String>,
    /// 认出来的时代和版本。
    era: Era,
    /// 每件工具的原文。
    tools: Vec<Value>,
    /// 服务给模型的说明（施工 X-39）：没给的不写，以前的缓存没有这一格照样认得。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    instructions: Option<String>,
}

/// 缓存里记着的（施工 X-39 起多了说明）。
#[derive(Debug, PartialEq)]
pub(super) struct Remembered {
    /// 认出来的时代和版本。
    pub(super) era: Era,
    /// 读得成的工具。
    pub(super) tools: Vec<Tool>,
    /// 服务给模型的说明，没给的没有。
    pub(super) instructions: Option<String>,
}

/// 服务 `service` 的缓存在哪：缓存目录 `cache` 下的 `mcp/<服务名>.json`。
pub(super) fn path(cache: &Path, service: &str) -> PathBuf {
    cache.join("mcp").join(format!("{service}.json"))
}

/// 缓存照什么认这个服务（施工 X-3 下、X-32 上）：命令的是命令和环境变量的写法（引用的照引用写，不取值）；网址的是网址和请求头的
/// 名字（有令牌的另记一个 `Authorization`），不认值：换了令牌，目录照旧能用。记在缓存文件原来的两格里，以前的缓存照旧认得。
fn key(server: &McpServer) -> (Vec<String>, Vec<String>) {
    match &server.reach {
        Reach::Command { command, env } => (
            command.clone(),
            env.iter()
                .map(|(name, value)| match value {
                    EnvValue::Text(text) => format!("{name}={text}"),
                    EnvValue::Reference(reference) => format!("{name}={}", reference.toml()),
                })
                .collect(),
        ),
        Reach::Url {
            url,
            bearer,
            headers,
        } => (
            vec![url.clone()],
            bearer
                .as_ref()
                .map(|_| "Authorization".to_string())
                .into_iter()
                .chain(headers.iter().map(|(name, _)| name.clone()))
                .collect(),
        ),
    }
}

/// 记下服务 `server` 这一次认出来的时代 `era`、列出来的 `tools`、给模型的说明 `instructions`（施工 X-39），写在 `path`。
pub(super) fn write(
    path: &Path,
    server: &McpServer,
    era: &Era,
    (tools, instructions): (&[Tool], Option<&str>),
) {
    let (command, env) = key(server);
    let cached = Cached {
        command,
        env,
        era: era.clone(),
        tools: tools.iter().map(|tool| tool.raw.clone()).collect(),
        instructions: instructions.map(str::to_string),
    };
    let written = serde_json::to_vec_pretty(&cached)
        .map_err(std::io::Error::other)
        .and_then(|bytes| generated::write(path, &bytes));
    drop(written);
}

/// 服务 `server` 的缓存（在 `path`）：命令、环境变量的写法和它记着的一样才算，交回时代、工具（读不成的那一件不要）、说明。
/// 没有的、对不上的没有；读不成的记一行。
pub(super) fn read_at(path: &Path, server: &McpServer) -> Option<Remembered> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
        Err(_) => {
            return None;
        }
    };
    let cached = match lm_kernel::kernel::json::from_slice::<Cached>(&bytes) {
        Ok(cached) => cached,
        Err(_) => {
            return None;
        }
    };
    if (cached.command.clone(), cached.env.clone()) != key(server) {
        return None;
    }
    let tools = cached
        .tools
        .iter()
        .filter_map(|raw| Tool::read(raw).ok())
        .collect();
    Some(Remembered {
        era: cached.era,
        tools,
        instructions: cached.instructions,
    })
}

#[cfg(test)]
mod tests;
