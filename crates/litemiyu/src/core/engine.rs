//! 引擎线程（`docs/00-设计.md` 第四节第 1 条）：一个单线程的异步运行时，收界面的命令，直接调 `lm-engine`；会话推来的事件
//! 当场读成 [`Push`](super::Push) 交回界面。
//!
//! 用到才起：线程在界面第一次要它时才起（[`super::Core`]），配置、模型目录、工具、会话都在第一次发话时才造（[`setup`]）。

mod children;
mod forward;
mod handing;
mod looping;
mod models;
mod readback;
mod retitle;
mod sessions;
mod setup;
mod undo;
mod view;

use std::sync::Arc;
use std::thread;
use std::time::Duration;

use lm_engine::session::Handle;
use lm_kernel::kernel::block::{Block, Text};
use lm_kernel::kernel::id::{CallId, CommandId, Seq};
use lm_kernel::kernel::origin::{By, Person};
use lm_kernel::kernel::session::{Command as Kernel, Outcome, Queued, Reason};
use tokio::sync::mpsc;

use super::{Command, Deliver, Update};

/// 引擎线程、阻塞线程的栈：会话、路由、工具都跑在运行时的任务里，大块的活在阻塞线程里做；照 tokio 工作线程的默认（2 MB）少
/// 16 KB。
///
/// 少的那 16 KB（LM-6 量的）：musl 照「栈 + 线程局部存储」整页映射一块可读写的（2 MB 的栈是 2052 KB），比 2 MB 大一点，
/// 落在地址上时常整个盖住一段 2 MB 对齐的范围；系统开着透明大页（`/sys/kernel/mm/transparent_hugepage/enabled` 是 `always`，
/// 本机就是）时，栈顶头一次碰到那一段，内核就拿一整个 2 MB 的大页给它。引擎线程平时只用到栈顶一百来 KB，进程一直多占 2 MB
/// 实打实的内存（`AnonHugePages: 2048`）：musl 版本机说一句、闲 4 秒，十次里九次撞上，PSS 多 2 MB；少 16 KB 以后十次一次
/// 没有（LM-6 量的）。映射比 2 MB 小就盖不住对齐的一整段，内核不给大页。glibc 的栈另有摆法，量下来没撞上过；
/// 两边用同一个数。
const STACK: usize = 2 * 1024 * 1024 - 16 * 1024;

/// 阻塞线程闲多久就退（设计第四节第 1 条「要用才起，闲了就退」）。tokio 默认 10 秒：一轮说完、闲下来以后两条阻塞线程还
/// 留着，各占一份栈和一个分配区。
const BLOCKING_KEEP_ALIVE: Duration = Duration::from_secs(2);

/// 运行时一次从系统收多少个 I/O 事件（占用 四 A7）。tokio 默认 1024，事件的数组一直留着（1024 × 12 字节）；这个进程同时开着的
/// 描述符只有几个（模型那一条连接、几个子进程的管道、窗口之间的收件口），一次收不满 64 个，收不完的下一圈接着收。
const IO_EVENTS_PER_TICK: usize = 64;

/// 最后一份推送过去多久算安静下来了（Miyu 核心 `settle.rs` 的 `QUIET`）。
const QUIET: Duration = Duration::from_secs(2);

/// 起引擎线程，交回给它发命令的那一头，和线程退出时断开的那一头（从不发东西：运行时放下了、会话都收完尾了才断开；界面
/// 退出时照它等，`super::Core` 的 `Drop`）。线程起不来的照「起不来」报给界面，断开的那一头当场断开。
pub fn spawn(
    deliver: Deliver,
    bare: bool,
    rescue: Option<crate::rescue::Prepared>,
) -> (
    mpsc::UnboundedSender<Command>,
    std::sync::mpsc::Receiver<()>,
) {
    let (commands, inbox) = mpsc::unbounded_channel();
    let (finished, ended) = std::sync::mpsc::channel::<()>();
    let told = Arc::clone(&deliver);
    let started = thread::Builder::new()
        .name("engine".into())
        .stack_size(STACK)
        .spawn(move || {
            run(inbox, deliver, bare, rescue);
            drop(finished);
        });
    if let Err(error) = started {
        told(Update::Failed(format!("engine thread: {error}")));
    }
    (commands, ended)
}

/// 线程里跑的：造运行时，一条条收命令，界面那一头关了就停下会话、退出。
fn run(
    inbox: mpsc::UnboundedReceiver<Command>,
    deliver: Deliver,
    bare: bool,
    rescue: Option<crate::rescue::Prepared>,
) {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .thread_keep_alive(BLOCKING_KEEP_ALIVE)
        .thread_stack_size(STACK)
        .max_io_events_per_tick(IO_EVENTS_PER_TICK)
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            deliver(Update::Failed(format!("engine runtime: {error}")));
            return;
        }
    };
    let mut engine = Engine::new(deliver);
    engine.bare = bare.then_some(true);
    engine.rescue = rescue;
    runtime.block_on(engine.serve(inbox));
}

/// 引擎这一头记着的。
struct Engine {
    /// 交回界面。
    deliver: Deliver,
    /// 造会话要的几样：第一次发话时造好，之后一直用。
    ready: Option<setup::Ready>,
    /// 正在说话的会话：`/new` 以后、还没发话时没有。
    session: Option<Handle>,
    /// 把它推来的交回界面的那个任务：换会话时先掐掉，旧会话的推送不会排在新会话的后面。
    forwarder: Option<tokio::task::AbortHandle>,
    /// 读回以后安静下来还堆的那个任务（`readback.rs`）：又读回了就掐掉重排。
    settling: Option<tokio::task::AbortHandle>,
    /// 下一个新开的会话开哪一档（[`Command::New`]）：`None` 照配置的默认。
    bare: Option<bool>,
    /// 推送里派出去的子会话的编号交回这里订阅（`children.rs`）：转推送的那几个任务拿着发的一头。
    agents: (
        mpsc::UnboundedSender<String>,
        mpsc::UnboundedReceiver<String>,
    ),
    /// 已经订阅着的子会话：一个只订阅一次。
    watched: std::collections::BTreeSet<String>,
    /// 人切进去看着的子会话（`view.rs`）：说话、打断这些交给它。
    viewing: Option<view::Viewing>,
    /// 应急模式备好的：第一次发话时交给 [`setup::Ready::new`]（之后它记着）。
    rescue: Option<crate::rescue::Prepared>,
    /// 正在说话的会话的日志只读入口（`readback.rs`）：读回正文放掉的一段照它取一截。
    reader: Option<lm_engine::session::Backlog>,
    /// 只读打开着的、日志中间坏了一行的会话的编号（`sessions.rs` 的 `view_broken`）：没有 `session`，要会话办的都拒。
    read_only: Option<String>,
    /// 最后要的那一次读回、列你说的话（`readback.rs`）：下一次读完了先等它交了再交，交回照要的先后。
    handed: Option<tokio::task::JoinHandle<()>>,
    /// 这一次运行里没人能确认（[`Command::Unattended`]）：开的会话、订阅的子会话都交给它。
    unattended: bool,
    /// 还没开会话时 `/effort` 选的（`models.rs`）：开了会话交上。
    pending_effort: Option<Option<String>>,
    /// 应急模式下 `/model` 选的（`models.rs` 的 `chat_default`）：不写配置，只记在这里，这一次运行里以后开的会话照它开。
    /// 平常没有（记进配置的 `models.chat`）。
    rescue_chat: Option<String>,
}

impl Engine {
    fn new(deliver: Deliver) -> Self {
        Self {
            deliver,
            ready: None,
            session: None,
            forwarder: None,
            settling: None,
            bare: None,
            agents: mpsc::unbounded_channel(),
            watched: std::collections::BTreeSet::new(),
            viewing: None,
            rescue: None,
            reader: None,
            read_only: None,
            handed: None,
            unattended: false,
            pending_effort: None,
            rescue_chat: None,
        }
    }

    /// 一条条收命令，界面那一头关了（退出）就停下会话。
    async fn serve(mut self, mut inbox: mpsc::UnboundedReceiver<Command>) {
        loop {
            tokio::select! {
                command = inbox.recv() => match command {
                    Some(command) => self.handle(command).await,
                    None => break,
                },
                Some(agent) = self.agents.1.recv() => self.watch(agent).await,
            }
        }
        self.close().await;
    }

    /// 办一条命令。
    async fn handle(&mut self, command: Command) {
        if let Some(refused) = self
            .read_only
            .as_ref()
            .and_then(|_| refuse_read_only(&command))
        {
            return self.report(refused);
        }
        match command {
            Command::Send { text, images } => {
                self.send(Some(images), |attached| {
                    let mut blocks = vec![Block::Text(Text { text })];
                    blocks.extend(attached.unwrap_or_default());
                    Kernel::Send {
                        blocks,
                        urgent: false,
                    }
                })
                .await;
            }
            Command::Shell { text } => {
                self.send(None, |_| Kernel::Shell {
                    text,
                    urgent: false,
                })
                .await
            }
            Command::Stop => self.stop().await,
            Command::Barrier(done) => {
                // 等的一头不等了的没处可回。
                if done.send(()).is_err() {}
            }
            Command::Title(title) => {
                let meta = Kernel::SetMeta {
                    title: Some(title.clone()),
                    pinned: None,
                };
                match self.ask(meta).await {
                    Some(Outcome::Accepted { .. }) => {
                        self.report(Update::Renamed(Some(title).filter(|t| !t.is_empty())));
                    }
                    outcome => self.refused(outcome),
                }
            }
            Command::Retitle => self.retitle().await,
            Command::Revert => self.undo(false).await,
            Command::Unrevert => self.undo(true).await,
            Command::Redo { text, images } => self.redo(text, images).await,
            Command::Unattended => {
                self.unattended = true;
                self.unattend(self.session.as_ref());
            }
            Command::Choices => self.choices().await,
            Command::Configure(model) => self.configure(model).await,
            Command::ChatDefault(reference) => self.chat_default(reference).await,
            Command::Efforts(reference) => self.efforts(reference).await,
            Command::Effort(level) => self.set_effort(level).await,
            Command::New { bare } => {
                self.bare = bare;
                self.close().await;
            }
            Command::Sessions => self.list().await,
            Command::Delete(id) => self.delete(id).await,
            Command::Export(id, path) => self.export(id, path).await,
            Command::Open { id, view_broken } => self.open_old(id, view_broken).await,
            Command::ReloadLists => {
                if let Some(ready) = &self.ready {
                    ready.reload_lists();
                }
            }
            Command::Reconfigure => {
                if let Some(Err(why)) = self.ready.as_ref().map(setup::Ready::reconfigure) {
                    self.report(Update::Failed(why));
                }
            }
            Command::Interrupt { send } => {
                let queued = if send { Queued::Send } else { Queued::Return };
                self.tell(Kernel::Interrupt { queued }).await;
            }
            Command::Withdraw(seq) => {
                let message = seq.and_then(Seq::new);
                self.tell(Kernel::Withdraw { message }).await;
            }
            Command::Compact(instructions) => self.tell(Kernel::Compact { instructions }).await,
            Command::Clear => self.tell(Kernel::Clear).await,
            Command::View(id) => self.view(id).await,
            Command::ReadBack {
                session,
                piece,
                count,
                after,
                upto,
            } => {
                self.read_back(&session, (piece, count), (after, upto))
                    .await
            }
            Command::Spoken {
                session,
                upto,
                texts,
            } => self.spoken(&session, upto, texts).await,
            Command::Changes {
                session,
                path,
                after,
                upto,
                texts,
            } => {
                self.changes(&session, path, (after, upto), texts).await;
            }
            Command::Loop(words) => self.looping(&words).await,
            Command::JobOutput(job) => {
                let Some(main) = self.session.clone() else {
                    return;
                };
                let text = children::output(&main, &job).await;
                self.report(Update::JobOutput { job, text });
            }
            Command::StopJob(job) => {
                let Some(main) = self.session.clone() else {
                    return;
                };
                let cause = self.id();
                if let Some(reason) = children::stop(&main, &job, cause).await {
                    self.report(Update::StopRefused { reason });
                }
            }
            Command::Answer {
                session,
                call,
                answer,
            } => {
                let Ok(call_id) = CallId::parse(&call) else {
                    self.answer_refused(call, None);
                    return;
                };
                let outcome = self
                    .ask_in(&session, Kernel::Answer { call_id, answer })
                    .await;
                if let Some(Outcome::Rejected { reason }) = outcome {
                    self.answer_refused(call, Some(reason.code()));
                }
            }
        }
    }

    /// 说一句话、跑一条命令：还没开会话的先开。开不成、被拒的照「没发出去」报，字回到输入框。`images` 是这一句附的图片
    /// （LM-28）：先交给会话存成 blob、换成图片块（`Handle::attach`），再照 `make` 造命令；有一张附不上的整句不发，照原因码报。
    async fn send(
        &mut self,
        images: Option<Vec<lm_engine::session::Attach>>,
        make: impl FnOnce(Option<Vec<Block>>) -> Kernel,
    ) {
        if self.session.is_none()
            && let Err(message) = self.open().await
        {
            self.report(Update::Unsent {
                reason: None,
                message,
            });
            return;
        }
        let attached = match images {
            Some(images) if !images.is_empty() => match self.attach(images).await {
                Ok(blocks) => Some(blocks),
                Err(unsent) => return self.report(unsent),
            },
            images => images.map(|_| Vec::new()),
        };
        match self.ask(make(attached)).await {
            Some(Outcome::Rejected { reason }) => self.report(Update::Unsent {
                reason: Some(reason.code().to_string()),
                message: reason.code().to_string(),
            }),
            Some(_) => self.tell_ignored(),
            None => self.report(Update::Unsent {
                reason: None,
                message: "session stopped".to_string(),
            }),
        }
    }

    /// 把附的图片交给正在说话的那个会话（看着子会话的是它）存成图片块；附不上的交回要报的「没发出去」。
    async fn attach(
        &mut self,
        images: Vec<lm_engine::session::Attach>,
    ) -> Result<Vec<Block>, Update> {
        let session = match &self.viewing {
            Some(viewing) => Some(viewing.handle.clone()),
            None => self.session.clone(),
        };
        let stored = match session {
            Some(session) => session.attach(images).await.ok(),
            None => None,
        };
        match stored {
            Some(Ok(attached)) => {
                // 这时的模型看不了图：照发（会话替它转述，或者换成一句占位），告诉人一声。
                if attached.blind {
                    self.report(Update::ImagesUnseen);
                }
                Ok(attached.blocks)
            }
            Some(Err(error)) => Err(Update::Unsent {
                reason: Some(error.code().to_string()),
                message: format!("{}: {}", error.code(), error.name()),
            }),
            None => Err(Update::Unsent {
                reason: None,
                message: "session stopped".to_string(),
            }),
        }
    }

    /// 第一句话发出去了：读配置时被忽略的配置项，连同会话找技能、读 MCP 服务时被忽略的（造会话时就有，载入的会话等它第一次
    /// 开回合），有的交 [`Update::Ignored`]，界面弹一条提示（LM-3补，`app/checkup.rs`）。一次运行只交一次；应急模式不交。等的
    /// 那一下另起一个任务，不挡下一条命令；会话停了还没记下的，只交配置项。
    fn tell_ignored(&mut self) {
        let Some(settings) = self.ready.as_mut().and_then(setup::Ready::take_ignored) else {
            return;
        };
        let seen = self.session.as_ref().and_then(Handle::ignored);
        let deliver = Arc::clone(&self.deliver);
        tokio::spawn(async move {
            let mut found = match seen {
                Some(mut seen) => seen
                    .wait_for(Option::is_some)
                    .await
                    .ok()
                    .and_then(|found| found.clone())
                    .unwrap_or_default(),
                None => lm_engine::checkup::Ignored::default(),
            };
            found.settings = settings;
            if !found.is_empty() && deliver(Update::Ignored(found)) {}
        });
    }

    /// 开会话、打开旧会话的时候（LM-9）：要用到模型目录了，接上后台取、更新（[`setup::Ready::want_catalog`]）。无人值守的这一次
    /// 运行（`litemiyu ask`）不取：问完就退，取到一半白取。
    fn want_catalog(&mut self) {
        if self.unattended {
            return;
        }
        if let Some(ready) = self.ready.as_mut() {
            ready.want_catalog();
        }
    }

    /// 开一个新会话：第一次的先造好要的几样；订阅它推来的，交回界面。
    async fn open(&mut self) -> Result<(), String> {
        if self.ready.is_none() {
            self.ready = Some(setup::Ready::new(self.rescue.take(), self.unattended)?);
        }
        self.want_catalog();
        let command = self.id();
        let Some(ready) = &self.ready else {
            return Err("engine not ready".to_string());
        };
        let handle = ready
            .create(command, self.bare, self.rescue_chat.clone())
            .await?;
        self.unattend(Some(&handle));
        let subscription = handle
            .subscribe()
            .await
            .map_err(|_| "session stopped".to_string())?;
        self.report(Update::Ready(handle.id().as_str().to_string()));
        self.report(Update::Loops(handle.loops()));
        self.follow(subscription, handle, false);
        self.hand_pending_effort().await;
        Ok(())
    }

    /// 接着把会话 `handle` 推来的交回界面，它就是正在说话的会话；`settle` 的安静下来先还一次堆（打开旧会话：补发时放掉的
    /// 一大堆）。
    fn follow(
        &mut self,
        subscription: lm_engine::session::Subscription,
        handle: Handle,
        settle: bool,
    ) {
        let task = tokio::spawn(forward::forward(
            subscription,
            handle.clone(),
            (Arc::clone(&self.deliver), self.agents.0.clone()),
            settle,
        ));
        self.forwarder = Some(task.abort_handle());
        self.session = Some(handle);
    }

    /// `/loop`（`looping.rs`）：还没开会话的，开循环先开会话，停的当场答「没有循环」。先记一条用了 `/loop`
    /// （`command.ran`）再开：开的那一刻交进来的第一句话排在它后面，下次载入照它认循环开着（`looping::Trail`）。
    async fn looping(&mut self, words: &str) {
        let Ok(asked) = looping::parse(words) else {
            self.report(Update::Refused {
                reason: Some("bad_params".to_string()),
                message: "bad_params".to_string(),
            });
            return;
        };
        if self.session.is_none() {
            if matches!(asked, lm_engine::schedule::Asked::Off) {
                self.report(Update::Said(looping::receipt(&asked, false)));
                return;
            }
            if let Err(message) = self.open().await {
                self.report(Update::Failed(message));
                return;
            }
        }
        let Some(session) = self.session.clone() else {
            return;
        };
        if !session.loops() {
            self.report(Update::Refused {
                reason: Some("unknown_command".to_string()),
                message: "unknown_command".to_string(),
            });
            return;
        }
        let ran = Kernel::Ran {
            text: format!("/loop {}", words.trim()),
            command: "loop".to_string(),
        };
        let outcome = self.ask_main(ran).await;
        self.refused(outcome);
        let done = match &asked {
            lm_engine::schedule::Asked::Off => session.stop_loop().await,
            lm_engine::schedule::Asked::Fixed { every, prompt } => {
                session
                    .start_loop(Some(*every), (*prompt).to_string())
                    .await
            }
            lm_engine::schedule::Asked::Dynamic { prompt } => {
                session.start_loop(None, (*prompt).to_string()).await
            }
        };
        // 会话停了的：没处可开，也没处可报。
        if let Ok(stopped) = done {
            self.report(Update::Said(looping::receipt(&asked, stopped)));
            // 停掉的没有推送跟着来（推送以后才看循环变没变，`forward`）：当场交一次。
            self.report(Update::LoopNow(session.looping()));
        }
    }

    /// `/stop`（Miyu 核心的 `commands/basic.rs`）：打断这一轮，排着的话留着；没有在进行的回合照样往下。子代理、后台命令
    /// 还没搬（L5），没有要停的。最后记一条 `command.ran`，界面照它把留着的排队话放进正文。没有会话的不发。
    async fn stop(&mut self) {
        let interrupt = Kernel::Interrupt {
            queued: Queued::Keep,
        };
        match self.ask(interrupt).await {
            None => return,
            Some(Outcome::Rejected { reason }) if reason != Reason::NotRunning => {
                self.report(Update::Refused {
                    reason: Some(reason.code().to_string()),
                    message: reason.code().to_string(),
                });
                return;
            }
            Some(_) => {}
        }
        let ran = Kernel::Ran {
            text: "/stop".to_string(),
            command: "stop".to_string(),
        };
        self.tell(ran).await;
    }

    /// 发给会话一条命令：被拒的照原因码报给界面。没有会话的不发。
    async fn tell(&mut self, command: Kernel) {
        let outcome = self.ask(command).await;
        self.refused(outcome);
    }

    /// 被拒的照原因码报给界面。
    fn refused(&self, outcome: Option<Outcome>) {
        if let Some(Outcome::Rejected { reason }) = outcome {
            self.report(Update::Refused {
                reason: Some(reason.code().to_string()),
                message: reason.code().to_string(),
            });
        }
    }

    /// 发给会话一条命令，等它的回应；没有会话、会话停了的是 `None`。切进了子会话的交给它（循环照旧对着主会话，
    /// 走 [`Engine::ask_main`]）。
    async fn ask(&mut self, command: Kernel) -> Option<Outcome> {
        let id = self.id();
        let session = match &self.viewing {
            Some(viewing) => &viewing.handle,
            None => self.session.as_ref()?,
        };
        session
            .command(id, By::Person(Person::default()), command)
            .await
            .ok()
    }

    /// 发给主会话一条命令（循环：LiteMiyu 只对着主会话）。
    async fn ask_main(&mut self, command: Kernel) -> Option<Outcome> {
        let id = self.id();
        self.session
            .as_ref()?
            .command(id, By::Person(Person::default()), command)
            .await
            .ok()
    }

    /// 切进子会话 `id`、退回主会话（`None`）。取不到那个子会话的照旧看着主会话，界面那一头等不到 [`Update::Viewed`]。
    async fn view(&mut self, id: Option<String>) {
        self.viewing = None;
        let (Some(id), Some(main)) = (id, self.session.clone()) else {
            return;
        };
        self.viewing = view::enter(&main, &id, Arc::clone(&self.deliver)).await;
    }

    /// 交回界面。界面不收了的是在退出：没处可报。
    /// 发一条命令给会话 `session`：主会话的照它，主会话派的子会话照主会话的把手取（`Handle::child`）。
    async fn ask_in(&mut self, session: &str, command: Kernel) -> Option<Outcome> {
        let main = self.session.clone()?;
        if session.is_empty() || main.id().as_str() == session {
            return self.ask_main(command).await;
        }
        let id = lm_kernel::kernel::id::SessionId::parse(session).ok()?;
        let child = main.child(&id).await?;
        child
            .command(self.id(), By::Person(Person::default()), command)
            .await
            .ok()
    }

    /// 订阅主会话派出去的子会话 `id`（`children.rs`）：一个只订阅一次；`/new` 以后旧会话的不管。
    async fn watch(&mut self, id: String) {
        let Some(main) = self.session.clone() else {
            return;
        };
        if !self.watched.insert(id.clone()) {
            return;
        }
        if self.unattended
            && let Ok(child) = lm_kernel::kernel::id::SessionId::parse(&id)
        {
            let child = main.child(&child).await;
            self.unattend(child.as_ref());
        }
        children::watch(&main, &id, Arc::clone(&self.deliver), self.agents.0.clone()).await;
    }

    /// 没人能确认的这一次运行里，会话 `handle` 也交上（[`Command::Unattended`]）。停了的没处交。
    fn unattend(&self, handle: Option<&Handle>) {
        if let Some(handle) = handle.filter(|_| self.unattended)
            && handle.unattend().is_err()
        {}
    }

    fn report(&self, update: Update) {
        if !(self.deliver)(update) {}
    }

    /// 回答被拒：调用编号、原因码。
    fn answer_refused(&self, call: String, reason: Option<&str>) {
        self.report(Update::AnswerRefused {
            call,
            reason: reason.map(str::to_string),
            message: reason.unwrap_or("bad_call").to_string(),
        });
    }

    /// 下一条命令的编号：跨进程也不重（[`lm_engine::session::new_command_id`]）。内核载入时照日志记着上一次运行里接受过的
    /// 编号，每个进程从 `c1` 数起的话，重开说过话的会话头一句会被当成上一次的哪一条、吞掉。
    fn id(&self) -> CommandId {
        lm_engine::session::new_command_id()
    }

    /// 有计划地停下正在说话的会话：日志落好盘再退出。
    async fn close(&mut self) {
        self.watched.clear();
        self.reader = None;
        self.read_only = None;
        self.viewing = None;
        // 单线程的运行时：掐掉以后它不会再跑，旧会话的推送不会排到之后开的会话后面。
        if let Some(forwarder) = self.forwarder.take() {
            forwarder.abort();
        }
        if let Some(session) = self.session.take() {
            // 停不下的是已经停了：没有要等的。
            if session.stop().await.is_err() {}
        }
    }
}

/// 只读打开着坏了一行的会话时（[`Engine::read_only`]），要会话办的命令照 Miyu 核心拒（`session_read_only`，界面照
/// `refusal_hints` 说「这个会话只能看，不能接着聊」）：说话、跑命令、重做照「没发出去」报（字退回输入框），别的照拒绝报。
/// 交回要报的拒绝；列会话、开别的、`/new`、读回、导出、删这些不用会话的照常办（`None`）。
fn refuse_read_only(command: &Command) -> Option<Update> {
    let reason = Some("session_read_only".to_string());
    let message = "session_read_only".to_string();
    let update = match command {
        Command::Send { .. } | Command::Shell { .. } | Command::Redo { .. } => {
            Update::Unsent { reason, message }
        }
        Command::Stop
        | Command::Title(_)
        | Command::Retitle
        | Command::Revert
        | Command::Unrevert
        | Command::Configure(_)
        | Command::Effort(_)
        | Command::Interrupt { .. }
        | Command::Withdraw(_)
        | Command::Compact(_)
        | Command::Clear
        | Command::Loop(_) => Update::Refused { reason, message },
        _ => return None,
    };
    Some(update)
}
