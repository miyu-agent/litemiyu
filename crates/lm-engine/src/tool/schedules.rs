//! 循环的端口（Miyu 施工 J-2，设计 `45-定时和循环.md` 第三节第 4 条）：`schedule_wakeup` 经它改动态循环下一次醒的时刻、停掉
//! 循环；没开着循环的会话里经它挂一个到点继续（LM-20，Miyu 没有：那边没开着循环的一律拒）。定时任务（cron 三件）LiteMiyu 不要（`docs/00-设计.md` 第二节：没有常驻进程）。实现在会话：循环的唤醒只记在 actor
//! 内存里的一个定时器上（设计第五节第 9 条，`session/looping.rs`），执行器照一次调用造一个端口交给工具。
//!
//! 和 Miyu 不同：端口是一个会话一个的（一次调用一个），不再带会话编号；答的只有下一次的时刻（Miyu 交回唤醒表里的一整行，
//! `schedule_wakeup` 也只用到下一次的时刻）。

use std::fmt;
use std::future::Future;
use std::pin::Pin;

use lm_kernel::kernel::time::Timestamp;

/// 定下一次、挂到点继续的工具名：工具面上有它的会话才认 `/loop`。
pub const SCHEDULE_WAKEUP: &str = "schedule_wakeup";

/// 动态循环最短隔多久再醒（秒）：短了照它。没有上限（施工 WK-1，项目主人 2026-10-30 定）：长到时刻写不下的，照最晚的时刻。
pub const WAKEUP_MIN: u64 = 60;

/// 她要做的。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScheduleAsk {
    /// 过多少秒再交一次话：已经照 [`WAKEUP_MIN`] 收过下限。开着动态循环的是定它的下一次；没开着循环的是挂一个到点继续
    /// （只交一次）。
    Wakeup {
        /// 过多少秒。
        delay: u64,
        /// 到点交进来的话，覆盖原来的；不写的照旧。没开着循环、也没有挂着的时要写。
        prompt: Option<String>,
    },
    /// 停掉这个会话的动态循环、挂着的到点继续。
    StopLoop,
}

/// 端口答的。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScheduleAnswer {
    /// 动态循环改好了、到点继续挂上了：下一次的时刻。
    Woken {
        /// 下一次醒的时刻。
        next: Timestamp,
    },
    /// 动态循环停了、挂着的到点继续撤了。
    LoopStopped,
}

/// 没成的。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScheduleRefused {
    /// 要停的时候这个会话没开着动态循环、也没有挂着的；人开着固定间隔的循环（它的节奏不归她定）。
    NoLoop,
    /// 没开着循环、也没有挂着的，又没写到点交进来的话。
    NoPrompt,
    /// 会话停了、这次调用已经了结：没办成。
    Unavailable,
}

/// 等端口答的 future。
pub type ScheduleAnswering<'a> =
    Pin<Box<dyn Future<Output = Result<ScheduleAnswer, ScheduleRefused>> + Send + 'a>>;

/// 循环的端口：执行器照一次调用造一个，交给工具。
pub trait SchedulePort: Send + Sync {
    /// 在这个会话上办 `ask`。
    fn schedule(&self, ask: ScheduleAsk) -> ScheduleAnswering<'_>;
}

impl fmt::Debug for dyn SchedulePort {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SchedulePort")
    }
}

/// 两个端口比的是不是同一个：[`crate::tool::Call`] 照格子比较时用。
impl PartialEq for dyn SchedulePort {
    fn eq(&self, other: &dyn SchedulePort) -> bool {
        std::ptr::addr_eq(self, other)
    }
}

impl Eq for dyn SchedulePort {}
