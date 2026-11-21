//! 找会话（`-s`、`-c`）：照会话目录列（`lm_engine::session::list_sessions`，扫各会话的概要），不起引擎。全屏的 `-s`、`-c` 也
//! 照它查。`-s <编号>` 在列表里找不到的再照会话目录找（LM-25）：子代理的会话不进列表，照编号也打得开。

use lm_engine::session::{Found, Listed};
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
/// `-c` 只认主会话（列会话的那一份）。`-s <编号>` 先在那一份里找；找不到的再照会话目录找（LM-25，
/// `lm_engine::session::find_session`）：子代理的会话不在列表里，照编号直接打开。子会话自己、或者它上面哪一层的会话有窗口开着的
/// 都照实说、不打开（子会话跟着主会话在那个窗口的进程里，两处同时写一份日志会写坏）。
///
/// # Errors
///
/// 没有这个会话、一个都没有、被别的窗口开着、列不出来。
pub fn pick(words: &Words, id: Option<&str>) -> Result<String, NotFound> {
    let failed = |why: String| NotFound {
        reason: "failed",
        message: words.say("list_failed", &[("reason", &why)]),
    };
    let paths = Paths::locate(&Env::current()).map_err(|error| failed(error.to_string()))?;
    let listed = lm_engine::session::list_sessions(&paths).map_err(|e| failed(e.to_string()))?;
    choose(words, id, &listed, |id| {
        lm_engine::session::find_session(&paths, id)
    })
}

/// [`pick`] 的挑法：`listed` 是列出来的主会话（最近的在前），`find` 照编号在会话目录里找（列表里没有时才调）。
fn choose(
    words: &Words,
    id: Option<&str>,
    listed: &[Listed],
    find: impl FnOnce(&str) -> Option<Found>,
) -> Result<String, NotFound> {
    let taken = |id: &str| NotFound {
        reason: "taken",
        message: words.say("taken", &[("id", id)]),
    };
    let found = match id {
        Some(id) => listed.iter().find(|one| one.id.as_str() == id),
        None => listed.first(),
    };
    if let Some(found) = found {
        return match found.open {
            true => Err(taken(found.id.as_str())),
            false => Ok(found.id.as_str().to_string()),
        };
    }
    let Some(id) = id else {
        return Err(NotFound {
            reason: "not_found",
            message: words.get("no_sessions").to_string(),
        });
    };
    // 列表里没有：子会话，或者概要读不出来的主会话。照会话目录找。
    let Some(found) = find(id) else {
        return Err(NotFound {
            reason: "not_found",
            message: words.say("no_such_session", &[("id", id)]),
        });
    };
    match &found.held {
        None => Ok(found.id.as_str().to_string()),
        Some(held) if held == &found.id => Err(taken(id)),
        Some(held) => Err(NotFound {
            reason: "taken",
            message: words.say("taken_above", &[("id", id), ("parent", held.as_str())]),
        }),
    }
}

#[cfg(test)]
mod tests;
