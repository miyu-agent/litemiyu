//! 等人的那几段（Miyu 终端 `transcript/waited.rs`，发版前 check，2026-11-12 项目主人：等人确认、答题时用时还在走）：和核心算
//! 用时的口径一样——从要确认（`tool.approval_requested`）、提问（`question.asked`）起，到定了、答了或者那一次调用有了结果为止；
//! 几处同时在等的只算一次。运行状态行的用时、收尾行的用时都减掉它。

use std::collections::HashSet;
use std::time::{Duration, Instant};

use crate::core::Asking;

/// 这一轮等人等了多久。
#[derive(Debug, Default, Clone)]
pub struct Waited {
    /// 还在等的调用。
    open: HashSet<String>,
    /// 从什么时候起在等（`open` 不空时才有）。
    since: Option<Instant>,
    /// 等完了的几段加起来。
    total: Duration,
}

impl Waited {
    /// 新的一轮：从零算。
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// 确认、提问的事件：开始等、等完。
    pub fn asking(&mut self, asking: &Asking, now: Instant) {
        match asking {
            Asking::Asked(asked) => self.start(&asked.call_id.to_string(), now),
            Asking::Approval(asked) => self.start(&asked.call_id.to_string(), now),
            Asking::Answered(answered) => self.end(&answered.call_id.to_string(), now),
            Asking::Decided(decided) => self.end(&decided.call_id.to_string(), now),
        }
    }

    /// 这一次调用开始等人。
    fn start(&mut self, call_id: &str, now: Instant) {
        self.open.insert(call_id.to_string());
        self.since.get_or_insert(now);
    }

    /// 这一次调用不用再等了（定了、答了、有了结果）。
    pub fn end(&mut self, call_id: &str, now: Instant) {
        if !self.open.remove(call_id) || !self.open.is_empty() {
            return;
        }
        if let Some(since) = self.since.take() {
            self.total += now.saturating_duration_since(since);
        }
    }

    /// 到 `now` 一共等了多久（还在等的那一段算到 `now`）。
    pub fn total(&self, now: Instant) -> Duration {
        self.total
            + self
                .since
                .map_or(Duration::ZERO, |s| now.saturating_duration_since(s))
    }

    /// 现在在等人。
    pub fn waiting(&self) -> bool {
        self.since.is_some()
    }
}

#[cfg(test)]
pub(super) mod tests;
