//! 头读后台命令的输出（施工 7-4 补），等它长了的信号（施工 V-6 上），人停掉一个任务、父会话停下这个会话的全部任务（施工 7-4）。

use tokio::sync::oneshot;

use crate::tool::{JobError, Output};
use lm_kernel::kernel::id::{CommandId, JobId};
use lm_kernel::kernel::origin::By;

use super::{Halt, Handle, Message, Stopped};
use crate::session::jobs::Unreadable;

impl Handle {
    /// 头读后台命令 `job` 的输出（施工 7-4 补）：交回读得到的字、它还在不在跑。读的和她用 `jobs` 读的是同一份
    /// （`docs/blueprint/session/tools.md` 第 6 条第 3 款）：结束了、存成 blob 的读 blob，别的读会话目录下的输出文件，跑着的
    /// 读到这时的。不进内核、不写盘：是什么照名册当场看，开文件另起一个任务，不占 actor。
    ///
    /// # Errors
    ///
    /// 会话停了。里面那一层：这个会话没派过这个后台命令（[`Unreadable::Unknown`]）。
    pub async fn job_output(&self, job: JobId) -> Result<Result<Output, Unreadable>, Stopped> {
        let (reply, answer) = oneshot::channel();
        self.send(Message::Output { job, reply })?;
        answer.await.map_err(|_| Stopped)
    }

    /// 人停掉任务 `job`（施工 7-4）：`by` 是停它的人，`cause` 是那条命令。后台命令整组杀掉，记 `job.reported`（`stopped`）；
    /// 子代理停掉它这一轮连它派的，记 `child.reported`（`stopped`）。两种都叫醒她。回报落了盘才回。
    ///
    /// # Errors
    ///
    /// 会话停了。里面那一层：这个会话没有这个任务、它已经结束了。
    pub async fn stop_job(
        &self,
        job: JobId,
        by: By,
        cause: CommandId,
    ) -> Result<Result<(), JobError>, Stopped> {
        let (reply, answer) = oneshot::channel();
        self.send(Message::Halt(Halt::One {
            job,
            by,
            cause,
            reply,
        }))?;
        answer.await.map_err(|_| Stopped)
    }

    /// 停掉这个会话派出去、还没结束的全部任务，连它们派的（施工 7-4）：父会话停下它的时候，会话表经端口来调。都只记下、
    /// 不叫醒它；后台命令那几条的 `by`、`cause` 照给的。都停好了才回。
    ///
    /// # Errors
    ///
    /// 会话停了。
    pub(crate) async fn stop_jobs(&self, by: By, cause: CommandId) -> Result<(), Stopped> {
        let (reply, answer) = oneshot::channel();
        self.send(Message::Halt(Halt::All { by, cause, reply }))?;
        answer.await.map_err(|_| Stopped)
    }
}
