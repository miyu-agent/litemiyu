//! 查用量，执行器这一头（施工 8-15，从 Miyu `miyu-session/src/usage.rs` 搬来）：交给 `session_usage` 的端口
//! （[`crate::tool::UsagePort`]）。派出去那一刻全抄下：内核算的上下文（`Session::context_used()`，和压缩线同一个算法）、
//! 窗口、压缩线、她干活的毫秒数，和给头看的那一份累计（[`Tally`]，照日志一批批加的）。工具在别的任务里跑，拿不到内核和
//! 累计；派出去和跑起来之间这几样不变。Miyu 照 SQLite 的用量汇总查、算金额、换汇率，LiteMiyu 都没有（设计第二节）。

use std::sync::Arc;

use lm_kernel::kernel::session::Session;
use lm_kernel::kernel::time::Timestamp;

use crate::session::current::Tally;
use crate::tool::{ContextUse, SESSION_USAGE, Spent, UsagePort};

/// 派出去的是 `session_usage` 的，照内核这一刻的 `session`、这一刻的累计 `tally`、派出去的时刻 `at` 抄一份端口；别的工具没有
/// （不白算一遍上下文）。
pub(crate) fn port(
    name: &str,
    session: &Session,
    tally: &Tally,
    at: Timestamp,
) -> Option<Arc<dyn UsagePort>> {
    if name != SESSION_USAGE {
        return None;
    }
    let limits = session.context_limits();
    let context = session.context_used().map(|used| ContextUse {
        used,
        window: limits.window,
        line: limits.compaction_line,
    });
    let usage = tally.usage;
    Some(Arc::new(Port {
        context,
        busy_ms: session.busy_ms(at),
        spent: Spent {
            requests: tally.requests,
            input: usage.input(),
            cached: usage.cache_read,
            output: usage.output,
        },
    }))
}

/// 一次调用的端口：派出去那一刻抄下的。
struct Port {
    context: Option<ContextUse>,
    busy_ms: u64,
    spent: Spent,
}

impl UsagePort for Port {
    fn context(&self) -> Option<ContextUse> {
        self.context
    }

    fn busy_ms(&self) -> Option<u64> {
        Some(self.busy_ms)
    }

    fn spent(&self) -> Spent {
        self.spent
    }
}
