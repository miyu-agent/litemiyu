//! `schedule_wakeup`（Miyu 施工 J-2，设计 45 第三节第 4 条，照 Claude Code 的 `ScheduleWakeup`）：没写间隔的 `/loop` 是动态循环，
//! 她每轮结束调它定下一次多久以后醒。访问类别是读。
//!
//! - 间隔短于 [`WAKEUP_MIN`] 的照它，不报错，没有上限（施工 WK-1）；`prompt` 覆盖醒来交进来的话，不写的照旧（照 Claude Code，
//!   她每次把同一句原话传回来）；`reason` 只给人看。成了说下一次几点，照会话的时区写。
//! - `stop: true` 停掉这个会话的动态循环，别的参数不看。
//! - 这个会话没开着动态循环的，端口拒，说没有在跑的循环。

use serde::Deserialize;

use crate::tool::load::{self, LoadError};
use crate::tool::{
    Call, Done, Progress, Running, SCHEDULE_WAKEUP, ScheduleAnswer, ScheduleAsk, ScheduleRefused,
    Spec, Tool, WAKEUP_MIN,
};
use lm_kernel::kernel::template::Template;
use lm_kernel::kernel::time::UtcOffset;
use lm_kernel::kernel::tool::Access;

use super::SOFTWARE;
use super::texts::{self, Common, clock};

/// `schedule_wakeup`。
pub(crate) struct Wakeup {
    spec: Spec,
    common: Common,
    set: Template,
    stopped: Template,
    no_loop: Template,
    no_delay: Template,
}

/// 她给的参数：都能不写，`stop` 以外要有 `delay_seconds`。间隔收成整数以前不挑大小：负的、太大的照边界。
#[derive(Deserialize)]
struct Args {
    #[serde(default)]
    delay_seconds: Option<i64>,
    #[serde(default)]
    prompt: Option<String>,
    #[serde(default)]
    reason: Option<String>,
    #[serde(default)]
    stop: bool,
}

impl Wakeup {
    /// 照编进程序的字造。
    pub(crate) fn load() -> Result<Wakeup, LoadError> {
        let wakeup = |name: &str, fields: &[&str]| texts::text("wakeup", name, fields);
        Ok(Wakeup {
            spec: load::spec(SOFTWARE, SCHEDULE_WAKEUP, Access::Read)?,
            common: Common::load()?,
            set: wakeup("set", &["next"])?,
            stopped: wakeup("stopped", &[])?,
            no_loop: wakeup("no-loop", &[])?,
            no_delay: wakeup("no-delay", &[])?,
        })
    }

    /// 端口办成了。
    fn answered(&self, answer: ScheduleAnswer, offset: UtcOffset, reason: &str) -> Done {
        match answer {
            ScheduleAnswer::Woken { next } => {
                let next = clock(next, offset);
                Done::ok(texts::line(&self.set, &[("next", &next)])).said(texts::said(
                    "wakeup/set",
                    &[("next", &next), ("reason", reason)],
                ))
            }
            ScheduleAnswer::LoopStopped => texts::ok(&self.stopped, "wakeup/stopped", &[]),
        }
    }

    /// 端口拒了。
    fn refused(&self, refused: ScheduleRefused) -> Done {
        match refused {
            ScheduleRefused::NoLoop => texts::error(&self.no_loop, "wakeup/no-loop", &[]),
            ScheduleRefused::Unavailable => self.common.off(),
        }
    }
}

impl Tool for Wakeup {
    fn spec(&self) -> &Spec {
        &self.spec
    }

    fn run(&self, call: Call, _progress: Progress) -> Running<'_> {
        Box::pin(async move {
            let args = match lm_kernel::kernel::json::from_str::<Args>(&call.args) {
                Ok(args) => args,
                Err(error) => return self.common.bad_args(&error),
            };
            let ask = match (args.stop, args.delay_seconds) {
                (true, _) => ScheduleAsk::StopLoop,
                (false, Some(delay)) => ScheduleAsk::Wakeup {
                    delay: clamp(delay),
                    prompt: args.prompt.filter(|prompt| !prompt.trim().is_empty()),
                },
                (false, None) => return texts::error(&self.no_delay, "wakeup/no-delay", &[]),
            };
            let Some(port) = call.schedules.as_ref() else {
                return self.common.off();
            };
            let reason = args.reason.unwrap_or_default();
            match port.schedule(ask).await {
                Ok(answer) => self.answered(answer, call.offset, &reason),
                Err(refused) => self.refused(refused),
            }
        })
    }
}

/// 短于 [`WAKEUP_MIN`] 的照它，负的也是。
fn clamp(delay: i64) -> u64 {
    u64::try_from(delay).unwrap_or(0).max(WAKEUP_MIN)
}
