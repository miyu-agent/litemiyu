//! 循环（L5 四，`docs/00-设计.md` 第五节第 9 条；Miyu 的唤醒表 `docs/blueprint/schedules.md` 里循环那一半）：人敲 `/loop`、她调
//! `schedule_wakeup`，到点把那句话交进这个会话开一轮。
//!
//! 和 Miyu 不同：Miyu 的循环是核心里唤醒表（SQLite）的一行，核心重启不丢；LiteMiyu 没有常驻进程，循环只记在 actor 内存里的一个
//! 定时器上（[`LoopRun`]，`actor/looping.rs`），窗口关了循环就停。重开那个会话时写一行让人知道循环停了、要再 `/loop`
//! （[`Trail`]）。一个会话至多一个循环：再开一个换掉原来的（同 Miyu）。
//!
//! - 固定间隔的：开的那一刻先交一次，之后每隔 `every` 秒交一次（Miyu 照上一次往后数整数个间隔、错过的跳过；这里定时器照交的那一刻
//!   往后数，不会错过）。
//! - 她自己定节奏的：开的那一刻先交一次，交过以后停着等，她调 `schedule_wakeup` 定下一次；不调就一直停着（同 Miyu「停着等」）。
//! - 到点继续（LM-20，Miyu 没有）：没开着循环时她调 `schedule_wakeup` 并写了那句话，挂一个只交一次的（[`LoopRun::once`]）。
//!   用的是同一个定时器、同一格 [`Looping`]（界面照循环的样子画下一次几点，`/loop off` 停得掉）；到点交过就没了，交进来的话
//!   照定时任务记（`by` 的 `loop` 不写），不算循环的一轮：她在那一轮里再调一次才有下一次。

use tokio::task::AbortHandle;

use lm_kernel::kernel::event::{Body, Event};
use lm_kernel::kernel::origin::{By, ScheduleLoop};
use lm_kernel::kernel::time::Timestamp;

/// `schedule_wakeup` 停掉循环时给人看的说法的编号（`schedule/wakeup.rs`）：载入时照它认出循环是她停的。
const STOPPED_KEY: &str = "software/loop/wakeup/stopped";

/// `schedule_wakeup` 定上了的说法的编号：载入时照它认出有一个挂着（循环的下一次，或者到点继续）。
const SET_KEY: &str = "software/loop/wakeup/set";

/// 记重开时那一行用的命令名（`command.ran` 的 `command`，人敲 `/loop` 时界面记的也是它）。
pub(crate) const COMMAND: &str = "loop";

/// 循环的节奏。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pace {
    /// 固定间隔：每多少秒一次。
    Fixed {
        /// 每多少秒。
        every: u64,
    },
    /// 她每轮结束自己定下一次。
    Dynamic,
}

impl Pace {
    /// 记进 `by` 的那一格：组装照它挑标签。
    pub(crate) fn kind(self) -> ScheduleLoop {
        match self {
            Pace::Fixed { .. } => ScheduleLoop::Fixed,
            Pace::Dynamic => ScheduleLoop::Dynamic,
        }
    }
}

/// 给头看的循环（[`crate::session::Handle::looping`]）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Looping {
    /// 编号：`t1`、`t2`……这次运行里照开的先后数，写进交进来那句话的 `by`。
    pub id: String,
    /// 节奏。到点继续的也写 [`Pace::Dynamic`]：下一次是她定的。
    pub pace: Pace,
    /// 到点交进来的话。
    pub prompt: String,
    /// 下一次的时刻；她自己定节奏的交过以后、她还没定下一次的没有。
    pub next: Option<Timestamp>,
}

/// 人要对循环做的（[`crate::session::Handle::start_loop`]、`stop_loop`）。
#[derive(Debug)]
pub(crate) enum Order {
    /// 开一个，换掉原来的：`every` 是固定间隔的秒数（界面照 [`crate::schedule::parse`] 收过下限），没有的是她自己定节奏。
    Start { every: Option<u64>, prompt: String },
    /// 停掉。
    Stop,
}

/// actor 里这次运行的循环：现在的那一个、开过几个、等着到点的定时器。
#[derive(Debug, Default)]
pub(crate) struct LoopRun {
    /// 现在的循环，或者挂着的到点继续。
    pub(crate) now: Option<Looping>,
    /// 现在的那一个是到点继续（她在没开着循环时挂的）：到点交一次就没了。
    once: bool,
    /// 这次运行里开过几个：编号照它数。
    made: u32,
    /// 等着到点的定时器：换、停、actor 退出时掐掉。
    timer: Option<AbortHandle>,
    /// 定时器的代数：掐掉以前已经送出来的「到点了」照它认出是旧的。
    generation: u64,
}

impl LoopRun {
    /// 开一个新的，换掉原来的：交回它的编号。
    pub(crate) fn start(&mut self, pace: Pace, prompt: String, at: Timestamp) -> &Looping {
        self.once = false;
        self.made = self.made.saturating_add(1);
        self.now.insert(Looping {
            id: format!("t{}", self.made),
            pace,
            prompt,
            next: Some(at),
        })
    }

    /// 挂一个到点继续：`at` 是到点的时刻。编号照循环的接着数。
    pub(crate) fn start_once(&mut self, prompt: String, at: Timestamp) {
        self.start(Pace::Dynamic, prompt, at);
        self.once = true;
    }

    /// 现在的那一个是到点继续。
    pub(crate) fn once(&self) -> bool {
        self.once && self.now.is_some()
    }

    /// 停掉：有的交回真。
    pub(crate) fn stop(&mut self) -> bool {
        self.disarm();
        self.once = false;
        self.now.take().is_some()
    }

    /// 换上新的定时器：旧的掐掉，交回新的一代。
    pub(crate) fn arm(&mut self, timer: impl FnOnce(u64) -> AbortHandle) {
        self.disarm();
        self.generation = self.generation.wrapping_add(1);
        self.timer = Some(timer(self.generation));
    }

    /// 掐掉定时器：之后送来的旧一代不认。
    pub(crate) fn disarm(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        if let Some(timer) = self.timer.take() {
            timer.abort();
        }
    }

    /// 第 `generation` 代的定时器到点了：是这一代、还有循环的交回真。
    pub(crate) fn due(&mut self, generation: u64) -> bool {
        let due = generation == self.generation && self.now.is_some();
        if due {
            self.timer = None;
        }
        due
    }
}

impl Drop for LoopRun {
    /// actor 退出：等着的定时器跟着掐掉，不留一个睡到点的任务。
    fn drop(&mut self) {
        self.disarm();
    }
}

/// 载入时一路记的：上一次运行关掉时循环是不是还开着、到点继续是不是还挂着（设计第五节第 9 条）。最后一条循环交进来的话、
/// 最后一次她用 `schedule_wakeup` 定上以后，没有人敲过 `/loop`（`command.ran` 的 `loop`，重开时写的那一行也是它）、她也没用
/// `schedule_wakeup` 停掉、挂着的到点继续也没交过的，算还开着：窗口关了它跟着停了，要写一行告诉人。
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct Trail {
    left: bool,
}

impl Trail {
    /// 记一条。
    pub(crate) fn note(&mut self, event: &Event) {
        match &event.body {
            // 循环交进来的一句：循环开着。到点继续交进来的（`loop` 没写）：它交过就没了。
            Body::MessageUser(_) => {
                if let By::Schedule(schedule) = &event.by {
                    self.left = schedule.looping.is_some();
                }
            }
            Body::CommandRan(ran) if ran.command == COMMAND => self.left = false,
            Body::ToolResult(result) => match result.human.as_ref().map(|said| said.key.as_str()) {
                Some(STOPPED_KEY) => self.left = false,
                Some(SET_KEY) => self.left = true,
                _ => {}
            },
            _ => {}
        }
    }

    /// 关掉时循环还开着。
    pub(crate) fn left(self) -> bool {
        self.left
    }
}

#[cfg(test)]
mod tests;
