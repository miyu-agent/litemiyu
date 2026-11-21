//! 会话在会话表里的那几样（L5 二，`table.rs`）：造主会话、载入主会话时建一张会话表；造子会话、载入子会话照表里的。派子代理
//! 要的 [`Agents`]、子会话向上回报的 [`Reporter`] 都照它造。Miyu 是核心的会话表造会话时交进 `Create` 的 `sessions`、`jobs`、
//! `lineage` 三格；LiteMiyu 的 `Create`、`Load` 是主程序填的，不加格，子会话走这里的两个入口（[`create_child`]、[`load_child`]）。

use std::path::Path;
use std::sync::Arc;

use lm_kernel::kernel::facts::Environment;
use lm_kernel::kernel::id::{CommandId, SessionId};
use lm_kernel::kernel::origin::{By, Session};
use lm_kernel::kernel::session::Reports;
use lm_kernel::policy::steps::{StepLimits, Watched};

use super::{Create, CreateError, Load, LoadError, Workplace};
use crate::session::agents::{Agents, job_in};
use crate::session::clock::new_id;
use crate::session::config::Configs;
use crate::session::handle::Handle;
use crate::session::job_ids::JobIds;
use crate::session::port::Models;
use crate::session::report::{Reporter, Upstream};
use crate::session::spawn::{Child, ForkChild, Lineage};
use crate::session::table::{Family, Kit, Table};
use crate::store::paths::Paths;
use crate::tool::Catalog;

/// 造主会话、载入主会话：请求模型的端口留得下一份的，建一张会话表（交回它，主会话的把手拿着）；留不下的，没有会话表。
pub(super) fn main_family(
    root: &Paths,
    models: &dyn Models,
    tools: &Arc<Catalog>,
    (home, configs, environment): (Option<&Path>, &Configs, &Environment),
) -> (Family, Option<Arc<Table>>) {
    let Some(models) = models.owned() else {
        return (Family::alone(None), None);
    };
    let table = Table::new(Kit {
        root: root.clone(),
        models,
        tools: Arc::clone(tools),
        home: home.map(Path::to_path_buf),
        configs: configs.clone(),
        offset: environment.offset,
    });
    (table.family(), Some(table))
}

/// 照会话表、第几层定的步数上限：子会话照子代理的（`SUBAGENT_STEPS`），主会话照有没有人看着。
pub(super) fn steps(family: &Family, attended: bool) -> StepLimits {
    match family.lineage {
        Some(_) => StepLimits::of(Watched::Subagent, None),
        None => StepLimits::of(Watched::of(attended), None),
    }
}

/// 能不能派子代理：有会话表、没到深度上限。
pub(super) fn spawns(family: &Family) -> bool {
    family.port().is_some() && Agents::allowed(family.lineage.as_ref())
}

/// 这个会话派子代理、留言、停子代理要的：有会话表才有。`forked` 的是 fork 子代理（LM-18）：它派的 fork 照「不能再 fork」拒。
pub(super) fn agents(
    family: &Family,
    session: &SessionId,
    (attended, forked): (bool, bool),
    reports: Reports,
) -> Option<Arc<Agents>> {
    let port = family.port()?;
    Some(Arc::new(Agents {
        port,
        session: session.clone(),
        depth: Agents::depth_of(family.lineage.as_ref()),
        parent: family
            .lineage
            .as_ref()
            .map(|lineage| lineage.parent.clone()),
        attended,
        reports,
        forked,
    }))
}

/// 子会话往哪儿报（施工 7-6）：有会话表、有父会话、造它的命令读得出任务编号才有。
pub(super) fn reporter(
    family: &Family,
    session: &SessionId,
    command: Option<&CommandId>,
) -> Option<Reporter> {
    let port = family.port();
    let parent = family.lineage.as_ref().map(|lineage| &lineage.parent);
    Upstream::of(port.as_ref(), parent, command, session).map(Reporter::start)
}

/// 任务编号：子会话领的号带上它在父会话里的编号（施工 7-1 补），照造它的命令读回。
pub(super) fn job_ids(family: &Family, command: Option<&CommandId>, used: u64) -> Arc<JobIds> {
    let prefix = family
        .lineage
        .as_ref()
        .zip(command)
        .and_then(|(lineage, command)| job_in(&lineage.parent, command));
    Arc::new(JobIds::starting_after(prefix, used))
}

/// 会话表造一个子会话（`Table::spawn`）：照主会话造出来时的那几样、父会话填好的 `child`，由父会话造（`by` 是它）。
pub(crate) async fn create_child(table: &Arc<Table>, child: Child) -> Result<Handle, CreateError> {
    let kit = table.kit();
    let parent = By::Session(Session {
        id: child.lineage.parent.clone(),
    });
    let now = crate::session::clock::Clock::default().now();
    // 时区照主会话的；在哪干活照父会话这一轮的。
    let setup = Create {
        root: &kit.root,
        id: new_id(now),
        attended: child.attended,
        oneshot: false,
        environment: Environment {
            offset: kit.offset,
            cwd: child.cwd,
            dirs: child.dirs,
        },
        command: child.command,
        by: parent,
        models: &*kit.models,
        tools: Arc::clone(&kit.tools),
        home: kit.home.as_deref(),
        configs: kit.configs.clone(),
        model: child.model,
        no_tools: false,
        // 子会话的预设照主会话记在会话表里的那一份（`open.rs`）。
        preset: None,
    };
    Box::pin(super::created(
        setup,
        Some(table.child_family(child.lineage)),
        false,
    ))
    .await
}

/// 会话表造一个 fork 子代理（LM-18，`Table::fork`）：照父会话落了盘的日志写好头几条（`fork.rs`），再照会话表的规矩载入它。
pub(crate) async fn fork_child(table: &Arc<Table>, child: ForkChild) -> Result<Handle, String> {
    let ForkChild {
        lineage,
        command,
        call,
        placeholder,
    } = child;
    let at = crate::session::clock::Clock::default().now();
    let id = new_id(at);
    let fork = super::fork::Fork {
        parent: lineage.parent,
        id: id.clone(),
        cause: command,
        at,
        call,
        depth: lineage.depth,
        placeholder,
    };
    super::fork::fork(&table.kit().root, fork).await?;
    load_child(table, &id)
        .await
        .map_err(|error| format!("fork {id} not opened: {error}"))
}

/// 会话表载入一个子会话（`Table::handle`：表里没有的）：在哪干活照它的日志，父会话、第几层照它的 `session.created`
/// （`load.rs` 读到了再交给 `family`）。
pub(crate) async fn load_child(
    table: &Arc<Table>,
    session: &SessionId,
) -> Result<Handle, LoadError> {
    let kit = table.kit();
    let setup = Load {
        root: &kit.root,
        id: session.clone(),
        // 在哪干活照它日志里最后一次记下的：子会话的工作目录是父会话派它时那一轮的。
        place: Workplace::Remembered {
            offset: kit.offset,
            pick: Box::new(|remembered: Option<&str>| remembered.unwrap_or_default().to_string()),
        },
        models: &*kit.models,
        tools: Arc::clone(&kit.tools),
        home: kit.home.as_deref(),
        configs: kit.configs.clone(),
    };
    let family = |lineage: Option<Lineage>| match lineage {
        Some(lineage) => table.child_family(lineage),
        None => Family::alone(None),
    };
    Box::pin(super::load::opened(setup, Some(&family)))
        .await
        .map(|(handle, _)| handle)
}
