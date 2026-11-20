//! 循环在 actor 里的那一半（L5 四，`session/looping.rs`）：人开、停（`Handle::start_loop`、`stop_loop`），`schedule_wakeup` 改下一次、
//! 停（`tools/schedules.rs`），定时器到点了交一条说话的命令进内核。
//!
//! 到点交的照 Miyu 的唤醒表（`miyu-endpoint` 的 `schedules/fire.rs`）：一条 `Send`，`by` 是 `schedule`（带任务编号和循环的
//! 种类）；照「别处来的话」的规矩收，闲着开一轮，正在跑一轮的排在后面。Miyu 先把下一次写进表再交：这里定时器先换上再交，
//! 同样她在那一轮里定的下一次不会被盖掉。
//!
//! 命令编号 `schedule/<任务>/<UUIDv7>/say`，和 Miyu（`<这一次的毫秒>`）不一样：Miyu 同一次可能交两回，要靠编号认出来；这里
//! 一次到点只交一回，要的是每次都不一样。会话的时钟不往回走，系统时间往回拨了就停在同一毫秒，光拿毫秒拼，第二次起和第一次
//! 撞了编号，内核只回上一次的回应、不开轮（[`crate::session::clock::unique`]）。重开时那一行同理。

use std::time::Duration;

use tokio::sync::oneshot;

use lm_kernel::kernel::block::{Block, Text};
use lm_kernel::kernel::id::CommandId;
use lm_kernel::kernel::origin::{By, Schedule};
use lm_kernel::kernel::session::{Command, Input, Received};
use lm_kernel::kernel::time::Timestamp;

use super::back::answer_back;
use super::mail::Mail;
use super::{Actor, answer};
use crate::session::clock::unique;
use crate::session::looping::{COMMAND, Order, Pace};
use crate::session::port::Back;
use crate::tool::{ScheduleAnswer, ScheduleAsk, ScheduleRefused};

impl Actor {
    /// 造会话、载入时定下认不认 `/loop`（工具面上有 `schedule_wakeup`），写进给头看的那一份。
    pub(crate) fn set_loops(&self, loops: bool) {
        self.counted().loops = loops;
    }

    /// 载入时认出上一次运行关掉时循环还开着（`looping::Trail`）：跑起来先记一行 `command.ran`（`loop`，`/loop off`，`by` 是
    /// 内核），让人知道循环停了、要再 `/loop`（设计第五节第 9 条）。
    pub(crate) fn set_loop_left(&mut self, left: bool) {
        self.loop_left = left;
    }

    /// 跑起来时要先记的那一行（[`Actor::set_loop_left`]）：只交一次。
    pub(super) fn loop_left_line(&mut self) -> Option<Input> {
        if !std::mem::take(&mut self.loop_left) {
            return None;
        }
        let at = self.clock.now();
        let id = CommandId::parse(&format!("loop/stopped/{}", unique(at))).ok()?;
        Some(Input::Command(Received {
            id,
            by: By::Kernel,
            at,
            command: Command::Ran {
                text: format!("/{COMMAND} off"),
                command: COMMAND.to_string(),
            },
        }))
    }

    /// 人开、停循环：当场办完，交回停掉了没有（开的照原来有没有一个被换掉）。开的那一刻先交一次那句话：定时器零秒到点。
    pub(super) fn loop_order(&mut self, order: Order, reply: oneshot::Sender<bool>) -> Mail {
        let stopped = self.looping.stop();
        if let Order::Start { every, prompt } = order {
            let pace = every.map_or(Pace::Dynamic, |every| Pace::Fixed { every });
            let at = self.clock.now();
            self.looping.start(pace, prompt, at);
            self.arm_loop(0);
        }
        self.show_loop();
        answer(reply, stopped);
        Mail::Done
    }

    /// `schedule_wakeup` 交来的：她自己定节奏的循环才认（同 Miyu 端口的 `dynamic`），当场答。
    pub(super) fn schedule_asked(
        &mut self,
        ask: ScheduleAsk,
        reply: oneshot::Sender<Result<ScheduleAnswer, ScheduleRefused>>,
    ) {
        let dynamic = self
            .looping
            .now
            .as_ref()
            .is_some_and(|looping| looping.pace == Pace::Dynamic);
        let answered = match (dynamic, ask) {
            (false, _) => Err(ScheduleRefused::NoLoop),
            (true, ScheduleAsk::StopLoop) => {
                self.looping.stop();
                Ok(ScheduleAnswer::LoopStopped)
            }
            (true, ScheduleAsk::Wakeup { delay, prompt }) => {
                let millis = delay.saturating_mul(1000);
                // 没有上限（施工 WK-1）：写不下的照最晚的时刻；定时器照 tokio 的办法收在三十来年以内。
                let next = Timestamp::from_unix_millis(
                    self.clock
                        .now()
                        .unix_millis()
                        .saturating_add(i64::try_from(millis).unwrap_or(i64::MAX)),
                )
                .unwrap_or(Timestamp::LATEST);
                if let Some(looping) = self.looping.now.as_mut() {
                    looping.next = Some(next);
                    if let Some(prompt) = prompt {
                        looping.prompt = prompt;
                    }
                }
                self.arm_loop(millis);
                Ok(ScheduleAnswer::Woken { next })
            }
        };
        self.show_loop();
        answer(reply, answered);
    }

    /// 第 `generation` 代的定时器到点了：交一条说话的命令。固定间隔的先换上下一次的定时器，她自己定节奏的停着等她定。旧一代的、
    /// 循环已经停了的不理。
    pub(super) fn loop_due(&mut self, generation: u64) -> Option<Input> {
        if !self.looping.due(generation) {
            return None;
        }
        let at = self.clock.now();
        let looping = self.looping.now.as_mut()?;
        let (id, prompt, pace) = (looping.id.clone(), looping.prompt.clone(), looping.pace);
        looping.next = match pace {
            Pace::Fixed { every } => {
                let millis = i64::try_from(every.saturating_mul(1000)).unwrap_or(i64::MAX);
                Timestamp::from_unix_millis(at.unix_millis().saturating_add(millis))
                    .or(Some(Timestamp::LATEST))
            }
            Pace::Dynamic => None,
        };
        if let Pace::Fixed { every } = pace {
            self.arm_loop(every.saturating_mul(1000));
        }
        self.show_loop();
        // 每次一个新的编号（见模块的说明）：写不成编号的（任务编号不会这样）这一次不交。
        let command = CommandId::parse(&format!("schedule/{id}/{}/say", unique(at))).ok()?;
        Some(Input::Command(Received {
            id: command,
            by: By::Schedule(Schedule {
                schedule: id,
                looping: Some(pace.kind()),
            }),
            at,
            command: Command::Send {
                blocks: vec![Block::Text(Text { text: prompt })],
                urgent: false,
            },
        }))
    }

    /// 换上过 `millis` 毫秒到点的定时器：到点送回「到点了」（带这一代），旧的掐掉。
    fn arm_loop(&mut self, millis: u64) {
        let backs = self.backs.clone();
        self.looping.arm(|generation| {
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_millis(millis)).await;
                answer_back(&backs, Back::LoopDue { generation });
            })
            .abort_handle()
        });
    }

    /// 给头看的循环换成现在的。
    fn show_loop(&self) {
        let now = self.looping.now.clone();
        self.counted().looping = now;
    }
}
