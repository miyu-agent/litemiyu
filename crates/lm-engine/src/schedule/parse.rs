//! `/loop` 后面的字怎么读（Miyu 施工 J-2，核心的斜杠命令 `commands/looping.rs` 里的那一半；设计 `45-定时和循环.md` 第三节第 4 条，
//! 照 Claude Code）。界面把 `/loop` 后面的字交给 [`parse`]，照读出来的调会话的把手（`Handle::start_loop`、`stop_loop`）。
//!
//! - `/loop <间隔> <话>`、`/loop <话> every <间隔>`、`/loop every <间隔> <话>`：固定间隔。间隔写 `30s`、`5m`、`2h`、`1d`，不到
//!   [`WAKEUP_MIN`] 秒的照它。
//! - `/loop <话>`：动态，她每轮自己调 `schedule_wakeup` 定下一次，不调就停。
//! - 光打 `/loop`：自主（Miyu 施工 LO-1，LM-44）。同动态，交进去的是出厂的那一句（[`super::autonomous`]）。
//! - `/loop off`：停掉这个会话的循环；单独一个 `clear`、`stop` 同它，大小写不论（Miyu 施工 LO-3）。

use std::fmt;

use crate::tool::WAKEUP_MIN;

/// 人要做的。
#[derive(Debug, PartialEq, Eq)]
pub enum Asked<'a> {
    /// 固定间隔：每多少秒一次（收过 [`WAKEUP_MIN`] 的下限），说什么。
    Fixed {
        /// 每多少秒。
        every: u64,
        /// 每次交进去的话。
        prompt: &'a str,
    },
    /// 动态：她自己定节奏，说什么。
    Dynamic {
        /// 每次交进去的话。
        prompt: &'a str,
    },
    /// 自主：光打 `/loop`，同动态，说的是出厂的那一句（[`super::autonomous`]）。
    Autonomous,
    /// 停掉这个会话的循环。
    Off,
}

/// 单独一个就是关掉循环的词（Miyu 施工 LO-3，项目主人 2026-11-16 定 `clear`、`stop` 同 `off`）：大小写不论；后面带了字的是
/// 要说的话。
const OFF: [&str; 3] = ["off", "clear", "stop"];

/// 参数不对：只写了间隔没写话（Miyu 回 `bad_params`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BadLoop;

impl fmt::Display for BadLoop {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("/loop needs something to say")
    }
}

impl std::error::Error for BadLoop {}

/// 读 `/loop` 后面的字（前后空白不算）。什么都没写的是自主。
///
/// # Errors
///
/// 只写了间隔没写话的（[`BadLoop`]）。
pub fn parse(asked: &str) -> Result<Asked<'_>, BadLoop> {
    let asked = asked.trim();
    if OFF.iter().any(|word| asked.eq_ignore_ascii_case(word)) {
        return Ok(Asked::Off);
    }
    let words: Vec<&str> = asked.split_whitespace().collect();
    let fixed = match words.as_slice() {
        [] => return Ok(Asked::Autonomous),
        ["every", interval, ..] => seconds(interval).map(|every| (every, after(asked, 2))),
        [interval, ..] if seconds(interval).is_some() => {
            seconds(interval).map(|every| (every, after(asked, 1)))
        }
        [.., "every", interval] => seconds(interval).map(|every| (every, before(asked, 2))),
        _ => None,
    };
    match fixed {
        Some((_, "")) => Err(BadLoop),
        Some((every, prompt)) => Ok(Asked::Fixed {
            every: every.max(WAKEUP_MIN),
            prompt,
        }),
        None => Ok(Asked::Dynamic { prompt: asked }),
    }
}

/// `30s`、`5m`、`2h`、`1d` 是多少秒；别的、溢出的没有。
fn seconds(word: &str) -> Option<u64> {
    let unit = match word.chars().last()? {
        's' => 1,
        'm' => 60,
        'h' => 3_600,
        'd' => 86_400,
        _ => return None,
    };
    let number = word.get(..word.len() - 1)?;
    if number.is_empty() || !number.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    number.parse::<u64>().ok()?.checked_mul(unit)
}

/// 跳过开头 `count` 个词以后剩下的原文，去掉前后空白。
fn after(text: &str, count: usize) -> &str {
    let mut rest = text.trim_start();
    for _ in 0..count {
        rest = rest
            .split_once(char::is_whitespace)
            .map_or("", |(_, rest)| rest.trim_start());
    }
    rest.trim_end()
}

/// 去掉末尾 `count` 个词以后剩下的原文，去掉前后空白。
fn before(text: &str, count: usize) -> &str {
    let mut rest = text.trim_end();
    for _ in 0..count {
        rest = rest
            .rsplit_once(char::is_whitespace)
            .map_or("", |(rest, _)| rest.trim_end());
    }
    rest.trim_start()
}

#[cfg(test)]
mod tests;
