//! 配置里的 MCP 服务（`docs/00-设计.md` 第五节第 11 条）：一个服务一张 `[mcp.<名字>]`，读成 [`McpServer`]。一个服务要么是本机
//! 拉起的一条命令，要么是一个网址（Streamable HTTP），见 [`Reach`]。只查写法；怎么起、怎么登记工具在引擎那一边。
//!
//! 从 Miyu 的软件包清单里拆出来的（`miyu-config` 的 `package/mcp.rs`）：LiteMiyu 没有软件包，服务只写在配置里，
//! 也只有本机这一种会话，所以没有清单的 `[[mcp]]`、没有 `venues`、没有 `clicall`。
//!
//! 不进配置清单：读配置的那一边跳过这一张表（`crate::config::parse`）。写错了的那一个不收、记一条问题，别的照收。

mod configured;

pub use configured::configured;

use std::fmt;

use toml_edit::{Item, TableLike, Value};

use crate::config::secret::Reference;

/// 服务名最多几个字符：工具名 `mcp__<服务名>__<工具名>` 最长 64 个，留给工具名的不能太少。
const NAME_CHARS: usize = 32;

/// 一次调用最少等多久，毫秒。
pub const TIMEOUT_MIN: u64 = 1_000;

/// 一次调用最多等多久，毫秒：一小时。
pub const TIMEOUT_MAX: u64 = 3_600_000;

/// 没写的等多久，毫秒：十分钟（浏览器这类服务一次调用就要好一阵）。
pub const TIMEOUT_DEFAULT: u64 = 600_000;

/// 一个 MCP 服务。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpServer {
    /// 服务名：小写字母开头，只有小写字母、数字、`-`，最多 32 个。
    pub name: String,
    /// 怎么到它：拉起一条命令，或者连一个网址。
    pub reach: Reach,
    /// 一次调用等多久，毫秒。
    pub timeout_ms: u64,
    /// 这张表写在第几行。
    pub line: Option<usize>,
}

/// 怎么到一个服务（施工 X-32 上）：`command`、`url` 写一个。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reach {
    /// 本机拉起的进程，经标准输入输出说 MCP。
    Command {
        /// 命令：第一项是程序，后面是参数，不经 shell。
        command: Vec<String>,
        /// 另给它的环境变量，照写的先后：值是字，或者引用一个密钥、一个环境变量。
        env: Vec<(String, EnvValue)>,
    },
    /// 一个网址，经 Streamable HTTP 说 MCP。
    Url {
        /// 网址：`https://`，或者本机地址的 `http://`（本机不本机由核心连的时候认，`mcp.md`「网址的服务怎么说话」）。
        url: String,
        /// 令牌：发成 `Authorization: Bearer <值>`。
        bearer: Option<EnvValue>,
        /// 别的请求头，照写的先后。
        headers: Vec<(String, EnvValue)>,
    },
}

/// 环境变量、令牌、请求头的值。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnvValue {
    /// 照原样。
    Text(String),
    /// `{ secret = … }`、`{ env = … }`：起它的那一刻照核心的密钥文件、环境取。
    Reference(Reference),
}

impl McpServer {
    /// 它的功能的编号：`mcp-<服务名>`。
    pub fn feature_id(&self) -> String {
        format!("mcp-{}", self.name)
    }
}

/// 一处问题是哪一种。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Code {
    /// 表里不认识的键。
    UnknownKey,
    /// 服务写错了：服务名、命令、环境变量、网址、请求头、时限。
    BadMcp,
}

impl Code {
    /// 写进协议、日志的名字。
    pub fn as_str(self) -> &'static str {
        match self {
            Code::UnknownKey => "unknown_key",
            Code::BadMcp => "bad_mcp",
        }
    }
}

/// 读不成的一处：第几行、代码、换进给人看的那一句的那一格、英文一句。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    /// 第几行，从 1 数。
    pub line: Option<usize>,
    /// 代码。
    pub code: Code,
    /// 换进给人看的那一句的那一格：哪个键、写错的值。
    pub detail: String,
    /// 英文一句，日志用。
    pub message: String,
    /// 不收的是哪个服务：`[mcp.<名字>]` 的名字原样，`mcp` 整个没写成表的是 `mcp`（[`configured`] 填；主程序照它点名，
    /// LM-3补）。
    pub server: Option<String>,
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.line {
            Some(line) => write!(f, "line {line}: {}", self.message),
            None => f.write_str(&self.message),
        }
    }
}

/// 一张表里认的键。
const FIELDS: [&str; 6] = ["command", "env", "url", "bearer", "headers", "timeout_ms"];

/// 表 `table`（叫 `name`）里只能有 [`FIELDS`]；`text` 是整份配置的字，算行号用。
fn only(text: &str, table: &dyn TableLike, name: &str) -> Result<(), Problem> {
    for (key, item) in table.iter() {
        if !FIELDS.contains(&key) {
            return Err(Problem {
                line: item.span().map(|span| line_of(text, span.start)),
                code: Code::UnknownKey,
                detail: format!("{name}.{key}"),
                message: format!("{name}.{key} is not a known key"),
                server: None,
            });
        }
    }
    Ok(())
}

/// 第 `offset` 个字节在第几行（从 1 数）。
fn line_of(text: &str, offset: usize) -> usize {
    text.get(..offset)
        .map_or(1, |head| head.matches('\n').count() + 1)
}

/// 叫 `name` 的服务的几格，表是 `table`，写在第 `line` 行。
fn fields(table: &dyn TableLike, name: String, line: Option<usize>) -> Result<McpServer, Problem> {
    let at = format!("mcp.{name}");
    let reach = reach(table, &at, line)?;
    let timeout_ms = match table.get("timeout_ms") {
        Some(item) => item
            .as_integer()
            .and_then(|ms| u64::try_from(ms).ok())
            .filter(|ms| (TIMEOUT_MIN..=TIMEOUT_MAX).contains(ms))
            .ok_or_else(|| {
                bad(
                    line,
                    &format!("{at}.timeout_ms"),
                    format!(
                        "{at}.timeout_ms must be a whole number from {TIMEOUT_MIN} to {TIMEOUT_MAX}"
                    ),
                )
            })?,
        None => TIMEOUT_DEFAULT,
    };
    Ok(McpServer {
        name,
        reach,
        timeout_ms,
        line,
    })
}

/// 怎么到它：`command`（可以带 `env`）、`url`（可以带 `bearer`、`headers`）写一个，混着写、都没写的不收。
fn reach(table: &dyn TableLike, at: &str, line: Option<usize>) -> Result<Reach, Problem> {
    let wrong =
        |key: &str, what: &str| bad(line, &format!("{at}.{key}"), format!("{at}.{key} {what}"));
    let values = "must be a table, each value text or { secret = … } or { env = … }";
    if table.get("url").is_none() {
        for key in ["bearer", "headers"] {
            if table.get(key).is_some() {
                return Err(wrong(key, "goes with url, not command"));
            }
        }
        let command = table
            .get("command")
            .and_then(Item::as_array)
            .and_then(texts)
            .filter(|command| !command.is_empty())
            .ok_or_else(|| {
                wrong(
                    "command",
                    "must be a list of words, the program first, none of them empty; or write url instead",
                )
            })?;
        let env = match table.get("env") {
            Some(item) => named(item, valid_variable).ok_or_else(|| wrong("env", values))?,
            None => Vec::new(),
        };
        return Ok(Reach::Command { command, env });
    }
    for key in ["command", "env"] {
        if table.get(key).is_some() {
            return Err(wrong(key, "goes with command, not url: write one of them"));
        }
    }
    let url = table
        .get("url")
        .and_then(Item::as_str)
        .filter(|url| valid_url(url))
        .ok_or_else(|| {
            wrong(
                "url",
                "must start with https:// (or http:// for this machine), no spaces",
            )
        })?
        .to_string();
    let bearer = match table.get("bearer") {
        Some(item) => Some(
            value(item)
                .ok_or_else(|| wrong("bearer", "must be text or { secret = … } or { env = … }"))?,
        ),
        None => None,
    };
    let headers = match table.get("headers") {
        Some(item) => named(item, valid_header).ok_or_else(|| wrong("headers", values))?,
        None => Vec::new(),
    };
    Ok(Reach::Url {
        url,
        bearer,
        headers,
    })
}

/// 网址的写法：`https://` 或 `http://` 开头，后面还有字，没有空白、控制字符。`http://` 只给本机地址，连的时候认。
fn valid_url(url: &str) -> bool {
    let rest = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"));
    rest.is_some_and(|rest| !rest.is_empty())
        && !url.chars().any(|c| c.is_whitespace() || c.is_control())
}

/// 环境变量名：不是空的，没有 `=` 和 NUL。
fn valid_variable(name: &str) -> bool {
    !name.is_empty() && !name.contains(['=', '\0'])
}

/// 请求头的名字：字母、数字、`-`，不是空的；`Authorization` 归 `bearer`，`Mcp-Session-Id`、`MCP-Protocol-Version`、`Mcp-Method`、
/// `Mcp-Name`（施工 PT-1 上，规范的标准请求头）归核心。
fn valid_header(name: &str) -> bool {
    !name.is_empty()
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        && ![
            "authorization",
            "mcp-session-id",
            "mcp-protocol-version",
            "mcp-method",
            "mcp-name",
        ]
        .contains(&name.to_ascii_lowercase().as_str())
}

/// 服务名合不合写法。
fn valid_name(name: &str) -> bool {
    name.starts_with(|c: char| c.is_ascii_lowercase())
        && name.chars().count() <= NAME_CHARS
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// 一串字，每一个都不是空的、没有 NUL；有不是字的没有。
fn texts(array: &toml_edit::Array) -> Option<Vec<String>> {
    array
        .iter()
        .map(|value| {
            value
                .as_str()
                .filter(|text| !text.is_empty() && !text.contains('\0'))
                .map(str::to_string)
        })
        .collect()
}

/// `env`、`headers`：名字合 `valid` 的，值照 [`value`]；有一项不合的没有。
fn named(item: &Item, valid: fn(&str) -> bool) -> Option<Vec<(String, EnvValue)>> {
    item.as_table_like()?
        .iter()
        .map(|(key, item)| {
            if !valid(key) {
                return None;
            }
            Some((key.to_string(), value(item)?))
        })
        .collect()
}

/// 一个值：字（没有 NUL），或者一个引用（`{ secret = … }`、`{ env = … }`）。
fn value(item: &Item) -> Option<EnvValue> {
    match item {
        Item::Value(Value::String(text)) => {
            let text = text.value();
            (!text.contains('\0')).then(|| EnvValue::Text(text.clone()))
        }
        other => Some(EnvValue::Reference(Reference::read(
            other.as_table_like()?,
        )?)),
    }
}

/// 写错了的一处。
fn bad(line: Option<usize>, detail: &str, message: String) -> Problem {
    Problem {
        line,
        code: Code::BadMcp,
        detail: detail.to_string(),
        message,
        server: None,
    }
}
