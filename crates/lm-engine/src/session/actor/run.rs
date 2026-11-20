//! 派一次工具调用（从 `actor.rs` 挪来，施工 M-3：那边到了 500 行）：照内核的 `RunTool` 抄好这一刻的几样，交给执行工具的端口。

use std::sync::Arc;

use lm_kernel::kernel::session::{Action, Input};

use super::Actor;
use crate::session::agents::Inherit;
use crate::session::jobs::JobLimits;
use crate::session::tools::Dispatch;

impl Actor {
    /// 回内核的 `RunTool`：别的动作不归这里，不回。
    pub(super) fn run_tool(&mut self, action: Action) -> Option<Input> {
        let Action::RunTool {
            call_id,
            name,
            args,
            cwd,
            dirs,
            cause,
            ..
        } = action
        else {
            return None;
        };
        let at = self.clock.now();
        // 任务的上限照这一轮的配置（施工 7-12 下）。
        let jobs = Some(
            self.jobs
                .port(call_id, cause, JobLimits::of(self.config.current())),
        );
        let usage = {
            let counted = self.counted();
            crate::session::usage::port(&name, &self.session, &counted.tally, at)
        };
        self.tools.run(
            at,
            Dispatch {
                call_id,
                name,
                args,
                cwd,
                config: Arc::clone(self.config.current()),
                usage,
                jobs,
                dirs,
                subagents: self.session.subagents(),
                inherit: Inherit::of(&*self.model),
            },
        )
    }
}
