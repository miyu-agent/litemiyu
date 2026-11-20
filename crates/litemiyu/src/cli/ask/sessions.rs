//! 找会话（`-s`、`-c`）：照会话目录列（`lm_engine::session::list_sessions`，扫各会话的概要），不起引擎。全屏的 `-s`、`-c` 也
//! 照它查。

use lm_engine::session::Listed;
use lm_engine::store::paths::{Env, Paths};

use crate::cli::words::Words;

/// 找不到要的会话：退出码、为什么（`jsonl` 的 `reason`）、那一句。
#[derive(Debug)]
pub struct NotFound {
    /// 为什么：`not_found`、`taken`、`failed`。
    pub reason: &'static str,
    /// 那一句。
    pub message: String,
}

/// 磁盘上的主会话，最近有动静的在前。
///
/// # Errors
///
/// 数据目录找不到、放会话的目录读不了：那一句。
pub fn list() -> Result<Vec<Listed>, String> {
    let paths = Paths::locate(&Env::current()).map_err(|error| error.to_string())?;
    lm_engine::session::list_sessions(&paths).map_err(|error| error.to_string())
}

/// `-s <编号>` 的那一个、`-c` 的最近那一个：在、没有窗口开着的交回编号。
///
/// # Errors
///
/// 没有这个会话、一个都没有、被别的窗口开着、列不出来。
pub fn pick(words: &Words, id: Option<&str>) -> Result<String, NotFound> {
    let listed = list().map_err(|why| NotFound {
        reason: "failed",
        message: words.say("list_failed", &[("reason", &why)]),
    })?;
    let found = match id {
        Some(id) => listed.iter().find(|one| one.id.as_str() == id),
        None => listed.first(),
    };
    let Some(found) = found else {
        return Err(NotFound {
            reason: "not_found",
            message: match id {
                Some(id) => words.say("no_such_session", &[("id", id)]),
                None => words.get("no_sessions").to_string(),
            },
        });
    };
    if found.open {
        return Err(NotFound {
            reason: "taken",
            message: words.say("taken", &[("id", found.id.as_str())]),
        });
    }
    Ok(found.id.as_str().to_string())
}
