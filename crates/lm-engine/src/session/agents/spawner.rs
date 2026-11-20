//! 一次调用的派子代理的端口（施工 7-5；Miyu 施工 C-9 从 `agents.rs` 挪出来）：领编号、造子会话、把交代送进去。Miyu 的 fork
//! 子代理、交代带的附件不搬。

use std::sync::Arc;

use crate::tool::{AgentPort, NotSpawned, Order, Spawned, Spawning};
use lm_kernel::kernel::block::{Block, Text};
use lm_kernel::kernel::id::{JobId, SessionId};
use lm_kernel::kernel::origin::{By, Session};
use lm_kernel::kernel::session::{Command, Outcome};

use super::{Agents, Inherit, JOB_DEPTH, command_id};
use crate::session::job_ids::JobIds;
use crate::session::spawn::{Child, Lineage};

/// 一次调用的派子代理的端口。
pub(super) struct Spawner {
    pub(super) agents: Arc<Agents>,
    pub(super) ids: Arc<JobIds>,
    pub(super) cwd: String,
    pub(super) dirs: Vec<String>,
    pub(super) inherit: Inherit,
}

impl AgentPort for Spawner {
    /// 标题不交给子会话：它只给头看，记在 `job.started` 里（工具报）。
    fn spawn<'a>(&'a self, order: Order<'a>) -> Spawning<'a> {
        let Order { prompt, .. } = order;
        Box::pin(async move {
            let job = self.ids.next();
            let agents = &self.agents;
            let parent = &agents.session;
            if agents.depth + 1 > JOB_DEPTH {
                return Err(NotSpawned);
            }
            let child = Child {
                lineage: Lineage {
                    parent: parent.clone(),
                    depth: agents.depth + 1,
                },
                command: command_id(parent, &job, ""),
                attended: agents.attended,
                cwd: self.cwd.clone(),
                dirs: self.dirs.clone(),
                model: self.inherit.model(),
            };
            let session = agents.port.create(child).await.map_err(|_| NotSpawned)?;
            // 造好了、还没交回：这时被掐掉（打断、来了一句话），守着的停掉它（施工 7-5 补）。
            let mut unclaimed = Unclaimed {
                agents: Arc::clone(agents),
                job: job.clone(),
                child: Some(session.clone()),
            };
            // 交代原样送进去，不加包装（第一条第 2 条）。
            let by = By::Session(Session { id: parent.clone() });
            let send = Command::Send {
                blocks: vec![Block::Text(Text {
                    text: prompt.to_string(),
                })],
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
