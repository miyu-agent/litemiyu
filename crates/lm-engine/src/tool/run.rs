//! 执行一次调用（`05-内核接口.md` 第六节「执行这一步」，施工 4-2）：交给工具的、工具交回的、执行中的
//! 输出推给谁。

use std::collections::BTreeMap;
use std::fmt;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use lm_kernel::kernel::block::{Block, Text};
use lm_kernel::kernel::event::{JobMessaged, JobStarted, PeerWatch, Said};
use lm_kernel::kernel::id::{ContentHash, MediaType};
use lm_kernel::kernel::time::UtcOffset;

use crate::tool::{
    AgentPort, JobPort, Log, MessagePort, QuestionPort, SessionsPort, Stop, UsagePort,
};

/// 一次调用交给工具的：修正过的参数、这一轮的工作目录、系统的家目录、LiteMiyu 的数据目录、她看过的文件、这个会话日志的
/// 只读入口、会话的时区、派子代理的端口、留言的端口、任务端口和列会话的端口。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Call {
    /// 修正过的参数：一个 JSON 对象的原文。
    pub args: String,
    /// 这一轮的工作目录：回合开始时的那一个。
    pub cwd: String,
    /// 系统的家目录（施工 4-4 上）：路径里的 `~` 照它换，和权限策略换的一样，碰到的才是同一个文件。读不出来的是空的。
    pub home: Option<PathBuf>,
    /// LiteMiyu 的数据目录（施工 4-4 下，`docs/00-设计.md` 第五节第 6 条）：哪一件工具都不能碰。
    /// `shell` 照它认删它的命令是高危，`write`、`edit` 照它认记忆的目录。不知道的是空的。
    pub data_root: Option<PathBuf>,
    /// 她这个会话里看过的文件（施工 4-6 上）：写的工具改一个已经在了的文件之前，照它核对。
    pub seen: Arc<Seen>,
    /// 叫停的旗（施工 4-9 再补一）：执行器「叫它停」、future 被丢掉时举起来。
    pub stop: Stop,
    /// 这个会话日志的只读入口（施工 6-4）：只有 `history` 用。没有的（测试里的假调用）是空的。
    pub log: Option<Log>,
    /// 会话的时区（施工 6-4）：照会话现在的环境，只有 `history` 用。
    pub offset: UtcOffset,
    /// 派子代理的端口（施工 7-5）：执行器照这一次调用抄好父会话的那几样，只有 `agent` 用。没有的（测试里的假调用、
    /// 核心没装会话表的）是空的，`agent` 照派不了出错。
    pub agents: Option<Arc<dyn AgentPort>>,
    /// 发话的端口（施工 7-7、C-5）：执行器照这一次调用抄好父会话、派出去的子代理，只有 `send_message` 用。没有的
    /// （测试里的假调用、核心没装会话表的）是空的，`send_message` 照送不到出错。
    pub messages: Option<Arc<dyn MessagePort>>,
    /// 任务端口（施工 7-3）：起好的后台命令交给它，拿回编号。只有 `shell` 用；没有的（会话外面的调用，例如测试）不能放到
    /// 后台。
    pub jobs: Option<Arc<dyn JobPort>>,
    /// 列会话的端口（施工 C-3）：执行器照这一次调用抄好这个会话的编号，只有本机的主会话有，只有 `sessions` 用。
    /// 没有的（测试里的假调用、子会话、场所会话、核心没装会话表的）是空的，`sessions` 照没有别的会话答。
    pub sessions: Option<Arc<dyn SessionsPort>>,
    /// 查用量的端口（施工 8-15）：执行器照这一次调用抄好这个会话的编号、派出去那一刻的上下文，只有 `session_usage` 用。
    /// 没有的（测试里的假调用、没开用量汇总的核心）是空的，`session_usage` 照什么都没花答。
    pub usage: Option<Arc<dyn UsagePort>>,
    /// 提问的端口（施工 D-2）：执行器照这一次调用造一个，只有 `ask_user` 用。没人能回答的会话、测试里的假调用没有，
    /// `ask_user` 照「这里没人能回答」出错。
    pub questions: Option<Arc<dyn QuestionPort>>,
    /// 循环的端口（施工 J-2）：引擎造会话、载入时交进来，只有 `schedule_wakeup` 用。测试里的假调用、没交的没有，照「这个会话
    /// 用不了循环」出错。
    pub schedules: Option<Arc<dyn crate::tool::SchedulePort>>,
    /// 这一轮冻结的配置（施工 N-2 下）：工具读自己包的配置项、取密钥。会话交；测试里的假调用、会话外面的调用没有，照默认值。
    pub settings: Option<Arc<dyn crate::tool::SettingsPort>>,
    /// 权限策略判过的真实位置（施工 5-18，`11-权限与沙盒.md` 第七节）：工具报的每一条 [`Target`] 判的时候换成的那一个。
    /// 执行器照这一次调用放进来，工具经 [`Call::real`] 换路径时核对，换出来的不在里面的不碰。测试里直接造的调用、执行前的链
    /// 报路径时没有，照换出来的碰。
    pub checked: Option<Arc<[PathBuf]>>,
    /// 调用的事实（施工 E-M，[`crate::tool::Caller`]）：执行器照派它的那一轮放进来，命令、脚本用。测试里直接造的调用、会话外面
    /// 的调用、执行前的链报路径时没有。
    pub caller: Option<crate::tool::Caller>,
}

/// 她看过的文件（`10-自带软件.md` 第五节「她看过的」，施工 4-6 上）：换成真实位置以后的路径，和她最后一次看到的
/// 内容哈希。读过的（读了哪一段都算）、自己写过的都算。
pub type Seen = BTreeMap<PathBuf, ContentHash>;

/// 这次调用认出来的高危（施工 5-17，`docs/designs/11-权限与沙盒.md` 第四节「高危命令照规则拦」）：高危的问人，灭顶的直接拒
/// （`docs/00-设计.md` 第五节第 6 条）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hazard {
    /// 哪一种：协议、日志里的写法（例如 `delete_home`），问人时放进说明的 `hazard`。
    pub code: &'static str,
    /// 灭顶的直接拒，拒的时候写给她的一句（连同给人看的说法）；没有的问人。
    pub refused: Option<lm_kernel::kernel::tool::Worded>,
}

/// 一次调用要碰的一条路径（施工 4-3 下）：她给的原样，是读是写，碰的是不是这一条本身。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// 她给的路径，原样：相对的照这一轮的工作目录算，`~` 开头的照家目录算。
    pub path: String,
    /// 要写：新建、改、删。不是就是读。
    pub write: bool,
    /// 碰的是这一条本身：最后一段是链接的不跟（原来给 `trash` 删链接本身用，施工 4-9 再补二；LM-2 删了 `trash` 以后没有工具设它，
    /// 权限策略那一半留着）。权限策略照
    /// `crate::fs::resolve_itself` 换真实的位置，和工具碰的是同一个。
    pub itself: bool,
}

/// 一次调用的结局：给模型看的内容块，出没出错，给人看的说法，效果。用时由执行器量。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Done {
    /// 出错了没有：工具执行了，但是出了错。错在哪，写在内容块里给她看。
    pub error: bool,
    /// 给模型看的内容。
    pub blocks: Vec<Block>,
    /// 给人看的说法（施工 4-5 上）：不发给模型，记进 `tool.result`，头照自己的语言换成字。没交的是空的。
    pub human: Option<Said>,
    /// 效果（施工 4-6 上）：读了、改了、删了哪个文件，照先后。不发给模型，记进 `tool.result`。
    pub effects: Vec<Effect>,
    /// 看到叫停的旗，停在改之前，什么都没改（施工 4-9 再补一）：执行器照「已取消，跑到一半」交给内核。
    pub stopped: bool,
    /// 交回的图片（施工 4-13）：执行器存成 blob，换成图片块，照先后接在 `blocks` 后面。
    pub images: Vec<Picture>,
    /// 要结束这一轮（施工 4-14）：这一步几件工具的结果都到了，内核不再请求模型，照常记 `turn.ended`。出错的、停了的不算。
    pub end_turn: bool,
    /// 她发出去的图（施工 X-19）：记进 `tool.result`，不发给模型。
    pub sent: Vec<lm_kernel::kernel::event::Sent>,
    /// 给头看的结构化结果（施工 W-11）：一个对象，形状由这件工具定（写在它的图纸里）；记进 `tool.result` 的 `detail`，不发给模型。
    pub detail: Option<serde_json::Value>,
}

/// 工具交回的一张图片：字节本身，和量好的媒体类型、宽高（`03-事件模型.md` 第四节：量不出尺寸的不当图片）。
#[derive(Clone, PartialEq, Eq)]
pub struct Picture {
    /// 图片文件的全部字节。
    pub bytes: Vec<u8>,
    /// 例如 `image/png`。
    pub media_type: MediaType,
    /// 宽，像素。
    pub width: u32,
    /// 高，像素。
    pub height: u32,
}

impl fmt::Debug for Picture {
    /// 字节不打出来：一张图几 MB。
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Picture")
            .field("bytes", &self.bytes.len())
            .field("media_type", &self.media_type)
            .field("width", &self.width)
            .field("height", &self.height)
            .finish()
    }
}

/// 工具报的一样效果（`10-自带软件.md` 第五节，施工 4-6 上）。改前改后带着内容本身：执行器存成 blob、换成哈希，
/// 再交进内核。路径都是换成真实位置以后的。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// 读了一个文件：读了第几行到第几行（从 1 数起，含两头；一行都没显示的是空的），整份文件的内容哈希。
    Read {
        /// 读的哪个文件。
        path: PathBuf,
        /// 读了第几行到第几行。
        lines: Option<[u64; 2]>,
        /// 整份文件的内容哈希。
        hash: ContentHash,
    },
    /// 改了一个文件：改前的内容（新建的是空的）、改后的内容。
    Changed {
        /// 改的哪个文件。
        path: PathBuf,
        /// 改前的内容。
        before: Option<Vec<u8>>,
        /// 改后的内容。
        after: Vec<u8>,
    },
    /// 派出去一个任务（施工 7-5，`agents.md`「对外的样子」）：照原样换成内核的 `job.started`。
    JobStarted(JobStarted),
    /// 给自己派的子代理留了言（施工 7-7，`agents.md` 第六条）：照原样换成内核的 `job.messaged`。
    JobMessaged(JobMessaged),
    /// 订了别的会话的「空了告诉我」（施工 C-6，`cross-session.md` 第六条第 2 款）：照原样换成内核的 `peer.watch`。
    PeerWatch(PeerWatch),
}

impl Done {
    /// 成功，交回一段字。
    pub fn ok(text: impl Into<String>) -> Done {
        Done {
            error: false,
            blocks: vec![Block::Text(Text { text: text.into() })],
            human: None,
            effects: Vec::new(),
            stopped: false,
            images: Vec::new(),
            end_turn: false,
            sent: Vec::new(),
            detail: None,
        }
    }

    /// 看到叫停的旗，停在改之前，什么都没改（施工 4-9 再补一）。内容由内核写。
    pub fn stopped() -> Done {
        Done {
            error: false,
            blocks: Vec::new(),
            human: None,
            effects: Vec::new(),
            stopped: true,
            images: Vec::new(),
            end_turn: false,
            sent: Vec::new(),
            detail: None,
        }
    }

    /// 出错，交回一段字：错在哪，写给她看，让她下一步自己改对。
    pub fn error(text: impl Into<String>) -> Done {
        Done {
            error: true,
            blocks: vec![Block::Text(Text { text: text.into() })],
            human: None,
            effects: Vec::new(),
            stopped: false,
            images: Vec::new(),
            end_turn: false,
            sent: Vec::new(),
            detail: None,
        }
    }

    /// 带上给头看的结构化结果（施工 W-11）：`detail` 是一个对象。
    #[must_use]
    pub fn detail(mut self, detail: serde_json::Value) -> Done {
        self.detail = Some(detail);
        self
    }

    /// 带上给人看的说法。
    #[must_use]
    pub fn said(mut self, human: Said) -> Done {
        self.human = Some(human);
        self
    }

    /// 再报一样效果。
    #[must_use]
    pub fn effect(mut self, effect: Effect) -> Done {
        self.effects.push(effect);
        self
    }

    /// 再交一张图片，接在后面（施工 4-13）。
    #[must_use]
    pub fn image(mut self, picture: Picture) -> Done {
        self.images.push(picture);
        self
    }

    /// 要结束这一轮（施工 4-14）。
    #[must_use]
    pub fn ending(mut self) -> Done {
        self.end_turn = true;
        self
    }
}

/// 执行中的输出推给谁：一段段推给头，不落盘（`03-事件模型.md` 第五节）。
pub struct Progress(Box<dyn Fn(String) + Send + Sync>);

impl Progress {
    /// 每一段交给 `push`。
    pub fn new(push: impl Fn(String) + Send + Sync + 'static) -> Progress {
        Progress(Box::new(push))
    }

    /// 推一段。
    pub fn push(&self, text: impl Into<String>) {
        (self.0)(text.into());
    }
}

impl fmt::Debug for Progress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Progress")
    }
}

/// 跑着的一次调用：交回结局的 future。掐掉就是丢掉它，工具手里的东西随之收拾；「叫它停」另有 [`Call::stop`] 的旗
/// （施工 4-9 再补一）。
pub type Running<'a> = Pin<Box<dyn Future<Output = Done> + Send + 'a>>;
