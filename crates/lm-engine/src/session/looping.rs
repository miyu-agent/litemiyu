//! 循环（L5 四，`docs/00-设计.md` 第五节第 9 条；Miyu 的唤醒表 `docs/blueprint/schedules.md` 里循环那一半）：人敲 `/loop`、她调
//! `schedule_wakeup`，到点把那句话交进这个会话开一轮。
//!
//! 和 Miyu 不同：Miyu 的循环是核心里唤醒表（SQLite）的一行，核心重启不丢；LiteMiyu 没有常驻进程，循环只记在 actor 内存里的一个
//! 定时器上（[`LoopRun`]，`actor/looping.rs`），窗口关了循环就停。重开那个会话时写一行让人知道循环停了、要再 `/loop`
//! （[`Trail`]）。一个会话至多一个循环：再开一个换掉原来的（同 Miyu）。
//!
//! - 固定间隔的：开的那一刻先交一次，之后每隔 `every` 秒交一次（Miyu 照上一次往后数整数个间隔、错过的跳过；这里定时器照交的那一刻
//!   往后数，不会错过）。
//! - 撤回还能恢复时到点（LM-34，`actor/looping.rs`）：三种都一样，这一次不交，搁着欠一次（[`Looping::held`]），撤回处理了补交。
//! - 她自己定节奏的（`/loop <话>`、光打的 `/loop`）：开的那一刻先交一次，她调 `schedule_wakeup` 定下一次；那一轮结束了她没定
//!   下一次的，循环结束、标记跟着没了（LM-44，Miyu 施工 LO-3「不调就停」；原来停着等，标记在、循环其实不会再跑）。
//! - `/stop` 也停掉循环（LM-44，Miyu 施工 LO-2）；挂着的到点继续和循环是同一个定时器，一起停。
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
    /// 搁着（LM-34）：到点时有一次撤回还能恢复（`/restore`），这一次没交、欠着；等撤回处理了（恢复了，或者人说了一句、
    /// 那一轮结束了）补交一次。界面照它写「循环在等撤回处理」。
    pub held: bool,
}

/// 人要对循环做的（[`crate::session::Handle::start_loop`]、`stop_loop`）。
#[derive(Debug)]
pub(crate) enum Order {
    /// 开一个，换掉原来的：`every` 是固定间隔的秒数（界面照 [`crate::schedule::parse`] 收过下限），没有的是她自己定节奏；
    /// `typed` 是人敲的那一整行（`/loop 10m …`），开的那一刻交的第一句带着它（LM-34）。
    Start {
        every: Option<u64>,
        prompt: String,
        typed: String,
    },
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
            held: false,
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

    /// 搁着、欠一次（LM-34）：到点时有一次撤回还能恢复。中间到点几次都只欠这一次。
    pub(crate) fn hold(&mut self) {
        if let Some(looping) = self.now.as_mut() {
            looping.held = true;
        }
    }

    /// 欠着一次。
    pub(crate) fn held(&self) -> bool {
        self.now.as_ref().is_some_and(|looping| looping.held)
    }

    /// 不再欠着：要补交了，或者她在那一轮里自己定了下一次（她定的盖过欠着的那一次）。
    pub(crate) fn release(&mut self) {
        if let Some(looping) = self.now.as_mut() {
            looping.held = false;
        }
    }

    /// 她自己定节奏的循环交过了、下一次还没定（也没欠着）：会话闲下来时还是这样的，就是那一轮结束了她没定下一次。
    pub(crate) fn unpaced(&self) -> bool {
        !self.once
            && self.now.as_ref().is_some_and(|looping| {
                looping.pace == Pace::Dynamic && looping.next.is_none() && !looping.held
            })
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
/// 最后一次她用 `schedule_wakeup` 定上以后，没有人敲过 `/loop`、`/stop`（`command.ran`，重开时写的那一行也是 `loop`）、她也没用
/// `schedule_wakeup` 停掉、挂着的到点继续也没交过的，算还开着：窗口关了它跟着停了，要写一行告诉人。她自己定节奏的循环交过
/// 一句以后有一轮结束了、她没定下一次的，那时循环已经自己结束（LM-44），不算还开着。
#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct Trail {
    left: bool,
    /// 她自己定节奏的循环交过一句，她还没定下一次。
    unpaced: bool,
}

/// `/stop` 记的命令名（`command.ran` 的 `command`）：它也停掉循环（LM-44）。
const STOP_COMMAND: &str = "stop";

impl Trail {
    /// 记一条。
    pub(crate) fn note(&mut self, event: &Event) {
        match &event.body {
            // 循环交进来的一句：循环开着。到点继续交进来的（`loop` 没写）：它交过就没了。
            Body::MessageUser(_) => {
                if let By::Schedule(schedule) = &event.by {
                    self.left = schedule.looping.is_some();
                    self.unpaced = schedule.looping == Some(ScheduleLoop::Dynamic);
                }
            }
            Body::CommandRan(ran) if ran.command == COMMAND || ran.command == STOP_COMMAND => {
                self.left = false;
                self.unpaced = false;
            }
            Body::ToolResult(result) => match result.human.as_ref().map(|said| said.key.as_str()) {
                Some(STOPPED_KEY) => {
                    self.left = false;
                    self.unpaced = false;
                }
                Some(SET_KEY) => {
                    self.left = true;
                    self.unpaced = false;
                }
                _ => {}
            },
            // 一轮结束了她还没定下一次：循环那时自己结束了。
            Body::TurnEnded(_) if self.unpaced => {
                self.left = false;
                self.unpaced = false;
            }
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
