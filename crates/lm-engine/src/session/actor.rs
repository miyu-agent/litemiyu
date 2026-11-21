//! actor 本身（`docs/designs/02-内核.md` 第七节「会话 actor 怎么跑」）：一个会话一个异步任务。
//!
//! 人的命令、执行器的回报进收件箱，一条一条送进内核，内核交出的动作照 02 第四节「执行器怎么回动作」的表回。当场就能回的
//! （落盘了、挂接点跑完了）放进本地的队列，先于收件箱里的送：和执行器替身（`miyu-kernel` 的 `testkit`）一个先后。

use std::collections::{BTreeMap, VecDeque};
use std::io;
use std::sync::{Arc, Mutex, atomic::AtomicBool};
use std::time::Instant;

use tokio::sync::{broadcast, mpsc, oneshot};

use lm_kernel::kernel::event::{Event, Purpose};
use lm_kernel::kernel::id::{CommandId, Seq};
use lm_kernel::kernel::session::{Action, Input, Limits, Outcome, Session};

use crate::session::blocking::blocking;
use crate::session::clock::Clock;
use crate::session::config::Turning;
use crate::session::guard::Guard;
use crate::session::handle::{Message, Pushed};
use crate::session::jobs::SessionJobs;
use crate::session::kinds;
use crate::session::port::{Back, ModelPort};
use crate::session::shown::{Next, Shown};
use crate::session::store::Store;
use crate::session::tools::{ToolKit, Tools};
use durable::Syncing;

mod back;
mod durable;
mod halt;
mod kit;
mod life;
mod listing;
mod looping;
mod mail;
mod mcp;
mod memory;
mod model;
mod parts;
mod peers;
mod project_docs;
mod retitle;
mod run;
mod shelf;
mod skills;
mod stop;
mod store;

/// 任务表里这个会话的那一份要的（施工 7-3；施工 7-4 挪到 `jobs.rs`）。
pub(crate) use crate::session::jobs::Kit as JobKit;
pub(crate) use kit::PolicyKit;
pub(crate) use life::spawn;
use mail::Mail;
pub(crate) use mcp::McpKit;

/// 推送的队列：一个会话最多攒这么多份还没被读走的。读得慢的订阅者被挤掉，掉了队（`04-核心协议.md` 第七节）。
pub(crate) const PUSH_QUEUE: usize = 1024;

/// 一个会话的 actor：内核的状态机，和回它的动作要用的。
pub(crate) struct Actor {
    /// 这个窗口对这个主会话的占有（L7，`crate::session::window`）：造会话、载入以后交进来；actor 退出时放下。子会话没有。
    /// 放在最前面：先于推送的通道放下，订阅的头看到会话停了的时候锁已经放了。
    window: Option<crate::session::window::Window>,
    session: Session,
    /// 写盘的地方：写的时候挪进阻塞线程，写完拿回来。
    store: Option<Box<dyn Store>>,
    model: Arc<dyn ModelPort>,
    /// 人的命令：拿着 `Handle` 的都放下了，它就关了。
    inbox: mpsc::UnboundedReceiver<Message>,
    /// 执行器的回报：请求的回报、到点了。
    back: mpsc::UnboundedReceiver<Back>,
    /// 交给执行器、往 `back` 里送的那一头。actor 自己拿着一份，`back` 不会自己关。
    backs: mpsc::UnboundedSender<Back>,
    pushes: broadcast::Sender<Arc<Pushed>>,
    /// 等着回应的命令：同一个编号来了几次，照先后排着。事件写进去了就交，不扣着等同步（速度线一，`durable.rs`）。
    replies: BTreeMap<CommandId, VecDeque<oneshot::Sender<Outcome>>>,
    /// 人要求重起标题、等结果的那几头（Miyu 施工 TT-1～TT-3，`retitle.rs`）：命令编号和交结果的那一头。
    retitles: Vec<(CommandId, oneshot::Sender<crate::session::handle::Retitled>)>,
    /// 后台同步的账、句柄（施工 V-19，`actor/durable.rs`）：和后台同步的阻塞线程共用。
    sync: Arc<Mutex<Syncing>>,
    /// 交回了「落盘了」、还没写进日志的几批（速度线二，`store.rs` 的 `append`）：同步在路上时不用同步的那几批攒在这里，
    /// 同步做完了一次写进去。程序被杀时丢的只有这些。
    unwritten: Vec<Event>,
    /// 攒着的那几批引用的 blob 登记到哪一个（速度线三，`Unsynced::latest`）：写进去以前，到它为止的都要同步了。
    unwritten_needs: Option<u64>,
    /// 一串后台同步做完、出错报回来的。actor 自己拿着送的那一头，通道不会自己关。
    synced: mpsc::UnboundedReceiver<Synced>,
    /// 交给后台同步、往 `synced` 里送的那一头。
    syncs: mpsc::UnboundedSender<Synced>,
    /// 还没说完的请求：叫停它的那一头，和交给端口的那一刻（算用时）。
    calls: BTreeMap<Seq, oneshot::Sender<()>>,
    /// 还没说完的辅助请求（施工 3-8 四补的回顾、五补的起标题，`model.rs`）：用途、照到第几条、叫停它的那一头（拿着不用：
    /// actor 停了放下它，请求跟着停）、交给端口的那一刻。一种用途至多一个。
    asides: Vec<(Purpose, Seq, oneshot::Sender<()>, Instant)>,
    clock: Clock,
    /// 执行工具的端口（施工 4-2）。
    tools: Tools,
    /// 任务表里这个会话的那一份（施工 7-3）：丢掉时整组杀掉还在跑的后台命令。
    jobs: SessionJobs,
    /// 向上回报（施工 7-6，`report.rs`）：有父会话的子会话才有。
    reporter: Option<crate::session::report::Reporter>,
    /// 会话表里子代理一共用掉的（`table.rs` 的 `AgentsSpent`）：表里的子会话每写一批日志加上这一批，主会话写概要时照它记。
    /// 主会话、没有会话表的没有。
    agents: Option<crate::session::table::AgentsSpent>,
    /// 执行前的链：权限策略（施工 4-3 下）。在阻塞线程里判，所以放在 `Arc` 里交过去（施工 4-9 再补四下）。
    guard: Arc<Guard>,
    /// 在等扩展守卫的调用（施工 HK-1，`guarding.rs`）；结果落盘后要发通知的直接调用（施工 HK-2，`after.rs`）。
    /// 有没有在跑的回合，和 `Handle` 共用：每送完一批输入写一次；actor 退出了写成没有（施工 3-9 上）。
    busy: Arc<AtomicBool>,
    /// 在不在等人确认、回答（施工 SL-1），和 `Handle` 共用：照 `busy` 每送完一批写一次。
    waiting: Arc<AtomicBool>,
    /// 配置（施工 8-4）：从哪取，和这一轮的那一份；回合开始时换，这一轮的请求都照它。
    config: Turning,
    /// 拿着订阅的头有几个（施工 7-9）：从没有到有、从有到没有时交内核 `Watched`。造会话、载入时是 0，和内核一样当没人看着。
    watchers: usize,
    /// 这时有没有至少一个头订阅着，和 `Handle` 共用（施工 C-5，照 `busy` 的做法）：`watchers` 从 0 到有、从有到 0 时写一次。
    /// `send_message` 发给这个会话的时候，别的会话照它和这个会话是不是一次性的，决定说 `sent` 还是 `held`。
    watched: Arc<AtomicBool>,
    /// 上一次交给内核的限额（施工 8-9，`model.rs`）：请求说完了、回合开始重新解析完和端口的比，变了再交。
    handed: Limits,
    /// 给头看的限额和会话接下来请求的模型，和 `Handle` 共用（施工 8-9、8-10）：交了新的限额、回合开始解析完写一次。
    shown: Arc<Mutex<Shown>>,
    /// 无功能的一档（设计第五节第 14 条，快照的 `no_tools`）：回合开始什么都不找、不注入。造会话、载入时照快照交。
    bare: bool,
    /// 应急模式（设计第五节第 16 条）：日志只在内存里（`store/memory.rs`），不交记忆、技能。
    rescue: bool,
    /// 这次运行的循环（L5 四，`actor/looping.rs`）：只在内存里，actor 退出就停。
    looping: crate::session::looping::LoopRun,
    /// 载入时认出上一次关掉时循环还开着：跑起来先记一行（`actor/looping.rs`）。
    loop_left: bool,
    /// MCP 服务（L5 六，`actor/mcp.rs`）：回合开始照它换工具面。无功能的一档没有。
    mcp: Option<mcp::McpKit>,
    /// 这时的策略快照和存它的 blob（`actor/kit.rs`）：换 MCP 工具面、换压缩线、上下文明细照它。造会话、载入时交。
    kit: Option<kit::PolicyKit>,
    /// 上一次看技能时人放技能的文件夹的指纹（L5 五补，`actor/skills.rs`）：载入的会话第一轮以前没有。
    skills: Option<crate::skills::watch::Stamp>,
    /// 收成槽的历史上一次从日志读不回来，为什么（红线 5，`actor/shelf.rs`）：读回来了就没有。
    unread: Option<String>,
}

/// 会话停了：写不进去。
pub(crate) struct Stop;

impl Actor {
    /// 一个 actor：会话的状态机、写盘的地方、请求模型的端口、工具目录和替工具写的两句、任务表、权限策略、收件箱、时钟、
    /// 配置。
    #[expect(
        clippy::too_many_arguments,
        reason = "造 actor 的几样各不相干，拼成一个结构体也只是换个地方列"
    )]
    pub(crate) fn new(
        session: Session,
        store: Box<dyn Store>,
        model: Arc<dyn ModelPort>,
        tools: ToolKit,
        jobs: JobKit,
        guard: Guard,
        inbox: mpsc::UnboundedReceiver<Message>,
        clock: Clock,
        config: Turning,
    ) -> Actor {
        let (backs, back) = mpsc::unbounded_channel();
        let (syncs, synced) = mpsc::unbounded_channel();
        let tools = Tools::new(tools, backs.clone());
        let jobs = SessionJobs::new(jobs, backs.clone());
        let (pushes, _) = broadcast::channel(PUSH_QUEUE);
        let busy = Arc::new(AtomicBool::new(!session.idle()));
        // 造会话、载入时已经把端口的限额交给了内核（`open.rs`）：记下交的是哪一份。
        let handed = model.limits();
        let shown = Arc::new(Mutex::new(Shown::new(
            session.context_limits(),
            Next::of(&*model),
        )));
        Actor {
            session,
            store: Some(store),
            model,
            inbox,
            back,
            backs,
            pushes,
            replies: BTreeMap::new(),
            retitles: Vec::new(),
            sync: Arc::new(Mutex::new(Syncing::new().with_blobs(tools.unsynced()))),
            unwritten: Vec::new(),
            unwritten_needs: None,
            synced,
            syncs,
            calls: BTreeMap::new(),
            asides: Vec::new(),
            clock,
            tools,
            jobs,
            reporter: None,
            agents: None,
            guard: Arc::new(guard),
            busy,
            waiting: Arc::new(AtomicBool::new(false)),
            config,
            watchers: 0,
            watched: Arc::new(AtomicBool::new(false)),
            handed,
            shown,
            bare: false,
            rescue: false,
            looping: crate::session::looping::LoopRun::default(),
            loop_left: false,
            mcp: None,
            kit: None,
            skills: None,
            unread: None,
            window: None,
        }
    }

    /// 向上回报交给 `reporter`（施工 7-6）：子会话造好、载入时交。
    pub(crate) fn set_reporter(&mut self, reporter: Option<crate::session::report::Reporter>) {
        self.reporter = reporter;
    }

    /// 子代理一共用掉的那一份（[`Actor::agents`] 那一格）：子会话造好、载入时交。
    pub(crate) fn set_agents(&mut self, agents: Option<crate::session::table::AgentsSpent>) {
        self.agents = agents;
    }

    /// 是不是无功能的一档（[`Actor::bare`] 那一格）。
    pub(crate) fn set_bare(&mut self, bare: bool) {
        self.bare = bare;
    }

    /// 是不是应急模式的会话（[`Actor::rescue`] 那一格）。
    pub(crate) fn set_rescue(&mut self, rescue: bool) {
        self.rescue = rescue;
    }

    /// 命令 `id` 在等回应：造会话的那一个，在 actor 跑起来之前就在等。
    pub(crate) fn wait_for(&mut self, id: CommandId, reply: oneshot::Sender<Outcome>) {
        self.replies.entry(id).or_default().push_back(reply);
    }

    /// 跑：先回造会话、载入吐出来的动作，再一封封收。执行器的回报先收，读流不断。
    pub(crate) async fn run(mut self, first: Vec<Action>) {
        if self.settle(first).await.is_err() {
            return;
        }
        // 上一次关掉时循环还开着（L5 四）：先记一行告诉人。
        if let Some(line) = self.loop_left_line()
            && self.drain(VecDeque::from([line])).await.is_err()
        {
            return;
        }
        loop {
            let input = tokio::select! {
                biased;
                Some(back) = self.back.recv() => match self.back(back) {
                    Some(input) => input,
                    None => continue,
                },
                // 后台同步做完了（施工 V-19）：出错的停下；攒着的写进去（速度线二）。
                Some((_, result)) = self.synced.recv() => match self.after_sync(result).await {
                    Ok(()) => continue,
                    Err(Stop) => return,
                },
                message = self.inbox.recv() => match self.post(message).await {
                    Err(Stop) => return,
                    Ok(Some(Mail::Input(input))) => *input,
                    Ok(Some(Mail::Done)) => continue,
                    Ok(Some(Mail::Halt(halt))) => match self.halt(halt).await {
                        Ok(()) => continue,
                        Err(Stop) => return,
                    },
                    Ok(Some(Mail::Stop(reply))) => return self.stop(reply).await,
                    Ok(Some(Mail::Delete(force, reply))) => match self.delete(force, reply).await {
                        true => return,
                        false => continue,
                    },
                    Ok(None) => {
                        self.settle_sync().await;
                        return;
                    }
                },
            };
            if self.drain(VecDeque::from([input])).await.is_err() {
                return;
            }
        }
    }

    /// 回一串动作，再把当场回的输入送进去。
    async fn settle(&mut self, actions: Vec<Action>) -> Result<(), Stop> {
        let mut inputs = VecDeque::new();
        for action in actions {
            inputs.extend(self.act(action).await?);
        }
        self.drain(inputs).await
    }

    /// 一条条送进内核，回每个动作；当场回的输入排在后面接着送，直到没有。
    async fn drain(&mut self, mut inputs: VecDeque<Input>) -> Result<(), Stop> {
        // 这一批全是流式的碎片（施工 V-22）：送进去的、内核交回的都是碎的（`kinds::chatty_input`、`chatty_action`），历史、
        // 账本、限额、策略都没动，有没有能压的不会变，收尾不用重算（它要过一遍整段有效历史，以前流式时每个字算一次）。
        let mut fragments = true;
        while let Some(input) = inputs.pop_front() {
            let chatty = kinds::chatty_input(&input);
            fragments &= chatty;
            // 闲下来收成了槽的，要看历史以前先读回来（红线 5，`actor/shelf.rs`）。
            self.unshelve_for(&input).await?;
            let actions = self.session.handle(input);
            // 内核一收下就写「有没有在跑的回合」：回应在批中间就发出去了，回合开头慢的那一阵不能算闲着（`tests/busy.rs`）。
            self.settle_busy();
            for action in actions {
                fragments &= kinds::chatty_action(&action);
                inputs.extend(self.act(action).await?);
            }
        }
        // 交进去的后台命令结束都落了盘，才从任务表里拿掉（施工 7-3）。
        self.jobs.land();
        self.settle_busy();
        self.push_now(!fragments);
        self.after_batch();
        // 循环欠着一次、撤回处理了的补交（LM-34，`actor/looping.rs`）：只在送完一批时看。
        self.loop_settled();
        self.rest();
        Ok(())
    }

    /// 照 02 第四节的表回一个动作；当场就能回的，交回要送进内核的输入。
    async fn act(&mut self, action: Action) -> Result<Option<Input>, Stop> {
        Ok(match action {
            Action::Append(events) => {
                let title = self.retitle_in(&events);
                let input = self.append(events).await?;
                self.retitle_landed(title);
                input
            }
            shell @ (Action::RunShell { .. } | Action::HaltShell { .. }) => {
                self.tools.person(shell)
            }
            Action::Reply { id, outcome } => {
                self.retitle_replied(&id, &outcome);
                self.reply(id, outcome);
                None
            }
            Action::Push(events) => {
                self.push(Pushed::Events(events));
                None
            }
            Action::RetitleFailed { failed, .. } => {
                self.retitle_failed(failed);
                None
            }
            Action::PushTransient(transient) => {
                self.push(Pushed::Transient(transient));
                None
            }
            // 回合开始（`turn.started` 已经落了盘）：冻结这一轮的配置，重新解析会话的引用，问挂接点（施工 8-4、8-10、R-4 上，`model.rs`）。
            Action::RunTurnStartHooks {
                turn,
                model,
                present,
                ..
            } => Some(self.turn_start(turn, model, present).await),
            Action::CallModel { seen, request, .. } => {
                self.call(seen, request);
                None
            }
            alarm @ (Action::Wake { .. } | Action::ExpireApproval { .. }) => {
                self.wake(alarm);
                None
            }
            Action::Aside {
                purpose,
                upto,
                request,
            } => {
                self.aside(purpose, upto, request);
                None
            }
            Action::CancelModel { seen } => {
                self.cancel(seen);
                None
            }
            // 替看不了图的模型看图（施工 8-17，`model.rs`）：交给端口，结果另走一路送回。
            Action::Describe { blob, request } => {
                self.describe(blob, request);
                None
            }
            // 回合结束：判过却没派出去的真实位置扔掉（施工 5-18）。
            Action::RunTurnEndHooks { .. } => {
                self.tools.forget_checked();
                None
            }
            Action::GuardTool {
                call_id,
                name,
                args,
                cwd,
                ..
            } => {
                // 判要碰磁盘（换真实的位置）：在阻塞线程里判，不占跑异步任务的线程，慢盘上只让这个会话自己等。
                let guard = Arc::clone(&self.guard);
                let (verdict, checked) = blocking(move || guard.judge(&name, args, cwd)).await;
                // 判过的真实位置派的时候交给工具（施工 5-18）。
                self.tools.checked(call_id, checked);
                Some(Input::ToolGuarded {
                    at: self.clock.now(),
                    call_id,
                    verdict,
                })
            }
            action @ Action::RunTool { .. } => self.run_tool(action), // `actor/run.rs`
            // 叫它停（施工 4-9 再补一）：只举旗，工具交回来照常送回。
            Action::StopTool { call_id } => {
                self.tools.stop(call_id);
                None
            }
            Action::CancelTool { call_id } => {
                self.tools.cancel(call_id);
                None
            }
            // 压完重读（施工 6-5）：当场在阻塞线程里读完、存成 blob，再做下一个动作（它后面紧跟着摘要请求）。
            Action::Reread { seen, paths, limit } => {
                let files = self.tools.reread(paths, limit).await;
                Some(Input::Reread {
                    at: self.clock.now(),
                    seen,
                    files,
                })
            }
            // 撤掉压缩时读回日志（施工 6-9）：当场在阻塞线程里读完再收收件箱；读不了的，会话停下。
            Action::ReadBack { from } => Some(self.read_back(from).await?),
            // 取回原文（施工 6-9）：当场照 blob 读完，排在收件箱里别的前面送回去。
            Action::Recall { blobs } => Some(Input::Recalled {
                texts: self.tools.recall(blobs).await,
            }),
            // 停掉撤掉的那几轮派出去的、太吵的监视命令（施工 7-8、MN-1 上，`halt.rs`）：不送回，回报照停好了的样子另外交来。
            Action::StopJobs {
                jobs,
                by,
                cause,
                reason,
            } => {
                self.stop_jobs(jobs, by, cause, reason).await;
                None
            }
            // 向上回报（施工 7-6）：交给交回报的任务，不等。没有父会话的（主会话）内核不会交。
            Action::Report(upward) => {
                if let Some(reporter) = &self.reporter {
                    reporter.send(upward);
                }
                None
            }
            // 回答落了盘，送给在等的工具（施工 D-2，`tools/questions.rs`）。
            Action::AnswerTool { call_id, answers } => {
                self.tools.answer(call_id, answers);
                None
            }
        })
    }
}

/// 一串后台同步做完、出错报回来的：同步到哪一条、成没成（施工 V-19，`actor/store.rs` 的 `dispatch`）。
type Synced = (Seq, io::Result<()>);

/// 交回一声。等的那一头不等了，就没人收。
#[expect(
    clippy::let_underscore_must_use,
    reason = "等的那一头不等了：回的话没人要，丢掉"
)]
fn answer<T>(reply: oneshot::Sender<T>, value: T) {
    let _ = reply.send(value);
}

#[cfg(test)]
mod tests;
