//! 停掉派出去的任务（施工 7-4，`docs/blueprint/session/actor.md`「停掉任务」，`agents.md` 第五条）：人停一个，
//! 父会话停下这个会话时连它派的全停。Miyu 另有人删了子代理（`Deleted`，施工 7-8）：LiteMiyu 不删子会话，不搬。她自己用 `jobs` 停的不经这里，走任务端口（`jobs/query.rs`）。撤销停掉那几轮派出去的
//! 也在这里（施工 7-8，`agents.md` 第七条第 1 条）：回报记 `undone`，撤销的回应不等它们。
//!
//! 后台命令当场在阻塞线程里杀、存，回报当场交进内核：人停的，回应排在回报落盘之后（先见结果，后见回应）。子代理要经会话
//! 表停它的会话、再把回报送回这个会话，另起一个任务跑：在收件箱里等，回报就送不进来了。

use std::collections::VecDeque;

use crate::tool::JobError;
use lm_kernel::kernel::id::{CommandId, JobId};
use lm_kernel::kernel::origin::By;
use lm_kernel::kernel::session::StopReason;

use super::back::answer_back;
use super::{Actor, Stop, answer};
use crate::session::handle::Halt;
use crate::session::jobs::{Target, Who, Why};
use crate::session::port::Back;

impl Actor {
    /// 办一件停的事。写不进去的，会话停下。
    pub(super) async fn halt(&mut self, halt: Halt) -> Result<(), Stop> {
        match halt {
            Halt::One {
                job,
                by,
                cause,
                reply,
            } => {
                let who = Who {
                    by,
                    cause: Some(cause),
                    why: Why::Stopped { by_model: false },
                };
                match self.jobs.target(&job) {
                    Err(error) => answer(reply, Err(error)),
                    Ok(Target::Command) => {
                        let result = self.stop_command(job, who).await?;
                        answer(reply, result);
                    }
                    Ok(Target::Agent(child)) => {
                        let stopping =
                            self.jobs
                                .stop_agent(job, child, Why::Stopped { by_model: false });
                        tokio::spawn(async move { answer(reply, stopping.await) });
                    }
                }
            }
            Halt::All { by, cause, reply } => {
                let mut agents = Vec::new();
                for (job, target) in self.jobs.running() {
                    let who = Who {
                        by: by.clone(),
                        cause: Some(cause.clone()),
                        why: Why::Stopped { by_model: true },
                    };
                    match target {
                        // 已经自己结束了的不要紧：只认先到的那一个。
                        Target::Command => match self.stop_command(job, who).await? {
                            Ok(()) | Err(_) => {}
                        },
                        Target::Agent(child) => agents.push(self.jobs.stop_agent(
                            job,
                            child,
                            Why::Stopped { by_model: true },
                        )),
                    }
                }
                tokio::spawn(async move {
                    for stopping in agents {
                        // 已经有人停了它的，不要紧。
                        match stopping.await {
                            Ok(()) | Err(_) => {}
                        }
                    }
                    answer(reply, ());
                });
            }
        }
        Ok(())
    }

    /// 停掉内核交来的几个（动作 `StopJobs`）：撤销停掉那几轮派出去、还在跑的（施工 7-8，`undone`），打断时停这一步在等的
    /// 前台子代理（施工 T-1 下，`stopped`，人停的），太吵的监视命令（施工 MN-1 上，`too_chatty`，`by`、`cause` 照起它的那次调用）。后台命令当场在阻塞线程里整组杀掉、存好输出，回报（`by`、`cause` 是撤销、
    /// 打断的人和命令）经收件箱交回，排在这一批后面：回应不等它落盘，可回应之前命令已经杀了。子代理另起一个任务经会话表停，
    /// 连它派的一起停，回报由子会话交来。已经结束了的（正好自己退出了、已经报过）不要紧。
    pub(super) async fn stop_jobs(
        &mut self,
        jobs: Vec<JobId>,
        by: By,
        cause: Option<CommandId>,
        reason: StopReason,
    ) {
        let why = match reason {
            StopReason::Undone => Why::Undone,
            StopReason::Interrupted => Why::Stopped { by_model: false },
            StopReason::TooChatty => Why::TooChatty,
        };
        let mut agents = Vec::new();
        for job in jobs {
            match self.jobs.target(&job) {
                Ok(Target::Command) => {
                    let who = Who {
                        by: by.clone(),
                        cause: cause.clone(),
                        why,
                    };
                    if let Some(ended) = self.jobs.stop_command(job, who).await {
                        answer_back(&self.backs, Back::Job(ended));
                    }
                }
                Ok(Target::Agent(child)) => agents.push(self.jobs.stop_agent(job, child, why)),
                Err(_) => {}
            }
        }
        if !agents.is_empty() {
            tokio::spawn(async move {
                for stopping in agents {
                    // 已经有人停了它、它正好报完了的，不要紧。
                    match stopping.await {
                        Ok(()) | Err(_) => {}
                    }
                }
            });
        }
    }

    /// 停一条后台命令，回报当场交进内核、落了盘再交回。已经结束了的交回 [`JobError::Ended`]。
    async fn stop_command(&mut self, job: JobId, who: Who) -> Result<Result<(), JobError>, Stop> {
        let Some(ended) = self.jobs.stop_command(job, who).await else {
            return Ok(Err(JobError::Ended));
        };
        let at = self.clock.now();
        let input = self.jobs.arrived(at, ended);
        self.drain(VecDeque::from([input])).await?;
        Ok(Ok(()))
    }
}
