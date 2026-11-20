//! 派子代理，执行器这一头（`docs/blueprint/agents.md` 第一条，`session/tools.md`「派子代理」，施工 7-5，从 Miyu
//! `miyu-session/src/agents.rs` 搬来）：照父会话这一刻的样子填好一个子会话，经会话表的端口（[`SessionPort`]）造出来，再把
//! 交代作为父会话发来的话送进去，开它的第一轮。
//!
//! 一个会话一份 [`Agents`]：第几层、父会话、有没有人能确认，造会话、载入时定。每一次调用照这一轮的工作目录、加进来的目录造
//! 一个端口交给工具（[`Agents::for_call`]），编号从会话共用的那一串里领；父子之间留言的端口也照它造（`messages.rs`，施工 7-7）。
//!
//! 子会话用哪个模型（施工 8-8，`models.md`「怎么走」第三条第 4 条）：父会话这时生效的引用（[`Inherit`]），记进子会话
//! `session.created` 的 `model`。（原来她还能写池 `pool` 让子会话记 `@<池>`，LM-8 砍了模型池跟着没了：旧会话的快照里那一格
//! 还在，她写了也照不写办，`basesystem/subagent.rs`。）
//!
//! fork 子代理（LM-18，照 Miyu 施工 A-F 中，`agents.md`「fork 子代理」）：子会话从这次调用那一步分叉，接着父会话到那一步为止的
//! 对话、用父会话的快照；fork 出来的不能再 fork（[`Agents::forked`]）。
//!
//! Miyu 这里还有场所、人格、预设、记忆、知识库、脚本调工具、覆盖的说明、只读、附件、显示图片、关起来的会话那些格，
//! LiteMiyu 都没有（设计第二节「不要的」）。

use std::sync::Arc;

use crate::tool::AgentPort;
use lm_kernel::kernel::event::{Effect, JobKind, JobStarted};
use lm_kernel::kernel::id::{CallId, CommandId, JobId, SessionId};
use lm_kernel::kernel::origin::{By, Session};
use lm_kernel::policy::JOB_DEPTH;

use crate::session::job_ids::JobIds;
use crate::session::spawn::{Lineage, SessionPort};

mod spawner;

use spawner::Spawner;

/// 一个会话派子代理要的：造子会话的端口，和子会话照抄的、会话里不变的几样。
pub(crate) struct Agents {
    /// 会话表交进来的端口。
    pub(crate) port: Arc<dyn SessionPort>,
    /// 这个会话：子会话的父。
    pub(crate) session: SessionId,
    /// 这个会话是第几层：主会话是 0。
    pub(crate) depth: u32,
    /// 父会话：子会话才有，留言发给它（施工 7-7）。
    pub(crate) parent: Option<SessionId>,
    /// 有没有人能确认：快照里的。
    pub(crate) attended: bool,
    /// 回报的正文怎么截：快照里的（施工 7-4）。停掉子代理时交回的回报照它截，和子会话自己向上回报的一样。
    pub(crate) reports: lm_kernel::kernel::session::Reports,
    /// 这个会话是 fork 子代理（LM-18，同 Miyu 施工 A-F 中）：它再 fork 的，工具照 `agent/no-fork.txt` 拒。
    pub(crate) forked: bool,
}

impl Agents {
    /// 会话能不能派子代理（`agents.md` 第一条第 5、6 条）：还没到深度上限（[`JOB_DEPTH`]）。到了上限的，子会话再往下就
    /// 超了。造会话时照它定工具面里有没有 `subagent`。
    pub(crate) fn allowed(lineage: Option<&Lineage>) -> bool {
        Agents::depth_of(lineage) < JOB_DEPTH
    }

    /// 第几层照 `lineage` 算：主会话没有，是 0。
    pub(crate) fn depth_of(lineage: Option<&Lineage>) -> u32 {
        lineage.map_or(0, |lineage| lineage.depth)
    }

    /// 交给一次调用 `call` 的端口：编号从 `ids` 领，子会话在这一轮的工作目录 `cwd`、加进来的目录 `dirs` 里，模型照 `inherit`
    /// （施工 8-8）。fork 子代理从这次调用那一步分叉（LM-18）。
    pub(crate) fn for_call(
        self: &Arc<Agents>,
        ids: Arc<JobIds>,
        call: CallId,
        (cwd, dirs): (String, Vec<String>),
        inherit: Inherit,
    ) -> Arc<dyn AgentPort> {
        Arc::new(Spawner {
            agents: Arc::clone(self),
            call,
            ids,
            cwd,
            dirs,
            inherit,
        })
    }

    /// 停掉一个没人管的子会话（施工 7-5 补）：派到一半被掐掉的，派完了结果却没人要的（调用已经被掐掉，`tools/back.rs`）。
    /// 在别的任务里发，不等；没有运行时的不发，父会话下次载入时照样收掉它（第一条第 8 条）。
    pub(crate) fn stop_unclaimed(&self, job: &JobId, child: SessionId) {
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let port = Arc::clone(&self.port);
        let id = command_id(&self.session, job, "/stop");
        let by = By::Session(Session {
            id: self.session.clone(),
        });
        runtime.spawn(async move { drop(port.stop(child, id, by).await) });
    }
}

/// 子会话用哪个模型要的（施工 8-8）：父会话这时生效的引用。
#[derive(Debug)]
pub(crate) struct Inherit {
    /// 父会话这时生效的引用；没有的是空的（子会话照它那时的 `models.chat`）。
    pub(crate) reference: Option<String>,
}

impl Inherit {
    /// 照会话请求模型的端口 `model`（它这时生效的引用）抄一份。
    pub(crate) fn of(model: &dyn crate::session::port::ModelPort) -> Inherit {
        Inherit {
            reference: model.reference(),
        }
    }

    /// 子会话记下的引用：父会话的。
    fn model(&self) -> Option<String> {
        self.reference.clone()
    }
}

/// 一份结果的效果里派出去的子代理：任务编号和子会话（施工 7-5 补）。后台命令、没有子会话的不算。
pub(crate) fn spawned_in(effects: &[Effect]) -> Vec<(JobId, SessionId)> {
    effects
        .iter()
        .filter_map(|effect| match effect {
            Effect::JobStarted(JobStarted {
                job,
                what: JobKind::Agent,
                session: Some(child),
                ..
            }) => Some((job.clone(), child.clone())),
            _ => None,
        })
        .collect()
}

/// 父会话发给子会话的命令编号：`<父会话>/<任务编号><后缀>`。父会话的编号整个数据根里不重，任务编号一个会话里不重，
/// 所以它在哪儿都不重。任务编号带着前缀（施工 7-1 补）也还短：段数有深度上限管着，每一段是派过的第几个，离命令编号的
/// 128 个字节远着。
pub(crate) fn command_id(parent: &SessionId, job: &JobId, suffix: &str) -> CommandId {
    CommandId::parse(&format!("{parent}/{job}{suffix}"))
        .unwrap_or_else(|e| unreachable!("会话编号、任务编号都短，合命令编号的写法：{e}"))
}

/// 子会话在父会话里的任务编号（施工 7-6）：从造它的命令编号 `<父会话>/<编号>`（`session.created` 的 `cause`）读回来。
/// 不是这个样子的（不是派出来的）没有。向上回报照它补上任务编号（施工 7-6），它领的号照它带前缀（施工 7-1 补，`JobIds`）。
pub(crate) fn job_in(parent: &SessionId, command: &CommandId) -> Option<JobId> {
    let job = command
        .as_str()
        .strip_prefix(parent.as_str())?
        .strip_prefix('/')?;
    JobId::parse(job).ok()
}

#[cfg(test)]
mod tests;
