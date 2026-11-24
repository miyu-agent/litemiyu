//! 会话表（设计第五节第 2 条，L5 二）：一个主会话一张，管主会话和它派的子会话（连子会话再派的）。派子代理、父子之间留言、
//! 向上回报、停子代理都经它（[`SessionPort`]，Miyu 是核心的会话表经 `miyu-endpoint/src/spawn.rs` 交给会话的那一份）。
//!
//! 谁拿着谁：主会话的把手（[`Handle`]）拿着这张表，表拿着子会话的把手；会话 actor 只拿着表的弱引用（[`Port`]）。界面放下
//! 主会话的把手，表跟着没了，子会话的把手跟着放下，它们的 actor 跟着退出；主会话的 actor 照旧在收件箱关了时退出。表要找
//! 主会话（子会话向上回报、留言给父）时照弱的把手找，主会话没了的就是没了。
//!
//! 子会话在表里照编号找：一个会话只起一个 actor。表里没有的（父会话载入以后叫起的、停了的），照会话目录载入它；别的窗口
//! 直接开着它的（LM-25，`window.rs`）不载入。和 Miyu 不同：没有闲退、没有会话列表的推送、不删子会话。

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError, Weak};

use lm_kernel::kernel::id::{CommandId, SessionId};
use lm_kernel::kernel::origin::By;
use lm_kernel::kernel::session::{Command, Outcome, Queued};
use lm_kernel::kernel::time::UtcOffset;

use crate::session::config::Configs;
use crate::session::current::{Carried, Tally};
use crate::session::handle::{Handle, WeakHandle};
use crate::session::jobs::{Jobs, Peek, peek};
use crate::session::port::Models;
use crate::session::spawn::{Child, ForkChild, Lineage, Pending, SessionPort};
use crate::store::paths::Paths;
use crate::tool::Catalog;

/// 造子会话、载入子会话要的：都照主会话造出来、载入时的。
pub(crate) struct Kit {
    pub(crate) root: Paths,
    pub(crate) models: Arc<dyn Models>,
    pub(crate) tools: Arc<Catalog>,
    pub(crate) home: Option<PathBuf>,
    pub(crate) configs: Configs,
    /// 时区：子会话照主会话的。
    pub(crate) offset: UtcOffset,
}

/// 一张会话表。
pub(crate) struct Table {
    kit: Kit,
    /// 任务表：主会话和子会话共用一张（`jobs.rs`）。
    jobs: Arc<Jobs>,
    /// MCP 服务：主会话和子会话共用一份，第一次要时读（L5 六，`crate::mcp::Servers`）。
    mcp: Mcp,
    /// 主会话：弱的，界面放下它就没了。
    main: Mutex<Option<(SessionId, WeakHandle)>>,
    /// 子会话：一个会话一个 actor。载入、造的时候拿着这把锁，别的同一时刻要它的等着，不起第二个。
    children: tokio::sync::Mutex<BTreeMap<SessionId, Handle>>,
    /// 表里的子会话（连孙会话）一共用掉的（[`AgentsSpent`]）。
    agents: AgentsSpent,
    /// 主会话这个窗口开它以前子代理用掉的：概要里记的（[`Table::resume_agents`]），底栏照它连同子代理一起算
    /// （[`Handle::earlier_agents`]）。没记的没有。
    earlier: Mutex<Option<Tally>>,
}

/// 子代理（连孙代理）一共用掉的：表里的子会话每写一批日志加上这一批（`actor/store.rs`），主会话的窗口写概要时照它记
/// （`window.rs`）。主会话自己的不算在里面。
pub(crate) type AgentsSpent = Arc<Mutex<Tally>>;

/// 一个会话在会话表里的那几样：表（弱的）、共用的任务表、父会话和第几层。造会话、载入时交给 actor。
pub(crate) struct Family {
    pub(crate) table: Weak<Table>,
    pub(crate) jobs: Arc<Jobs>,
    pub(crate) lineage: Option<Lineage>,
    /// MCP 服务（L5 六）：有会话表的照表里那一份，没有的自己一份。
    pub(crate) mcp: Mcp,
    /// 表里的子会话：写日志时把用掉的加进表里那一份（[`AgentsSpent`]）。主会话、没有会话表的没有。
    pub(crate) agents: Option<AgentsSpent>,
}

/// 一个主会话的 MCP 服务，连同第一次读服务、找技能时被忽略的几样：第一次要时读（造会话、载入以后第一次开回合）。
pub(crate) type Mcp = Arc<Extras>;

/// 技能和 MCP 服务这一份（主会话和子会话共用）。
pub(crate) struct Extras {
    /// MCP 服务：第一次要时读（L5 六，`crate::mcp::Servers`）。
    pub(crate) servers: std::sync::OnceLock<Arc<crate::mcp::Servers>>,
    /// 第一次找技能、读 MCP 服务时被忽略的那两样（LM-3补，`crate::checkup`）：造会话时当场填，载入的会话第一次开回合时填；
    /// 无功能的一档什么都不读，填空的。只填一次。主程序照它在第一句话以后弹一条提示。
    pub(crate) ignored: tokio::sync::watch::Sender<Option<crate::checkup::Ignored>>,
    /// 主会话钉着的预设（LM-29）：造主会话、载入时填，只填一次；它派的子会话照这一份筛工具面、技能、MCP 服务
    /// （`open.rs`），头照它写会话用的是哪个预设（`Handle::preset`）。
    pub(crate) preset: std::sync::OnceLock<crate::presets::Chosen>,
    /// 调的一方另交进来的 MCP 服务（`Create`、`Load` 的 `handed`）：造主会话、载入主会话时填，只填一次，读服务以前填。
    pub(crate) handed: std::sync::OnceLock<Vec<lm_kernel::config::mcp::McpServer>>,
}

impl Default for Extras {
    fn default() -> Extras {
        Extras {
            servers: std::sync::OnceLock::new(),
            ignored: tokio::sync::watch::Sender::new(None),
            preset: std::sync::OnceLock::new(),
            handed: std::sync::OnceLock::new(),
        }
    }
}

impl Extras {
    /// 交进来的 MCP 服务：没填的是空的。
    pub(crate) fn handed(&self) -> &[lm_kernel::config::mcp::McpServer] {
        self.handed.get().map_or(&[], Vec::as_slice)
    }

    /// 记下被忽略的那两样（技能、MCP 服务）：已经记过的不再记。
    pub(crate) fn note(&self, found: crate::checkup::Ignored) {
        self.ignored.send_if_modified(|seen| match seen {
            Some(_) => false,
            None => {
                *seen = Some(found);
                true
            }
        });
    }
}

impl Family {
    /// 没有会话表的会话：请求模型的端口造不出能留着的一份（[`Models::owned`]），派不了子代理；或者是界面直接载入的子会话，
    /// 不接回它的父会话。任务表自己一张。
    pub(crate) fn alone(lineage: Option<Lineage>) -> Family {
        Family {
            table: Weak::new(),
            jobs: Arc::new(Jobs::new()),
            lineage,
            mcp: Mcp::default(),
            agents: None,
        }
    }

    /// 交给 actor 的端口：表还在才有。
    pub(crate) fn port(&self) -> Option<Arc<dyn SessionPort>> {
        (self.table.strong_count() > 0)
            .then(|| Arc::new(Port(self.table.clone())) as Arc<dyn SessionPort>)
    }
}

impl Table {
    /// 一张新表：造主会话、载入主会话时造。
    pub(crate) fn new(kit: Kit) -> Arc<Table> {
        Arc::new(Table {
            kit,
            jobs: Arc::new(Jobs::new()),
            mcp: Mcp::default(),
            main: Mutex::new(None),
            children: tokio::sync::Mutex::new(BTreeMap::new()),
            agents: AgentsSpent::default(),
            earlier: Mutex::new(None),
        })
    }

    /// 主会话的窗口接上（`window.rs` 的 `attach`）：子代理一共用掉的从概要里记的 `earlier` 接着算，交回表里那一份。
    pub(crate) fn resume_agents(&self, earlier: Option<&Carried>) -> AgentsSpent {
        let earlier = earlier.map(Tally::resumed);
        *self.agents.lock().unwrap_or_else(PoisonError::into_inner) =
            earlier.clone().unwrap_or_default();
        *self.earlier.lock().unwrap_or_else(PoisonError::into_inner) = earlier;
        Arc::clone(&self.agents)
    }

    /// 这个窗口开主会话以前子代理用掉的（[`Table::resume_agents`]）：没记的没有。
    pub(crate) fn earlier_agents(&self) -> Option<Tally> {
        self.earlier
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// 主会话在这张表里的那几样。
    pub(crate) fn family(self: &Arc<Table>) -> Family {
        Family {
            table: Arc::downgrade(self),
            jobs: Arc::clone(&self.jobs),
            lineage: None,
            mcp: Arc::clone(&self.mcp),
            agents: None,
        }
    }

    /// 主会话造好、载入好了：记下它的弱把手。
    pub(crate) fn set_main(&self, handle: &Handle) {
        *self.main.lock().unwrap_or_else(PoisonError::into_inner) =
            Some((handle.id().clone(), handle.downgrade()));
    }

    /// 会话 `session` 的把手：主会话照弱的找，子会话照表找，表里没有的照会话目录载入它（子会话）。
    async fn handle(self: &Arc<Table>, session: &SessionId) -> Result<Handle, String> {
        let main = self
            .main
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone();
        if let Some((id, weak)) = main
            && &id == session
        {
            return weak
                .upgrade()
                .ok_or_else(|| format!("session {session} stopped"));
        }
        let mut children = self.children.lock().await;
        if let Some(handle) = children.get(session) {
            return Ok(handle.clone());
        }
        // 别的窗口（或者这个进程里的界面）直接开着这个子会话（`-s <子会话编号>`，LM-25）：它的日志那边在写，这里不再起
        // 一个 actor，照实交回出错。
        let dir = self.kit.root.session(session);
        if crate::session::blocking::blocking(move || crate::session::window::open_elsewhere(&dir))
            .await
        {
            return Err(format!("session {session} is open in another window"));
        }
        let handle = crate::session::open::load_child(self, session)
            .await
            .map_err(|error| format!("session {session} not loaded: {error}"))?;
        children.insert(session.clone(), handle.clone());
        Ok(handle)
    }

    /// 表里的一个子会话（[`Handle::child`]）：不照会话目录载入，没有的就是没有。
    pub(crate) async fn child(&self, session: &SessionId) -> Option<Handle> {
        self.children.lock().await.get(session).cloned()
    }

    /// 照 `child` 造一个子会话，放进表里。父会话已经不在了的（停了、主会话被放下了）不造。
    async fn spawn(self: &Arc<Table>, child: Child) -> Result<SessionId, String> {
        let parent = &child.lineage.parent;
        let main_alive = self
            .main
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .is_some_and(|(id, weak)| id == parent && weak.upgrade().is_some());
        let mut children = self.children.lock().await;
        if !main_alive && !children.contains_key(parent) {
            return Err("the parent session is gone".to_string());
        }
        let handle = crate::session::open::create_child(self, child)
            .await
            .map_err(|error| error.to_string())?;
        let id = handle.id().clone();
        children.insert(id.clone(), handle);
        Ok(id)
    }

    /// 照 `child` 造一个 fork 子代理（LM-18），放进表里。父会话已经不在了的不造（同 [`Table::spawn`]）。拿着表的锁写头几条、
    /// 载入，同一时刻别的要它的等着。
    async fn fork(self: &Arc<Table>, child: ForkChild) -> Result<SessionId, String> {
        let parent = &child.lineage.parent;
        let main_alive = self
            .main
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .is_some_and(|(id, weak)| id == parent && weak.upgrade().is_some());
        let mut children = self.children.lock().await;
        if !main_alive && !children.contains_key(parent) {
            return Err("the parent session is gone".to_string());
        }
        let handle = crate::session::open::fork_child(self, child).await?;
        let id = handle.id().clone();
        children.insert(id.clone(), handle);
        Ok(id)
    }

    /// 造子会话、载入子会话要的。
    pub(crate) fn kit(&self) -> &Kit {
        &self.kit
    }

    /// 子会话在这张表里的那几样。
    pub(crate) fn child_family(self: &Arc<Table>, lineage: Lineage) -> Family {
        Family {
            table: Arc::downgrade(self),
            jobs: Arc::clone(&self.jobs),
            lineage: Some(lineage),
            mcp: Arc::clone(&self.mcp),
            agents: Some(Arc::clone(&self.agents)),
        }
    }

    /// 这个会话停了：从表里拿掉，下次要它时照会话目录载入。
    async fn forget(&self, session: &SessionId) {
        self.children.lock().await.remove(session);
    }
}

/// 交给会话 actor 的端口：拿着表的弱引用。表没了（界面放下了主会话），派不了、送不到。
struct Port(Weak<Table>);

impl Port {
    fn table(&self) -> Result<Arc<Table>, String> {
        self.0
            .upgrade()
            .ok_or_else(|| "the main session was closed".to_string())
    }
}

impl SessionPort for Port {
    fn create(&self, child: Child) -> Pending<'_, Result<SessionId, String>> {
        Box::pin(async move { self.table()?.spawn(child).await })
    }

    fn fork(&self, child: ForkChild) -> Pending<'_, Result<SessionId, String>> {
        Box::pin(async move { self.table()?.fork(child).await })
    }

    fn open(&self, session: SessionId) -> Pending<'_, Result<(), String>> {
        Box::pin(async move { self.table()?.handle(&session).await.map(|_| ()) })
    }

    fn command(
        &self,
        session: SessionId,
        id: CommandId,
        by: By,
        command: Command,
    ) -> Pending<'_, Result<Outcome, String>> {
        Box::pin(async move {
            let table = self.table()?;
            let handle = table.handle(&session).await?;
            match handle.command(id, by, command).await {
                Ok(outcome) => Ok(outcome),
                Err(_) => {
                    table.forget(&session).await;
                    Err(format!("session {session} stopped"))
                }
            }
        })
    }

    /// 停下子会话（施工 7-4）：先打断它在跑的一轮（排着的退回，没有在跑的不要紧），再停掉它派出去的，连它们派的。
    fn stop(&self, session: SessionId, id: CommandId, by: By) -> Pending<'_, Result<(), String>> {
        Box::pin(async move {
            let table = self.table()?;
            let handle = table.handle(&session).await?;
            let interrupt = Command::Interrupt {
                queued: Queued::Return,
            };
            let stopped = match handle.command(id.clone(), by.clone(), interrupt).await {
                Ok(_) => handle.stop_jobs(by, id).await,
                Err(stopped) => Err(stopped),
            };
            if stopped.is_err() {
                table.forget(&session).await;
                return Err(format!("session {session} stopped"));
            }
            Ok(())
        })
    }

    /// 子会话这会儿的样子（施工 7-4）：在阻塞线程里只读地读它的日志，不载入它。
    fn peek(&self, session: SessionId) -> Pending<'_, Result<Peek, String>> {
        Box::pin(async move {
            let table = self.table()?;
            let dir = table.kit.root.session(&session);
            let events = crate::spawn_blocking(move || crate::store::log::read_events(&dir))
                .await
                .map_err(|error| error.to_string())?
                .map_err(|error| error.to_string())?;
            Ok(peek(&events))
        })
    }
}
