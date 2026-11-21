//! 界面和引擎之间的缝（`docs/00-设计.md` 第四节第 2 条）：界面发 [`Command`]，引擎线程回 [`Update`]，都是 Rust 的枚举，
//! 不经 JSON、不经套接字。
//!
//! Miyu 的终端在这里另起一条线程连核心：找数据根、拉起核心、握手、订阅、补发、断线重连、请求编号、一行一条的 JSON-RPC。
//! LiteMiyu 把这一层整个换掉：[`Core`] 拿着一条通道，第一次要用时（敲第一个字、发第一句话）才起引擎线程（[`engine`]），
//! 引擎直接调 `lm-engine`，会话推来的事件当场读成 [`Push`] 送回界面。

mod engine;
mod kinds;
mod limits;
mod models;
mod presets;
pub(crate) mod push;
mod update;

use std::cell::OnceCell;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tokio::sync::mpsc;

pub use kinds::{EndReason, ToolStatus};
pub use limits::Limits;
pub use models::{Choice, ChoiceState, Efforts};
pub use presets::{PinState, Pinned};
pub use push::{
    Asking, Block, CallError, ChildEnd, Compaction, JobEnd, JobStart, Push, ShellRan, Usage,
};
pub use update::Update;

/// 界面要引擎做的事。
#[derive(Debug)]
pub enum Command {
    /// 说一句话。还没开会话的先开一个。
    Send {
        /// 说的字。
        text: String,
        /// 附的图片，照先后（LM-28）：字里照留着块上的字（`[图片 1]`），会话把它们存成 blob、当图片交给模型。没有的是空的。
        images: Vec<lm_engine::session::Attach>,
    },
    /// 人用 `!` 打的一条命令（「人自己跑命令 `!`」第 2 条）：照说一句话发，会话听到它的那一刻跑，不判权限。
    Shell {
        /// 命令行，原样。
        text: String,
    },
    /// 全停（`/stop`）：打断这一轮，排着的话留着不发，记一条用了 `/stop`。
    Stop,
    /// 打断在进行的这一轮。`send` 为真时排着队的消息接着发（两下 Esc），为假时退回来（Ctrl+C）。
    Interrupt {
        /// 排着队的消息接着发。
        send: bool,
    },
    /// 退回一句排着队、她还没听到的话：写了序号的退那一句，`None` 退最新的一条。退了会话推 `message.withdrawn`，
    /// 字照退回的那一套放回输入框。
    Withdraw(Option<u64>),
    /// 现在就压缩上下文（`/compact`），带着给摘要的要求。
    Compact(Option<String>),
    /// 清空上下文（`/clear`）。
    Clear,
    /// 开新会话（`/new`）：等第一句话再开。`preset` 是用哪个预设（设计第六节第 14 条，LM-29）：预设的名字（自带的是
    /// `full`、`bare`），`None` 照配置里的默认预设。
    New {
        /// 用哪个预设。
        preset: Option<String>,
    },
    /// 列预设（`/preset` 的框，`engine/presets.rs`）：照这一刻的配置文件读，交回 [`Update::Presets`]。
    Presets,
    /// 把这个预设记成默认预设（`sessions.default`，`/preset` 里回车选的）：写进配置，写完再交一次 [`Update::Presets`]。
    PresetDefault(String),
    /// 删掉这个自己定义的预设（`/preset` 里按 `d`、确认过的）：从配置里去掉，去完再交一次 [`Update::Presets`]。
    PresetDelete(String),
    /// 列磁盘上的会话，连这个窗口开着的也列（`/sessions` 的框、侧边栏「最近会话」，照 Miyu 当前这个照先后排在原位）：在
    /// 阻塞线程里扫，交回 [`Update::Sessions`]。
    Sessions,
    /// 打开一个旧会话（会话列表、侧边栏里点的）：会话编号。打开了交 [`Update::Opened`]，接着从头补发它的推送；打不开的
    /// 交 [`Update::OpenFailed`]，正在说话的会话照旧。
    Open {
        /// 会话编号。
        id: String,
        /// 日志中间坏了一行的只读打开（全屏界面，界面线第十八趟，同 Miyu 核心 YC-6）：停下正在说话的那个，交
        /// [`Update::Opened`]、补发坏行以前的，最后交 [`Update::ReadOnly`]；之后要会话办的一律照 `session_read_only` 拒。
        /// 不是的（命令行）照打不开报。
        view_broken: bool,
    },
    /// 配置界面存过（`app/settings.rs`）：重读配置交给开着的会话，下一轮、以后开的会话用上。还没起引擎的不用。
    Reconfigure,
    /// 对话里开 `/model` 时在后台拉到、存下了供应商的模型列表（`app/models.rs`）：重读存下的列表，`/model` 照新的列。还没起引擎的不用
    /// （起来时本来就读）。
    ReloadLists,
    /// `/loop` 和后面的字（`engine/looping.rs` 照引擎的读法读）。
    Loop(String),
    /// 读一条后台命令的输出（面板里展开它）：任务编号。
    JobOutput(String),
    /// 人停掉一个任务（面板里按 `x`）：任务编号。
    StopJob(String),
    /// 读回正文放掉的一串段（红线 5，`app/window.rs`）：从第几段起、几段，日志里序号在 `after` 之后、`upto` 为止的事件。
    ReadBack {
        /// 哪个会话的（主会话、切进去看着的子会话）。
        session: String,
        /// 从第几段起。
        piece: u32,
        /// 几段（连着的）。
        count: u32,
        /// 从这一条之后读起。
        after: u64,
        /// 读到这一条为止（含）。
        upto: u64,
    },
    /// 列日志里你说的话（`/jump`，`transcript/said.rs`）：序号到 `upto` 为止（含）；草稿里的旁白照 `texts` 写。
    Spoken {
        /// 哪个会话的（主会话、切进去看着的子会话）。
        session: String,
        /// 读到这一条为止（含）。
        upto: u64,
        /// 界面的字。
        texts: Box<crate::config::Texts>,
    },
    /// 找日志里改过一个文件的那几步（「文件修改」那一页，`transcript/changed.rs` 的 `Lookup`）：序号在 `after` 之后、`upto`
    /// 为止（含）的那一截；草稿里的旁白照 `texts` 写。
    Changes {
        /// 哪个会话的（主会话、切进去看着的子会话）。
        session: String,
        /// 哪个文件（参数里写的 `file_path`）。
        path: String,
        /// 从这一条之后读起。
        after: u64,
        /// 读到这一条为止（含）。
        upto: u64,
        /// 界面的字。
        texts: Box<crate::config::Texts>,
    },
    /// 切进主会话派的子会话看它（Miyu「切进子会话」）：它的整个编号；`None` 是退回主会话。切进去以后说话、`!` 跑命令、
    /// 打断、撤回、压缩、清空都交给它。
    View(Option<String>),
    /// 这一次运行里没人能确认、能回答（`litemiyu ask`）：开着的、以后开的会话和它们派的子会话
    /// 要问人的当场拒、提问当场跳过。快照、请求照旧，同全屏。
    Unattended,
    /// 换会话的模型（`litemiyu ask --model`，内核的 `Configure`）：模型 `<供应商>/<模型>` 或 `@池`，调的一方照配置查过。还没开会话
    /// 的先开一个；下一个回合起用它，以后都用它。
    Configure(String),
    /// 撤销上一轮（`/undo`，内核的 `Revert`）：撤成了交回 [`Update::Undone`]，被拒的照原因码报。
    Revert,
    /// 恢复刚撤销的那几轮（`/restore`，内核的 `Unrevert`）：恢复了交回 `Update::Undone { restore: true }`，被拒的照原因码报。
    Unrevert,
    /// 重做最后一轮（`/redo`、`/edit`，内核的 `Redo`）：`text` 是改过的那句（粘贴块换回了原文），`None` 原样重来。被拒的
    /// 照「没发出去」报（[`Update::Unsent`]），界面把先藏的那一轮显示回来、编辑的字放回输入框。
    Redo {
        /// 改过的那句；`None` 是原样。
        text: Option<String>,
        /// 改过的那句附的图片（LM-28）；`None` 是附件照原来的，空的是不要附件。
        images: Option<Vec<lm_engine::session::Attach>>,
    },
    /// 给正在说话的会话起名（`/rename`，内核的 `SetMeta`）。没有会话的不办。
    Title(String),
    /// 导出会话（`/export`）：编号、写到哪个文件。在引擎线程里直接写成 Markdown（`cli/export.rs`）；交回 [`Update::Exported`]。
    Export(String, std::path::PathBuf),
    /// 删掉会话（`/delete`，命令行线的 `remove_session`：拿着锁直接删目录）：会话的编号。先 [`Command::New`] 停下它再发这一条；
    /// 删好、删不掉都交回 [`Update::Deleted`]。
    Delete(String),
    /// 只打 `/rename`：让模型照最近的对话重起标题（`Handle::retitle`，Miyu 核心 TT-1）。收下了交回 [`Update::Retitling`]，
    /// 起好了照常推新标题，没起成交回 [`Update::RetitleFailed`]；没东西可起的照原因码报。
    Retitle,
    /// 一道栅栏（`litemiyu ask` 收尾前）：引擎线程办到这一条时回一声。引擎线程是单线程的：回的时候，之前转出来的推送都已经
    /// 交过了，同一批里接着的（回报叫醒的那一轮、同一批接着开的下一轮）不会还在路上。
    Barrier(std::sync::mpsc::Sender<()>),
    /// 列 `/model` 的一行行（`engine/models.rs`）：交回 [`Update::Choices`]。
    Choices,
    /// 把这个引用记成新会话默认的聊天模型（`models.chat`，`/model` 选了的；同 Miyu 的 `SetChat`）。
    ChatDefault(String),
    /// 列一个模型有哪几档思考程度（`/effort`）：`None` 照默认的聊天模型；交回 [`Update::Efforts`]。
    Efforts(Option<String>),
    /// 换这个会话的思考程度（`/effort`，内核的 `SetEffort`）：`None` 回到跟着配置。还没开会话的记着，开了再交。
    Effort(Option<String>),
    /// 回答一次确认或提问：问的会话、调用编号、答的什么。
    Answer {
        /// 问的那个会话的整个编号：主会话的，或者它派的子会话的（抽屉的 `owner`）。
        session: String,
        /// 调用编号。
        call: String,
        /// 答的什么。
        answer: lm_kernel::kernel::session::Answer,
    },
}

/// 引擎那一头交给界面的办法：交回假的是界面不收了（在退出）。
pub type Deliver = Arc<dyn Fn(Update) -> bool + Send + Sync>;

/// 退出时等引擎线程收尾最多多久：停不下的（卡在写盘上）不一直等（毛病二，配置 四：原来 30 秒，盘卡住时界面僵半分钟）。
/// 没等到的：概要没写出来的会话，下次列会话照日志现算。
pub const CLOSING: Duration = Duration::from_secs(3);

/// 界面拿着它发命令。引擎线程第一次要用时才起（`docs/00-设计.md` 第七节「守红线要做的」第 1 条）。放下时等引擎线程收尾。
pub struct Core {
    /// 交给引擎线程的通道，和引擎线程退出时断开的那一头（`engine::spawn`）：起了线程才有。
    commands: OnceCell<(
        mpsc::UnboundedSender<Command>,
        std::sync::mpsc::Receiver<()>,
    )>,
    /// 引擎的消息怎么交回界面。
    deliver: Deliver,
    /// 第一个会话用哪个预设（`litemiyu --preset <名字>`、`--bare`，设计第六节第 14 条）：起引擎线程时交给它；没写的照默认预设。
    preset: Option<String>,
    /// 应急模式备好的（`litemiyu --rescue`，设计第五节第 16 条）：起引擎线程时交给它；不是应急模式的没有。
    rescue: std::cell::Cell<Option<crate::rescue::Prepared>>,
}

impl Core {
    /// 一个还没起引擎线程的；引擎的消息经 `deliver` 交回界面。
    pub fn new(deliver: impl Fn(Update) -> bool + Send + Sync + 'static) -> Self {
        Self {
            commands: OnceCell::new(),
            deliver: Arc::new(deliver),
            preset: None,
            rescue: std::cell::Cell::new(None),
        }
    }

    /// 测试用：不起引擎线程，发的命令都丢掉（单元测试经 `App::handle` 按键时会先 `warm`：起的是真的引擎线程，万一发出去
    /// 一句话，它照真实的数据目录开会话）。
    #[cfg(test)]
    pub fn idle() -> Self {
        let core = Self::new(|_| true);
        let (commands, _dropped) = mpsc::unbounded_channel();
        let (_ends, ended) = std::sync::mpsc::channel::<()>();
        assert!(core.commands.set((commands, ended)).is_ok(), "新造的还没起");
        core
    }

    /// 应急模式（`litemiyu --rescue`）：引擎照备好的配置开不落盘的会话。同 [`Core::first_preset`]，起来时就要定。
    pub fn rescue(&mut self, prepared: crate::rescue::Prepared) {
        self.rescue.set(Some(prepared));
    }

    /// 第一个会话用预设 `name`（`litemiyu --preset <名字>`，`--bare` 是 `bare`）。引擎线程起了以后再设不管用：起来时就要定。
    pub fn first_preset(&mut self, name: String) {
        self.preset = Some(name);
    }

    /// 起引擎线程（起过的不再起）：人敲了第一个字，顺手先起，发第一句话时不用再等。
    pub fn warm(&self) {
        self.sender();
    }

    /// 发一个命令；引擎线程还没起的先起。
    pub fn send(&self, command: Command) {
        // 收的一头只在引擎线程退出时关掉，那时已经报过「起不来」，丢了也无妨。
        if self.sender().send(command).is_err() {}
    }

    /// 交给引擎线程的通道，没起的当场起。
    fn sender(&self) -> &mpsc::UnboundedSender<Command> {
        &self
            .commands
            .get_or_init(|| {
                engine::spawn(
                    Arc::clone(&self.deliver),
                    self.preset.clone(),
                    self.rescue.take(),
                )
            })
            .0
    }
}

/// 界面退出（`/exit`、主循环出错返回）：关掉通道，引擎线程停下开着的会话（日志落好盘、`Window` 放下时写最后一次概要、
/// 放锁）、放下运行时再退出，等它退完。主线程不等就从 `main` 返回的话，进程当场没了，引擎线程做到一半的跟着没了：没起
/// 标题的会话关掉时那一次概要从来写不出来。最多等 `CLOSING`。
impl Drop for Core {
    fn drop(&mut self) {
        self.finish(Instant::now() + CLOSING);
    }
}

impl Core {
    /// 叫引擎线程收尾，等它退完，最多等到 `until`（`main.rs` 退出时先画好「正在收尾」再调）；交回到点时还没退完。没起过的不等。
    pub fn finish(&mut self, until: Instant) -> bool {
        let Some((commands, ended)) = self.commands.take() else {
            return false;
        };
        drop(commands);
        // 线程退出时发的一头跟着放下：断开了就是退完了；到点没退完的不再等。从不发东西。
        let left = until.saturating_duration_since(Instant::now());
        matches!(
            ended.recv_timeout(left),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        )
    }
}

#[cfg(test)]
mod closing_tests {
    //! 退出时等引擎收尾（毛病二）：引擎卡住（盘卡在同步上）的，到点照退，不等满原来的 30 秒；退完了的不等到点。

    use std::time::{Duration, Instant};

    use super::{CLOSING, Core};

    #[test]
    fn a_stuck_engine_is_waited_for_until_the_deadline_only() {
        assert_eq!(CLOSING, Duration::from_secs(3));
        let mut core = Core::new(|_| true);
        let (commands, _engine_inbox) = tokio::sync::mpsc::unbounded_channel();
        // 引擎线程一直不退：发的一头一直拿着。
        let (_never_ends, ended) = std::sync::mpsc::channel::<()>();
        assert!(core.commands.set((commands, ended)).is_ok());
        let started = Instant::now();
        assert!(
            core.finish(started + Duration::from_millis(200)),
            "到点没退完：照退"
        );
        let took = started.elapsed();
        assert!(took >= Duration::from_millis(200), "{took:?}");
        assert!(took < CLOSING, "不等满三秒：{took:?}");
        assert!(!core.finish(Instant::now() + CLOSING), "收过尾的不再等");
    }

    #[test]
    fn an_engine_that_ends_is_not_waited_for_to_the_deadline() {
        let mut core = Core::new(|_| true);
        let (commands, mut engine_inbox) = tokio::sync::mpsc::unbounded_channel::<super::Command>();
        let (ends, ended) = std::sync::mpsc::channel::<()>();
        assert!(core.commands.set((commands, ended)).is_ok());
        // 假的引擎线程：通道关了就退（同真的：收完尾放下发的一头）。
        let engine = std::thread::spawn(move || {
            while engine_inbox.blocking_recv().is_some() {}
            drop(ends);
        });
        let started = Instant::now();
        assert!(!core.finish(started + Duration::from_secs(60)), "退完了");
        assert!(started.elapsed() < Duration::from_secs(30));
        engine.join().unwrap();
    }
}
