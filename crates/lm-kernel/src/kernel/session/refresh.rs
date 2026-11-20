//! 请求之前把事实查一遍（`docs/designs/02-内核.md` 第六节）：这一轮里环境、限额变过的，变了的才注入。

use super::Session;
use crate::kernel::event::{Body, Event};
use crate::kernel::facts::Environment;
use crate::kernel::origin::By;
use crate::kernel::time::Timestamp;

impl Session {
    /// 要请求了：这一轮里环境、限额变过的（`turn.refresh`），环境、会话编号、模型查一遍，和有效历史里最近一块不一样的记成
    /// 事实（`08-上下文投影.md` C10）。工作目录写这一轮的：派工具带的是它。
    pub(super) fn refresh_facts(&mut self, at: Timestamp) -> Vec<Event> {
        let Some(turn) = self.turn.as_mut() else {
            return Vec::new();
        };
        // 手动压缩单开的那一轮不查事实（施工 6-8，`compaction.md` 第七条第 4 条）：它不请求主模型；下一轮开头照常比着注入。
        if turn.manual.is_some() {
            return Vec::new();
        }
        if !std::mem::take(&mut turn.refresh) {
            return Vec::new();
        }
        let cause = turn.cause.clone();
        let environment = Environment {
            offset: self.environment.offset,
            cwd: turn.cwd.clone(),
            dirs: turn.dirs.clone(),
        };
        let facts = self.policy.facts.boundary(
            &self.history,
            at,
            &environment,
            (&self.id, self.identity()),
        );
        facts
            .into_iter()
            .map(|fact| self.record(at, By::Kernel, cause.clone(), Body::ContextInjected(fact)))
            .collect()
    }
}
