//! 怎么收尾：退出码、为什么、说为什么结束的那一句（照 Miyu `cli/ask.md`「说为什么结束的那一句」「退出码」）。

use super::follow::{Failure, Follow};
use crate::cli::words::Words;

/// 命令行自己停下的（不是那一轮自己结束的）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stop {
    /// Ctrl+C、SIGTERM。
    Interrupted,
    /// 到了 `--timeout`。
    TimedOut,
    /// 到了 `--max-steps`。
    MaxSteps,
}

/// 收尾。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ending {
    /// 退出码。
    pub exit: u8,
    /// 退出码不是 0 的为什么（`jsonl` 的 `done`、`json` 的 `reason`）。
    pub reason: Option<String>,
    /// 说为什么结束的那一句（`text` 印在标准错误上）；不用说的没有。
    pub said: Option<String>,
    /// 最后结束的那一轮最后一次请求出的错。
    pub error: Option<Failure>,
}

impl Ending {
    /// 开好会话以前就出错的：退出码、为什么，话已经说过了。
    pub fn early(exit: u8, reason: &str) -> Ending {
        Ending {
            exit,
            reason: Some(reason.to_string()),
            said: None,
            error: None,
        }
    }
}

/// 照跟过的几轮和命令行停没停下算收尾。
pub fn ending(follow: &Follow, stop: Option<Stop>, words: &Words) -> Ending {
    let error = follow.turns.last().and_then(|turn| turn.error.clone());
    if let Some(stop) = stop {
        let reason = match stop {
            Stop::Interrupted => "interrupted",
            Stop::TimedOut => "timed_out",
            Stop::MaxSteps => "max_steps",
        };
        let mut said = words.get(&format!("end.{reason}")).to_string();
        if !follow.jobs.is_empty() {
            said.push_str(words.get("end.jobs_stopped"));
        }
        return Ending {
            exit: 3,
            reason: Some(reason.to_string()),
            said: Some(said),
            error,
        };
    }
    let Some(last) = follow.turns.last() else {
        return Ending {
            exit: 1,
            reason: Some("failed".to_string()),
            said: Some(words.get("end.no_turn").to_string()),
            error,
        };
    };
    let (exit, reason, said) = match last.reason.as_str() {
        "completed" if follow.denied.is_empty() => (0, None, None),
        "completed" => (4, Some("unattended".to_string()), None),
        "interrupted" => (
            3,
            Some("interrupted".to_string()),
            Some(words.get("end.interrupted").to_string()),
        ),
        "error" => match &error {
            Some(failure) if failure.class == "no_model" => (
                5,
                Some("no_model".to_string()),
                Some(words.get("no_model").to_string()),
            ),
            Some(failure) => (1, Some("error".to_string()), Some(failed(words, failure))),
            None => (
                1,
                Some("error".to_string()),
                Some(words.say("end.error", &[("message", words.get("class.other"))])),
            ),
        },
        other => (
            1,
            Some(other.to_string()),
            Some(words.say("end.unfinished", &[("reason", other)])),
        ),
    };
    Ending {
        exit,
        reason,
        said,
        error,
    }
}

/// 「出错了：<分类>：<原话>」；原话是空的只写分类。
fn failed(words: &Words, failure: &Failure) -> String {
    let key = format!("class.{}", failure.class);
    let class = match words.get(&key) {
        same if same == key => words.get("class.other"),
        class => class,
    };
    let message = failure.message.trim();
    let text = match message.is_empty() {
        true => class.to_string(),
        false => format!("{class}{}{message}", words.get("colon")),
    };
    words.say("end.error", &[("message", &text)])
}
