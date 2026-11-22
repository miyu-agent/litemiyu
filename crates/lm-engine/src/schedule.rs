//! 循环的工具（Miyu `miyu-schedule` 里循环那一半，L5 四，`docs/blueprint/schedules.md`）：动态循环每轮定下一次的
//! `schedule_wakeup`，和 `/loop` 后面那几个字怎么读（[`parse`]，Miyu 在核心的斜杠命令里）。定时任务的 `cron_*` 三件不搬
//! （`docs/00-设计.md` 第二节：没有常驻进程）。
//!
//! 工具只拿 [`crate::tool::SchedulePort`]：循环的状态、定时器在会话的 actor 里（`session/looping.rs`）。给模型看的字在
//! `resources/software/loop/` 下，编进程序（`tool::shipped`）。Miyu 是能关的软件包；LiteMiyu 没有软件包，主程序把 [`tools`]
//! 并进会话的工具目录（同 [`crate::basesystem::tools`]）。

mod parse;
mod texts;
mod wakeup;

use std::sync::Arc;

use crate::tool::Tool;
use crate::tool::load::LoadError;

pub use parse::{Asked, BadLoop, parse};

/// 字放在 `resources/software/loop` 下，也是给人看的字的键 `software/loop/…` 的那一段：日志里存着。
pub const SOFTWARE: &str = "loop";

/// 光打 `/loop` 交进去的那一句（`software/loop/autonomous.txt`，原文同 Miyu 施工 LO-1，去掉行尾空白）：和 `/loop <话>` 一样是
/// 她自己定节奏的循环，只是交进去的不是人写的话。每次到点交的、记进日志的都是这句原文；界面照它认出这种循环，正文里不原样
/// 显示它（2026-11-22 项目主人）。
///
/// # Errors
///
/// 程序里没有这一份、是空的（安装坏了才会有，测试守着）。
pub fn autonomous() -> Result<&'static str, LoadError> {
    crate::tool::load::plain(SOFTWARE, "autonomous")
}

/// 这一组的工具：只有 `schedule_wakeup`。
///
/// # Errors
///
/// 哪一份字读不出来、写法不对（安装坏了才会有，测试守着出厂的都读得进）。
pub fn tools() -> Result<Vec<Arc<dyn Tool>>, LoadError> {
    Ok(vec![Arc::new(wakeup::Wakeup::load()?)])
}
