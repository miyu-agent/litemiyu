//! 拿着一个会话：发命令、订阅、有计划地停下（施工 3-7 中）。拿着它的都放下了，actor 就退出。

use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use tokio::sync::{broadcast, mpsc, oneshot};

use crate::tool::{JobError, Log, Output};
use lm_kernel::kernel::event::{Event, Transient};
use lm_kernel::kernel::id::{CommandId, JobId, SessionId};
use lm_kernel::kernel::origin::By;
use lm_kernel::kernel::session::{Command, ContextLimits, Outcome, Reason};

use crate::session::backlog::Backlog;
use crate::session::jobs::Unreadable;

mod context;
mod face;
mod jobs;
mod looping;
mod meter;
pub(crate) mod parts;
mod retitle;
mod subscription;

use crate::session::shown::{Next, Shown};
pub use context::{ContextRefused, LineRefused, about_tokens, parse_context};
pub use meter::Meter;
pub use parts::{Charged, Context, Spent, SystemPart};
pub use retitle::Retitled;
use subscription::Watching;
pub use subscription::{Ended, Subscription};

/// 一个会话：它的 actor 的收件箱。可以复制，几个头一起拿着。
#[derive(Debug, Clone)]
pub struct Handle {
    id: SessionId,
    inbox: mpsc::UnboundedSender<Message>,
    /// 有没有在跑的回合：actor 每送完一批输入就写一次（施工 3-9 上）。
    busy: Arc<AtomicBool>,
    /// 在不在等人确认、回答（施工 SL-1）：和 actor 共用，照 `busy` 写。
    waiting: Arc<AtomicBool>,
    /// 一次性的会话：`miyu ask` 开的（`session.created` 的 `oneshot`）。造好以后不变（施工 C-5，`send_message` 照它和
    /// [`Handle::watched`] 决定说 `sent` 还是 `held`）。
    oneshot: bool,
    /// 拿着订阅的头有没有至少一个：actor 每多了、少了一个订阅就写一次（施工 7-9，施工 C-5 从 `busy` 的做法照抄）。
    watched: Arc<AtomicBool>,
    /// 给头看的限额和会话接下来请求的模型：造会话、载入时交完限额向内核要的（施工 6-3 补）。和 actor 共用：钉住的池出错换了
    /// 成员（施工 8-9）、回合开始重新解析（施工 8-10），actor 写一次。
    shown: Arc<Mutex<Shown>>,
    /// 主会话拿着它的会话表（L5 二，`table.rs`）：拿着主会话把手的都放下了，表和子会话跟着没了。子会话的把手没有。
    table: Option<Held>,
    /// 这个主会话的 MCP 服务（L5 六）：头照它看哪几个连着。造会话、载入交回的把手有。
    mcp: Option<HeldMcp>,
}

/// 拿着的 MCP 服务那一格：不打出里面的东西。
#[derive(Clone)]
struct HeldMcp(crate::session::table::Mcp);

impl fmt::Debug for HeldMcp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Mcp")
    }
}

/// 拿着的会话表：主会话的把手都放下了它才跟着没；界面照它按编号取子会话（[`Handle::child`]）。不打出里面的东西。
#[derive(Clone)]
struct Held {
    table: Arc<crate::session::table::Table>,
}

impl fmt::Debug for Held {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Table")
    }
}

/// 会话表找主会话用的弱把手（L5 二）：不让主会话的 actor 活过界面放下它的那一刻。
#[derive(Debug, Clone)]
pub(crate) struct WeakHandle {
    id: SessionId,
    inbox: mpsc::WeakUnboundedSender<Message>,
    busy: Arc<AtomicBool>,
    waiting: Arc<AtomicBool>,
    oneshot: bool,
    watched: Arc<AtomicBool>,
    shown: Arc<Mutex<Shown>>,
}

impl WeakHandle {
    /// 主会话还在才有：交回的把手不拿着会话表。
    pub(crate) fn upgrade(&self) -> Option<Handle> {
        Some(Handle {
            id: self.id.clone(),
            inbox: self.inbox.upgrade()?,
            busy: Arc::clone(&self.busy),
            waiting: Arc::clone(&self.waiting),
            oneshot: self.oneshot,
            watched: Arc::clone(&self.watched),
            shown: Arc::clone(&self.shown),
            table: None,
            mcp: None,
        })
    }
}

/// 不拖住会话的「重新订阅」（审计 #063）：订阅掉了队，照最后看到的序号重新订阅、把落了盘的补上。拿着它不算拿着把手：
/// 会话该停照停，停了以后重新订阅交回 [`Stopped`]。
#[derive(Debug, Clone)]
pub struct Rejoin(Option<WeakHandle>);

impl Rejoin {
    /// 同 [`Handle::subscribe_after`]。
    ///
    /// # Errors
    ///
    /// 会话停了（连同没人拿着它的把手了）。
    pub async fn subscribe_after(&self, after: u64) -> Result<(Subscription, Backlog), Stopped> {
        let handle = self
            .0
            .as_ref()
            .and_then(WeakHandle::upgrade)
            .ok_or(Stopped)?;
        handle.subscribe_after(after).await
    }

    /// 测试用：一个已经停了的会话的（审计 2026-11-18 #063 的红测试演补不上）。只在 `testkit` 开关打开时编进去。
    #[cfg(feature = "testkit")]
    #[doc(hidden)]
    #[must_use]
    pub fn gone() -> Rejoin {
        Rejoin(None)
    }
}

/// 发给 actor 的。
#[derive(Debug)]
pub(crate) enum Message {
    /// 一个命令，和等它回应的那一头。命令装在盒子里：场所消息的格多了（施工 E-4 上的 `place`）以后它比别的种类大出两百多
    /// 字节，收件箱里每一封都照最大的那一种占地方（clippy 的 `large_enum_variant`）。
    Command {
        id: CommandId,
        by: By,
        command: Box<Command>,
        reply: oneshot::Sender<Outcome>,
    },
    /// 要订阅：从这一刻起的推送都交给它。一个订阅算一个在看着的头（施工 7-9）。
    Subscribe(oneshot::Sender<Taken>),
    /// 要这一刻「当前的」几样（施工 9-6 上）：已经订阅着的头再订阅一次时，不另起订阅。
    Current(oneshot::Sender<crate::session::current::Current>),
    /// 放下了一个订阅（施工 7-9）：订阅被丢掉时由它自己送来，连同要订阅、没等到回答就不等了的。
    Unsubscribed,
    /// 有计划地停下：它的事件都落了盘，actor 退出以前交回一声。
    Stop(oneshot::Sender<()>),
    /// 删会话之前停下（施工 3-8 三补）：`force` 是假的，内核说删不了就交回原因、照常跑；删得了、或者 `force`，后台命令
    /// 整组杀掉、不记，放开日志，交回一声再退出。
    Delete {
        force: bool,
        reply: oneshot::Sender<Result<(), Reason>>,
    },
    /// 停掉派出去的任务（施工 7-4）。
    Halt(Halt),
    /// 头问上下文一块块的、累计用量照块摊（Miyu 施工 CX-1 上、CX-3，`handle/parts.rs`）。
    Context(parts::Asked),
    /// 人要求重起标题（Miyu 施工 TT-1，`handle/retitle.rs`）：同 [`Message::Command`] 送 `Command::Retitle`，另交一头等结果。
    Retitle {
        id: CommandId,
        by: By,
        reply: oneshot::Sender<Outcome>,
        done: oneshot::Sender<retitle::Retitled>,
    },
    /// 头读一条后台命令的输出（施工 7-4 补）：不进内核、不写盘。
    Output {
        job: JobId,
        reply: oneshot::Sender<Result<Output, Unreadable>>,
    },
    /// 人开、停循环（L5 四，`/loop`）：不进内核。交回停掉了没有（开的照原来有没有一个被换掉）。
    Loop(crate::session::looping::Order, oneshot::Sender<bool>),
    /// 这个窗口对这个主会话的占有（L7 一，`crate::session::window`）：造会话、载入以后交给 actor 拿着，它退出时放下。
    Window(Box<crate::session::window::Window>),
    /// 收件目录里读到的、不进内核的一样（L7 二，`crate::session::peers`）。
    Peer(crate::session::peers::Peer),
    /// 这一次运行里没人能确认、能回答（`litemiyu ask`，[`Handle::unattend`]）。
    Unattended,
}

/// 停掉派出去的任务（施工 7-4，`docs/blueprint/session/actor.md`「停掉任务」）。Miyu 另有人删了子代理（`Deleted`）：
/// LiteMiyu 不删子会话，不搬。
#[derive(Debug)]
pub(crate) enum Halt {
    /// 人停一个：`by` 是人，`cause` 是那条命令。停好了、回报落了盘才回；没有、已经结束了的回 [`JobError`]。
    One {
        job: JobId,
        by: By,
        cause: CommandId,
        reply: oneshot::Sender<Result<(), JobError>>,
    },
    /// 这个会话被父会话停下：还在跑的全停，连它们派的，都只记下、不叫醒（带 `by_model`）。后台命令那几条 `by`、`cause`
    /// 照给的。都停好了才回。
    All {
        by: By,
        cause: CommandId,
        reply: oneshot::Sender<()>,
    },
}

/// 要订阅时 actor 在同一步里交回的（施工 3-8 六补）：从这一刻起的推送，这一刻落了盘的最后一条（一条都没有是 0），日志的
/// 只读入口。补发的那一截照后两样读（[`Backlog`]）。
#[derive(Debug)]
pub(crate) struct Taken {
    pub(crate) pushes: broadcast::Receiver<Arc<Pushed>>,
    pub(crate) upto: u64,
    pub(crate) log: Log,
    /// 这一刻「当前的」几样（施工 9-6 上）。
    pub(crate) current: crate::session::current::Current,
    /// 正在流的那次请求到目前为止的块（施工 9-8 八补）：只给这个订阅先推。
    pub(crate) streaming: Vec<lm_kernel::kernel::event::Transient>,
}

impl Handle {
    pub(crate) fn new(
        id: SessionId,
        inbox: mpsc::UnboundedSender<Message>,
        busy: Arc<AtomicBool>,
        oneshot: bool,
        watched: Arc<AtomicBool>,
        shown: Arc<Mutex<Shown>>,
    ) -> Handle {
        Handle {
            id,
            inbox,
            busy,
            waiting: Arc::new(AtomicBool::new(false)),
            oneshot,
            watched,
            shown,
            table: None,
            mcp: None,
        }
    }

    /// 交回的把手拿着这个主会话的 MCP 服务那一格（L5 六）。
    pub(crate) fn with_mcp(mut self, mcp: crate::session::table::Mcp) -> Handle {
        self.mcp = Some(HeldMcp(mcp));
        self
    }

    /// 这时连着的 MCP 服务名（L5 六，`crate::mcp::Servers::live`）：变了就有新的一份。还没读过配置的（载入以后没开过回合）、
    /// 无功能的一档、子会话的把手没有。
    pub fn mcp_live(
        &self,
    ) -> Option<tokio::sync::watch::Receiver<std::collections::BTreeSet<String>>> {
        self.mcp
            .as_ref()?
            .0
            .servers
            .get()
            .map(|servers| servers.live())
    }

    /// 第一次找技能、读 MCP 服务时被忽略的那两样（LM-3补，`crate::checkup`；配置项不在这里，在
    /// [`crate::configured::Loaded::ignored`]）：造会话交回时已经有了；载入的会话第一次开回合以后才有（`None` 是还没读）。
    /// 子会话的把手没有。
    pub fn ignored(&self) -> Option<tokio::sync::watch::Receiver<Option<crate::checkup::Ignored>>> {
        Some(self.mcp.as_ref()?.0.ignored.subscribe())
    }

    /// 主会话拿着它的会话表（L5 二）。
    pub(crate) fn with_table(mut self, table: Arc<crate::session::table::Table>) -> Handle {
        self.table = Some(Held { table });
        self
    }

    /// 主会话拿着的会话表；子会话、没有会话表的没有。
    pub(crate) fn table(&self) -> Option<&Arc<crate::session::table::Table>> {
        self.table.as_ref().map(|held| &held.table)
    }

    /// 弱的把手：会话表找主会话用。
    pub(crate) fn downgrade(&self) -> WeakHandle {
        WeakHandle {
            id: self.id.clone(),
            inbox: self.inbox.downgrade(),
            busy: Arc::clone(&self.busy),
            waiting: Arc::clone(&self.waiting),
            oneshot: self.oneshot,
            watched: Arc::clone(&self.watched),
            shown: Arc::clone(&self.shown),
        }
    }

    /// 不拖住这个会话的重新订阅（[`Rejoin`]）。
    pub fn rejoin(&self) -> Rejoin {
        Rejoin(Some(self.downgrade()))
    }

    /// 主会话派的一个子会话（连子会话再派的）的把手（界面线第二趟）：界面要订阅它、确认它的高危命令、看它的时间线。只认这次
    /// 运行里派出去、还在会话表里的；主会话自己、子会话的把手（没有会话表）、别的会话交回 `None`。
    pub async fn child(&self, session: &SessionId) -> Option<Handle> {
        self.table.as_ref()?.table.child(session).await
    }

    /// 有没有在跑的回合：核心看它决定能不能空闲退出（施工 3-9 上）。会话停了的，不算在跑。
    pub fn busy(&self) -> bool {
        self.busy.load(Ordering::Acquire)
    }

    /// 一次性的会话：`miyu ask` 开的（施工 C-5，`cross-session.md` 第三条第 4 款）。
    pub fn oneshot(&self) -> bool {
        self.oneshot
    }

    /// 这时有没有至少一个头订阅着（施工 C-5）：造会话、载入以后是假的，和内核一样当没人看着。
    pub fn watched(&self) -> bool {
        self.watched.load(Ordering::Acquire)
    }

    /// 会话编号。
    pub fn id(&self) -> &SessionId {
        &self.id
    }

    /// 给头看的限额：窗口、压缩线（施工 6-3 补）。协议照它回 `subscribe`（`docs/blueprint/protocol.md`）。会话中途变了的
    /// 是变了以后的（施工 8-9、8-10）。
    pub fn limits(&self) -> ContextLimits {
        self.shown
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .limits
    }

    /// 会话接下来请求的模型（施工 8-10）：引用、接下来发给谁。协议照它写 `subscribe` 回应的 `model`。回合开始重新解析过、
    /// 出错换了成员的是换了以后的。
    pub fn next(&self) -> Next {
        self.shown
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .next
            .clone()
    }

    /// 发一个命令，等它的回应：接受的，它产生的事件落了盘才回（`07-存储.md` S4）；拒绝的当场回。
    /// 编号 `id` 由发命令的一方生成，同一个编号只生效一次（`02-内核.md` 不变量 9）。
    ///
    /// # Errors
    ///
    /// 会话停了：写不进去、出了 bug、有计划地停下了。
    pub async fn command(
        &self,
        id: CommandId,
        by: By,
        command: Command,
    ) -> Result<Outcome, Stopped> {
        let (reply, answer) = oneshot::channel();
        self.send(Message::Command {
            id,
            by,
            command: Box::new(command),
            reply,
        })?;
        answer.await.map_err(|_| Stopped)
    }

    /// 这一刻「当前的」几样（施工 9-6 上）：已经订阅着、再订阅一次（协议上「还是那一个」）的，回应照它答。和那个订阅不在
    /// 同一步：头已经收着推送，这一份可能比它收到的新一点。
    ///
    /// # Errors
    ///
    /// 会话停了。
    pub async fn current(&self) -> Result<crate::session::current::Current, Stopped> {
        let (reply, answer) = oneshot::channel();
        self.send(Message::Current(reply))?;
        answer.await.map_err(|_| Stopped)
    }

    /// 订阅：从这一刻起，落了盘的事件和瞬时事件照先后交过来。在发命令之前订阅的，这个命令产生的
    /// 事件一定先于它的回应到（`04-核心协议.md` 第六节第 2 条）。
    ///
    /// 拿着订阅就算一个头在看着这个会话：没人看着的一次性会话，回报只记下、不叫醒她（`agents.md` 第三条第 3 条，施工
    /// 7-9）。订阅放下了（丢掉它、连接断了），自己告诉 actor。
    ///
    /// # Errors
    ///
    /// 会话停了。
    pub async fn subscribe(&self) -> Result<Subscription, Stopped> {
        self.take().await.map(|(subscription, ..)| subscription)
    }

    /// 订阅，连同补发之前的事件（施工 3-8 六补，协议的 `subscribe` 带 `after`）：交回的订阅从这一刻起推，[`Backlog`] 是日志里
    /// 序号大于 `after`、这一刻落了盘的那一截，两样在 actor 的同一步里拿，接得上、不重不漏。读那一截由拿着的一方去读，不占
    /// actor。别的同 [`Handle::subscribe`]。
    ///
    /// # Errors
    ///
    /// 会话停了。
    pub async fn subscribe_after(&self, after: u64) -> Result<(Subscription, Backlog), Stopped> {
        let (subscription, upto, log) = self.take().await?;
        Ok((subscription, Backlog::new(log, after, upto)))
    }

    /// 向 actor 要一个订阅，连同它在同一步里交回的：落了盘的最后一条、日志的只读入口。
    async fn take(&self) -> Result<(Subscription, u64, Log), Stopped> {
        let (reply, answer) = oneshot::channel();
        self.send(Message::Subscribe(reply))?;
        // 送进去了才算数：等回答的时候不等了（这个 future 被丢掉），它照样放下、告诉 actor，一来一去对得上。
        let watching = Watching(self.inbox.downgrade());
        let Taken {
            pushes,
            upto,
            log,
            current,
            streaming,
        } = answer.await.map_err(|_| Stopped)?;
        let subscription = Subscription::new(pushes)
            .watched(watching, current)
            .catching_up(streaming);
        Ok((subscription, upto, log))
    }

    /// 这一次运行里没人能确认、能回答（`litemiyu ask`）：之后要问人的当场拒、提问当场跳过，结果是内核现成的那两句
    /// （`core/tool-results/unattended`、`question-unattended`）。快照、请求都不变，不记事件；只管这一个 actor，派出去的子会话
    /// 各自交。
    ///
    /// # Errors
    ///
    /// 会话已经停了。
    pub fn unattend(&self) -> Result<(), Stopped> {
        self.send(Message::Unattended)
    }

    /// 有计划地停下：送进「要重启了」，等它产生的事件落了盘，actor 退出（`02-内核.md` 第六节
    /// 「载入、崩溃、重启」第 3 条）。再载入时，被打断的那一轮接着干。
    ///
    /// # Errors
    ///
    /// 会话已经停了。
    pub async fn stop(&self) -> Result<(), Stopped> {
        let (reply, answer) = oneshot::channel();
        self.send(Message::Stop(reply))?;
        answer.await.map_err(|_| Stopped)
    }

    /// 删会话之前停下（施工 3-8 三补，`docs/blueprint/session/actor.md` 第 9 条）：有回合在进行、正在读回日志、改回文件的
    /// 删不了，交回内核说的原因，会话照常；删得了的，这个会话在跑的后台命令整组杀掉、不记回报，什么都不再写，放开日志的
    /// 文件，actor 退出。交回的时候日志已经关了：会话表接着就挪它的目录（Windows 上开着的文件挪不走）。
    ///
    /// # Errors
    ///
    /// 会话已经停了。
    pub async fn delete(&self) -> Result<Result<(), Reason>, Stopped> {
        self.stop_for_deletion(false).await
    }

    /// 同 [`Handle::delete`]，只是不问删不删得了：父会话被删，派出去的子会话不管在不在忙，一起停下（`agents.md` 第七条
    /// 第 5 条）。在路上的请求叫停、在跑的工具掐掉，跑到一半的回合不收尾。
    ///
    /// # Errors
    ///
    /// 会话已经停了。
    pub async fn discard(&self) -> Result<(), Stopped> {
        // 不问删不删得了，actor 就不会说删不了：里面那一层总是 `Ok`。
        self.stop_for_deletion(true).await.map(|_| ())
    }

    async fn stop_for_deletion(&self, force: bool) -> Result<Result<(), Reason>, Stopped> {
        let (reply, answer) = oneshot::channel();
        self.send(Message::Delete { force, reply })?;
        answer.await.map_err(|_| Stopped)
    }

    fn send(&self, message: Message) -> Result<(), Stopped> {
        self.inbox.send(message).map_err(|_| Stopped)
    }
}

/// 推给订阅者的一份。
#[derive(Debug, Clone, PartialEq)]
pub enum Pushed {
    /// 落了盘的几条事件，照先后。
    Events(Vec<Event>),
    /// 一条瞬时事件：不落盘（`03-事件模型.md` 第五节）。
    Transient(Transient),
}

/// 会话停了：写不进去、出了 bug、有计划地停下了，或者没人拿着了。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stopped;

impl fmt::Display for Stopped {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "the session stopped")
    }
}

impl std::error::Error for Stopped {}

#[cfg(test)]
mod tests;
