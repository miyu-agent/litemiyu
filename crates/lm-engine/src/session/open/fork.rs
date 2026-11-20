//! 造 fork 子代理（LM-18，照 Miyu `miyu-session/src/open/fork.rs` 的 fork 子代理那一支、`miyu-endpoint/src/sessions/forked.rs`）：
//! 照父会话落了盘的日志算分叉点（派它的那次调用所在的 `message.assistant`）和那一刻的快照、模型、工作目录，写好子会话的头几条
//! （带 `forked` 的 `session.created`，序号接着分叉点；那一步每个调用的占位结果；以 `forked` 收住那一轮），再由会话表照常载入它
//! （载入认得 `forked`，照它读父会话的日志当底子，`load/stream.rs`）。
//!
//! 子会话不拷父会话的日志：只记父会话和分叉点。父会话的日志只往后追加，分叉点以前的不会变，父会话后来压缩、撤销记的都在分叉点
//! 以后，子会话读不到。
//!
//! 父会话一块块读（同 Miyu 施工 V-39 中）：边读边交给内核算分叉（`ForkFeed`），整份不同时留在内存里。
//!
//! 和 Miyu 不一样的：没有场所、权限、人格、预设、默认工作区、会话列表的索引、封存那几格（LiteMiyu 都没有）；Miyu 「分叉那一遍
//! 喂好的底子交给紧接着的载入」（V-39 下）没搬，载入再读一遍父会话。

use lm_kernel::kernel::event::{Body, Event, Forked, SessionCreated};
use lm_kernel::kernel::facts::Environment;
use lm_kernel::kernel::id::{CallId, CommandId, Seq, SessionId};
use lm_kernel::kernel::origin::By;
use lm_kernel::kernel::session::{ForkFeed, LoadError as Broken, Session};
use lm_kernel::kernel::time::{Timestamp, UtcOffset};
use lm_kernel::policy::Snapshot;

use super::scan::Place;
use crate::session::blocking::blocking_once;
use crate::session::store::Store;
use crate::store::blob::Blobs;
use crate::store::log::{Flow, SEGMENT_LIMIT, SessionLog, abandon, read_range};
use crate::store::paths::Paths;

/// 分叉要的几样。
pub(crate) struct Fork {
    /// 父会话。
    pub(crate) parent: SessionId,
    /// 子会话的编号。
    pub(crate) id: SessionId,
    /// 造它的命令编号：子会话的 `session.created` 的 `cause`。
    pub(crate) cause: CommandId,
    /// 此刻。
    pub(crate) at: Timestamp,
    /// 派它的那次调用：还在等结果的。
    pub(crate) call: CallId,
    /// 子会话第几层：父会话的加一。
    pub(crate) depth: u32,
    /// 那一步每个调用的占位结果（`core/tool-results/forked.txt`）。
    pub(crate) placeholder: String,
}

/// 写好子会话的头几条，交回分叉点。父会话照只读读（它正在跑这一轮）。原因是英文的一句，执行器记进运行日志。
pub(crate) async fn fork(root: &Paths, fork: Fork) -> Result<Seq, String> {
    let parent_dir = root.session(&fork.parent);
    let child_dir = root.session(&fork.id);
    let blobs = Blobs::new(root.blobs());
    // 同载入（`load.rs`）：在单起、做完就退的线程里读，解父会话日志时的分配跟着那条线程放掉。
    blocking_once(move || {
        let Fork {
            parent,
            id,
            cause,
            at,
            call,
            depth,
            placeholder,
        } = fork;
        let mut created: Option<Option<SessionCreated>> = None;
        let mut feed = ForkFeed::new(parent.clone());
        let mut place = Place::default();
        read_range(&parent_dir, Seq::FIRST, None, |events| {
            if created.is_none() {
                created = Some(match events.first().map(|event| &event.body) {
                    Some(Body::SessionCreated(first)) => Some((**first).clone()),
                    _ => None,
                });
            }
            feed.feed(events, &mut |event| place.note(event));
            Flow::Go
        })
        .map_err(|error| format!("parent log not read: {error}"))?;
        let Some(first) = created.flatten() else {
            return Err("parent log does not start with session.created".to_string());
        };
        // fork 出来的不能再 fork（同 Miyu 的 `refused`）：工具那一头已经挡了，这里再守一道。
        if first.forked.is_some() {
            return Err("a fork cannot fork again".to_string());
        }
        let (plan, base) = feed
            .finish(Some(call), &mut |event| place.note(event))
            .map_err(|error: Broken| format!("fork point not found: {error}"))?;
        let bytes = blobs
            .get(&plan.policy)
            .map_err(|error| format!("policy snapshot not fetched: {error}"))?;
        let snapshot = Snapshot::from_bytes(&bytes)
            .map_err(|error| format!("policy snapshot not understood: {error}"))?;
        let policy = snapshot
            .policy()
            .map_err(|error| format!("policy not built from the snapshot: {error}"))?;
        let (cwd, dirs) = place.remembered();
        let child = SessionCreated {
            policy: plan.policy,
            oneshot: false,
            cwd: cwd.clone().or(first.cwd),
            parent: Some(parent.clone()),
            depth: Some(depth),
            workspace_from: None,
            model: plan.model,
            no_tools: first.no_tools,
            forked: Some(Forked {
                from: parent.clone(),
                upto: plan.upto,
                seal_after: None,
                within: plan.within,
            }),
        };
        // 内核只照底子写头几条，不攒底子的有效历史（同 Miyu 施工 V-39 中）：环境照记下的，时区不进头几条。
        let environment = Environment {
            offset: UtcOffset::UTC,
            cwd: cwd.unwrap_or_default(),
            dirs,
        };
        let by = By::Session(lm_kernel::kernel::origin::Session { id: parent });
        let events: Vec<Event> = Session::fork_within(
            id,
            &base,
            &placeholder,
            cause,
            by,
            at,
            child,
            policy,
            environment,
        )
        .map_err(|error| format!("fork not written: {error}"))?;
        drop(base);
        let mut log = SessionLog::create_at(&child_dir, SEGMENT_LIMIT, plan.upto.next())
            .map_err(|error| format!("fork not written: {error}"))?;
        if let Err(error) = Store::append(&mut log, &events) {
            drop(log);
            drop(abandon(&child_dir));
            return Err(format!("fork not written: {error}"));
        }
        Ok(plan.upto)
    })
    .await
}
