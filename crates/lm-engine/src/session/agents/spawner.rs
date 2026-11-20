//! 一次调用的派子代理的端口（施工 7-5；Miyu 施工 C-9 从 `agents.rs` 挪出来）：领编号、造子会话、把交代送进去。fork 子代理
//! （LM-18，照 Miyu 施工 A-F 中，`agents.md`「fork 子代理」）经会话表从这次调用那一步分叉，交代前面接上外壳
//! （`core/jobs/fork-directive.txt`）。Miyu 交代带的附件不搬。

use std::sync::Arc;

use crate::tool::{AgentPort, NotSpawned, Order, Spawned, Spawning};
use lm_kernel::kernel::block::{Block, Text};
use lm_kernel::kernel::id::{CallId, JobId, SessionId};
use lm_kernel::kernel::origin::{By, Session};
use lm_kernel::kernel::session::{Command, Outcome};

use super::{Agents, Inherit, JOB_DEPTH, command_id};
use crate::session::job_ids::JobIds;
use crate::session::spawn::{Child, ForkChild, Lineage};

/// 一次调用的派子代理的端口。
pub(super) struct Spawner {
    pub(super) agents: Arc<Agents>,
    /// 这次调用：fork 子代理从它那一步分叉（LM-18）。
    pub(super) call: CallId,
    pub(super) ids: Arc<JobIds>,
    pub(super) cwd: String,
    pub(super) dirs: Vec<String>,
    pub(super) inherit: Inherit,
}

impl AgentPort for Spawner {
    /// 标题不交给子会话：它只给头看，记在 `job.started` 里（工具报）。
    fn spawn<'a>(&'a self, order: Order<'a>) -> Spawning<'a> {
        let Order { prompt, fork, .. } = order;
        Box::pin(async move {
            let job = self.ids.next();
            let agents = &self.agents;
            let parent = &agents.session;
            // 深度照算（同 Miyu 施工 A-F 中）：fork 子代理的工具面照父会话的，到了上限也有 `subagent`，派的时候挡。
            if agents.depth + 1 > JOB_DEPTH {
                return Err(NotSpawned);
            }
            let lineage = Lineage {
                parent: parent.clone(),
                depth: agents.depth + 1,
            };
            let command = command_id(parent, &job, "");
            let (session, prompt) = match fork {
                true => {
                    let child = ForkChild {
                        lineage,
                        command,
                        call: self.call,
                        placeholder: crate::resources::fork_placeholder().to_string(),
                    };
                    let session = agents.port.fork(child).await.map_err(|_| NotSpawned)?;
                    (session, directed(prompt))
                }
                false => {
                    let child = Child {
                        lineage,
                        command,
                        attended: agents.attended,
                        cwd: self.cwd.clone(),
                        dirs: self.dirs.clone(),
                        model: self.inherit.model(),
                    };
                    let session = agents.port.create(child).await.map_err(|_| NotSpawned)?;
                    (session, prompt.to_string())
                }
            };
            // 造好了、还没交回：这时被掐掉（打断、来了一句话），守着的停掉它（施工 7-5 补）。
            let mut unclaimed = Unclaimed {
                agents: Arc::clone(agents),
                job: job.clone(),
                child: Some(session.clone()),
            };
            // 交代原样送进去，不加包装（第一条第 2 条）；fork 的前面接上外壳。
            let by = By::Session(Session { id: parent.clone() });
            let send = Command::Send {
                blocks: vec![Block::Text(Text { text: prompt })],
                urgent: false,
            };
            let sent = agents
                .port
                .command(
                    session.clone(),
                    command_id(parent, &job, "/prompt"),
                    by,
                    send,
                )
                .await;
            match sent {
                Ok(Outcome::Accepted { .. } | Outcome::Reverted { .. }) => {
                    unclaimed.child = None;
                    Ok(Spawned { job, session })
                }
                Ok(Outcome::Rejected { .. }) | Err(_) => Err(NotSpawned),
            }
        })
    }

    fn forked(&self) -> bool {
        self.agents.forked
    }
}

/// fork 子代理收到的交代（同 Miyu 施工 A-F 中）：外壳（`core/jobs/fork-directive.txt`，去掉末尾的空白）、一个空行、她写的交代。
fn directed(prompt: &str) -> String {
    format!(
        "{}\n\n{prompt}",
        crate::resources::fork_directive().trim_end()
    )
}

/// 造好了、还没交回的子会话（施工 7-5 补，`agents.md` 第一条第 9 条）：派到一半被掐掉的，丢掉时停掉它，不留没人管的
/// 子代理；交回了就不管（`child` 拿走）。
struct Unclaimed {
    agents: Arc<Agents>,
    job: JobId,
    child: Option<SessionId>,
}

impl Drop for Unclaimed {
    fn drop(&mut self) {
        if let Some(child) = self.child.take() {
            self.agents.stop_unclaimed(&self.job, child);
        }
    }
}
