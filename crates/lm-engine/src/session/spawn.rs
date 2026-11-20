//! 会话表的端口（施工 7-5，`docs/blueprint/agents.md`「在哪」，从 Miyu `miyu-session/src/spawn.rs` 搬来）：会话 actor 比会话表
//! 低一层，派子代理要造子会话、给子会话发命令，经这里定义的 [`SessionPort`]，会话表（`table.rs`）照它实现。子会话向上回报、
//! 父会话载入以后叫起子会话也经它（施工 7-6）。
//!
//! LiteMiyu 的会话表只管主会话和它派的子会话（设计第五节第 2 条）：Miyu 这个端口上的会话列表的推送、闲退、列会话、读别的
//! 会话的日志、看是不是一次性会话、「空了告诉我」这几样不搬（会话之间交流在 L7）。fork 子代理 LM-18 补回来了（[`ForkChild`]）。

use std::future::Future;
use std::pin::Pin;

use lm_kernel::kernel::id::{CallId, CommandId, SessionId};
use lm_kernel::kernel::origin::By;
use lm_kernel::kernel::session::{Command, Outcome};

use crate::session::jobs::Peek;

/// 造子会话、给别的会话发命令。原因是英文的一句，执行器记进运行日志，不给她看。
pub(crate) trait SessionPort: Send + Sync {
    /// 照 `child` 造一个子会话：`session.created` 落了盘、会话表里有了它才交回编号。
    fn create(&self, child: Child) -> Pending<'_, Result<SessionId, String>>;

    /// 照 `child` 造一个 fork 子代理（LM-18，同 Miyu 施工 A-F 中）：从父会话落了盘的日志里派它的那一步分叉，头几条落了盘、
    /// 会话表里有了它才交回编号。
    fn fork(&self, child: ForkChild) -> Pending<'_, Result<SessionId, String>>;

    /// 叫起会话 `session`（施工 7-6）：没在跑的照会话表的规矩载入，在跑的什么都不做。父会话载入以后叫起还没回报的子会话：
    /// 崩了的由它们自己补报、重启打断的接着干（`agents.md` 第八条）。
    fn open(&self, session: SessionId) -> Pending<'_, Result<(), String>>;

    /// 给会话 `session` 发一个命令，等回应：编号 `id`，谁发的 `by`。会话没在跑的，照会话表的规矩先载入。子会话向上回报也
    /// 经它交给父会话（施工 7-6，`report.rs`）。
    fn command(
        &self,
        session: SessionId,
        id: CommandId,
        by: By,
        command: Command,
    ) -> Pending<'_, Result<Outcome, String>>;

    /// 停下会话 `session`（施工 7-4，`agents.md` 第五条）：打断它在跑的一轮（排着的退回），再停掉它派出去、还没结束的，
    /// 连它们派的一起。打断记成 `by` 发的、编号 `id`。停好了才交回。会话没在跑的，照会话表的规矩先载入。
    fn stop(&self, session: SessionId, id: CommandId, by: By) -> Pending<'_, Result<(), String>>;

    /// 会话 `session` 这会儿的样子（施工 7-4）：照它的日志算，不载入它。
    fn peek(&self, session: SessionId) -> Pending<'_, Result<Peek, String>>;
}

/// 端口交回的 future。
pub(crate) type Pending<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// 子会话是谁派的、第几层（`kernel/events-bodies.md`：`session.created` 的 `parent`、`depth`）。主会话没有。Miyu 的
/// `workspace_from`（默认工作区照哪个会话的）LiteMiyu 没有：没有默认工作区。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Lineage {
    /// 父会话。
    pub(crate) parent: SessionId,
    /// 第几层：父会话的加一，至少是 1。
    pub(crate) depth: u32,
}

/// 造一个子会话要的（`agents.md` 第一条第 1 条）：执行器照父会话这一刻的样子填好，会话表照它造。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Child {
    /// 父会话和第几层。
    pub(crate) lineage: Lineage,
    /// 造它的命令编号：`session.created` 的 `cause`，`by` 是父会话。
    pub(crate) command: CommandId,
    /// 有没有人能确认：父会话的。
    pub(crate) attended: bool,
    /// 工作目录：父会话这一轮的。
    pub(crate) cwd: String,
    /// 加进来的目录：父会话这一轮的。
    pub(crate) dirs: Vec<String>,
    /// 子会话用哪个模型（施工 8-8）：模型或 `@池`，记进它的 `session.created`。她写了池的是 `@<池>`（施工 8-8 补），没写的是
    /// 父会话这时生效的；都没有的是空的，子会话照它造出来那时的 `models.chat`。
    pub(crate) model: Option<String>,
}

/// 造一个 fork 子代理要的（LM-18，同 Miyu 施工 A-F 中的 `ForkChild`）：工作目录、快照、模型都照父会话落了盘的日志，只交这几样。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ForkChild {
    /// 父会话和第几层。
    pub(crate) lineage: Lineage,
    /// 造它的命令编号：`session.created` 的 `cause`（`<父会话>/<编号>`），`by` 是父会话。
    pub(crate) command: CommandId,
    /// 派它的那次调用：分叉点是那一步的 `message.assistant`。
    pub(crate) call: CallId,
    /// 那一步每个调用的占位结果（`core/tool-results/forked.txt`）。
    pub(crate) placeholder: String,
}
