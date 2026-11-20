//! 事件：已经发生的事实，追加进会话的日志（`docs/designs/03-事件模型.md`）。
//!
//! 一条事件在日志里是一行紧凑的 JSON，外壳的写法见第二节「外壳的写法」。
//! 认识的种类读成对应的类型；不认识的，`body` 原样留着。

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::kernel::id::{CommandId, EventKind, Seq, TurnId};
use crate::kernel::json::Objects;
use crate::kernel::origin::By;
use crate::kernel::raw::RawJson;
use crate::kernel::time::Timestamp;

mod context;
mod cost;
mod effect;
mod image;
mod job;
mod message;
mod model;
mod only;
mod peer;
mod question;
mod read;
mod restore;
mod session;
mod shell;
mod tool;
mod transient;
mod turn;

pub use context::{
    CompactTrigger, CompactionPaused, ContextCompacted, ContextInjected, PauseReason, RestoredFile,
};
pub use cost::{Cost, Prices, Real};
pub use effect::{
    Effect, FileChanged, FileRead, FileTrashed, JobKind, JobMessaged, JobStarted, PeerWatch,
};
pub use image::ImageDescribed;
pub use job::{ChildReason, ChildReported, JobLines, JobReason, JobReported};
pub use message::{MessageAssistant, MessageUser, MessageWithdrawn, TypedCommand};
pub use model::{
    BlockSpan, CallError, CallResult, ErrorClass, FirstDifference, ForgedCut, ForgedRole,
    MessageRole, ModelCalled, Part, Purpose, Usage,
};
pub use peer::{IdleReason, PeerIdle};
pub use question::{Choice, Question, QuestionAnswered, QuestionAsked, Response, fits};
pub use restore::{FilesRestored, RestoreAction, RestoreOutcome, Restored};
pub use session::{
    CommandRan, ContextCap, Effort, Forked, LinePin, MetaChanged, PolicyChanged, SessionCreated,
    SessionRecapped, SessionSealed, WorkspaceChanged,
};
pub use shell::{ShellRan, ShellReason};
pub use tool::{ApprovalDecided, ApprovalRequested, Decision, Said, Sent, ToolResult, ToolStatus};
pub use transient::{
    ChangeWhy, CompactableChanged, CompactionDone, CompactionProgress, CompactionStarted,
    EffortInUse, EffortSource, ModelChanged, ModelDelta, Piece, RetitleFailed, Retry, Status,
    ToolProgress, Transient, TransientBody,
};
pub use turn::{EndReason, TurnEnded, TurnReverted, TurnStarted, TurnUnreverted};

/// 一条事件：已经发生的一件事。追加进日志以后不改、不删；撤销和压缩也是追加一条新事件
/// （`03-事件模型.md` 第一节）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    /// 会话内的序号，从 1 开始，连续递增。
    pub seq: Seq,
    /// 发生的时刻，取自执行器送进来的时钟输入。
    pub at: Timestamp,
    /// 所属回合；不属于任何回合时没有。
    pub turn: Option<TurnId>,
    /// 由谁引起，取自连接，不取自正文。
    pub by: By,
    /// 引起它的命令，用于去重和追踪。
    pub cause: Option<CommandId>,
    /// 事件的种类，连同它自己的内容。
    pub body: Body,
}

/// 事件的种类：每一种写一行，内容的类型和它在 JSON 里的名字。
/// `Body` 本身、`kind`、按种类读、写出去，都照这一张表生成，加一种只加一行。
///
/// 内容大过 88 字节的几种装在盒子里（红线五 二）：枚举照最大的一种占地方，
/// `model.called` 一种就把每一条事件（连 `turn.ended` 这样 24 字节的）撑到 480 字节；有效历史整条留着事件，一万轮的会话
/// 光这一格就多占一两 MB。最常见的几种（人说的话、回复、回合的开头和结尾、注入的事实）都不大于 88 字节，留在枚举里。读写的
/// JSON 不变，盒子只是放在哪。`tests.rs` 的 `an_event_stays_small` 守着大小。
macro_rules! bodies {
    ($($(#[$doc:meta])* $variant:ident($ty:ty) = $kind:literal,)+) => {
        /// 事件的种类，连同它自己的内容。
        #[derive(Debug, Clone, PartialEq, Eq)]
        pub enum Body {
            $($(#[$doc])* $variant($ty),)+
            /// 不认识的种类，包括不认识的 `ext.*`：`body` 原样留着，投影跳过它。
            Unknown {
                /// 外壳里写的种类名。
                kind: EventKind,
                /// 原样的 `body`，写出去一字不差。
                body: RawJson,
            },
        }

        impl Body {
            /// 内核认识的全部种类名，照表里的先后。样本测试拿它查每一种都有样本
            /// （03 第三节「样本文件」）。
            pub const KINDS: &'static [&'static str] = &[$($kind,)+];

            /// 外壳里 `kind` 那一格写的名字。
            pub fn kind(&self) -> &str {
                match self {
                    $(Body::$variant(_) => $kind,)+
                    Body::Unknown { kind, .. } => kind.as_str(),
                }
            }

            /// 已经知道种类时直接照那一种读 `body`，不先截原文（施工 V-11）。不认识的种类原样留着；认识的读不出来报错，原话
            /// 同 [`Body::read`]。
            fn read_from<'de, D: Deserializer<'de>>(kind: &EventKind, d: D) -> Result<Body, D::Error> {
                let read = match kind.as_str() {
                    $($kind => <$ty>::deserialize(d).map(Body::$variant),)+
                    _ => {
                        return RawJson::deserialize(d).map(|body| Body::Unknown {
                            kind: kind.clone(),
                            body,
                        });
                    }
                };
                read.map_err(|e| D::Error::custom(format!("body of {kind} not readable: {e}")))
            }

            /// 按种类读原样截下来的 `body`（种类写在 `body` 后面的）。认识的种类读不出来是坏数据，报错写明是哪一种。
            /// 认识的照 [`Body::read_from`] 读同一份（LM-10：不再每一种另带一份「新起一个读法、读完查后面没字」）；
            /// 原话同以前照 `serde_json::from_str` 读：`body of <种类> not readable: <serde_json 的原话>`。
            fn read(kind: EventKind, body: RawJson) -> Result<Body, String> {
                if !Body::KINDS.contains(&kind.as_str()) {
                    return Ok(Body::Unknown { kind, body });
                }
                let mut d = serde_json::Deserializer::from_str(body.get());
                let read = Body::read_from(&kind, Objects(&mut d)).map_err(|e| e.to_string())?;
                d.end().map_err(|e| format!("body of {kind} not readable: {e}"))?;
                Ok(read)
            }
        }

        /// `body` 只写它自己的内容；种类写在外壳的 `kind` 里。
        impl Serialize for Body {
            fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                match self {
                    $(Body::$variant(body) => body.serialize(s),)+
                    Body::Unknown { body, .. } => body.serialize(s),
                }
            }
        }
    };
}

bodies! {
    /// 会话创建。
    SessionCreated(Box<SessionCreated>) = "session.created",
    /// 换了策略快照，或者换了权限。
    PolicyChanged(Box<PolicyChanged>) = "session.policy_changed",
    /// 改了标题、置顶。
    MetaChanged(MetaChanged) = "session.meta_changed",
    /// 换了会话在哪个目录干活（施工 9-7 上）。
    WorkspaceChanged(WorkspaceChanged) = "session.workspace_changed",
    /// 一句回顾（施工 3-8 四补）。
    SessionRecapped(SessionRecapped) = "session.recapped",
    /// 跑满了轮数，封存了（施工 C-8 下）。
    SessionSealed(SessionSealed) = "session.sealed",
    /// 人用了一个斜杠命令（施工 O-6）。
    CommandRan(CommandRan) = "command.ran",
    /// 回合开始。
    TurnStarted(TurnStarted) = "turn.started",
    /// 回合结束。
    TurnEnded(TurnEnded) = "turn.ended",
    /// 撤销了几个回合。
    TurnReverted(TurnReverted) = "turn.reverted",
    /// 恢复了最近一次撤销的回合。
    TurnUnreverted(TurnUnreverted) = "turn.unreverted",
    /// 撤销、恢复时改回文件的结局（施工 4-7 上）。
    FilesRestored(FilesRestored) = "files.restored",
    /// 人发来的消息，或者另一个会话发来的消息。
    MessageUser(MessageUser) = "message.user",
    /// 模型一次响应的完整内容，工具调用也在里面。
    MessageAssistant(MessageAssistant) = "message.assistant",
    /// 撤回排着队、她还没听到的消息。
    MessageWithdrawn(MessageWithdrawn) = "message.withdrawn",
    /// 一个工具调用的结果。
    ToolResult(Box<ToolResult>) = "tool.result",
    /// 请人确认一次工具调用。
    ApprovalRequested(ApprovalRequested) = "tool.approval_requested",
    /// 人对确认请求的决定。
    ApprovalDecided(ApprovalDecided) = "tool.approval_decided",
    /// 一个在跑的调用请人回答一组题。
    QuestionAsked(QuestionAsked) = "question.asked",
    /// 人对一组题的回答。
    QuestionAnswered(QuestionAnswered) = "question.answered",
    /// 注入进上下文的一块事实。
    ContextInjected(ContextInjected) = "context.injected",
    /// 压缩的检查点。
    ContextCompacted(Box<ContextCompacted>) = "context.compacted",
    /// 暂停了自动压缩（施工 6-6 上）。
    CompactionPaused(CompactionPaused) = "context.compaction_paused",
    /// 一次模型请求的记录。
    ModelCalled(Box<ModelCalled>) = "model.called",
    /// 后台命令结束了（施工 7-1）。
    JobReported(Box<JobReported>) = "job.reported",
    /// 子会话的回报（施工 7-1）。
    ChildReported(Box<ChildReported>) = "child.reported",
    /// 监视的后台命令出的一批行（施工 MN-1 上）。
    JobLines(JobLines) = "job.lines",
    /// 人在输入框里用 `!` 打的一条命令跑完了（施工 SH-1 上）。
    ShellRan(Box<ShellRan>) = "shell.ran",
    /// 等的那个会话空下来了，或者等不到了（施工 C-1）。
    PeerIdle(PeerIdle) = "peer.idle",
    /// 一张图的转述：看不了图的模型由 `models.vision` 替它看过（施工 8-17）。
    ImageDescribed(Box<ImageDescribed>) = "image.described",
}

impl Event {
    /// 写成日志里的一行：紧凑的 JSON，字段照图纸的顺序，不带换行。换行由存日志的那一层加。
    ///
    /// # Panics
    ///
    /// 实际不会 panic。serde_json 只在两种情况下写不出来：键不是字符串，或者某个 `Serialize`
    /// 自己报错。事件里这两样都没有。
    pub fn to_line(&self) -> String {
        serde_json::to_string(self)
            .expect("事件里只有字符串、数字和原样的 JSON，写成 JSON 不会失败")
    }

    /// 从日志里的一行读回来。认识的种类读成对应的类型，不认识的 `body` 原样留着。
    ///
    /// # Errors
    ///
    /// 这一行不是 JSON、缺了外壳的字段、某个字段不合写法、认识的种类 `body` 读不出来，
    /// 都返回错误，写明哪里错。
    pub fn from_line(line: &str) -> Result<Event, serde_json::Error> {
        crate::kernel::json::from_str(line)
    }

    /// 同 [`Event::from_line`]，只是只解 `only` 里的种类（施工 V-39 上补：判官读群会话日志用）。外壳照旧全解；`only` 里的
    /// 种类、有效历史照种类认的那几种（撤销、恢复、撤回、压缩、回合的开头和结尾、人话、回报、子会话回报、等的会话空了）
    /// 全解；`message.assistant`、`model.called` 只解有效历史撤销时看的 `seen`、`error` 几格，别的格空着；别的种类的
    /// `body` 只过一遍（是合法的 JSON 就行），读成 [`Body::Unknown`]，种类照写、`body` 是 `null`。照先后交给留着一切的
    /// 有效历史（[`crate::kernel::history::History::whole`]）算出来留下的事件和全解的一样。
    ///
    /// # Errors
    ///
    /// 读不成的照 [`Event::from_line`] 再读一遍，交回它的错，和全解报的一字不差；它读成了的交它读成的。不要的种类的
    /// `body` 合 JSON、只是不合那一种的写法的，不报（全解报）。
    pub fn from_line_only(line: &str, only: &[&str]) -> Result<Event, serde_json::Error> {
        read::line_only(line, only).or_else(|_| Event::from_line(line))
    }

    /// 不解，照写出去的样子认一行的种类（施工 V-41 上：翻页先扫一遍日志，只解要的那几行）。[`Event::to_line`] 写的每一行
    /// 都以 `{"seq":<数>,"at":"<时刻>","kind":"<种类>"` 打头（外壳照图纸的先后，紧凑、不转义）：对得上的交回种类；对不上的
    /// （字段换了先后、带空白、带转义的，内核写不出来）交回空的，要知道种类得照 [`Event::from_line`] 解。
    ///
    /// 对得上的、这一行又读得成的，认出来的就是它读成的种类：打头的这一格就是外壳的 `kind`，外壳的每一格只许写一遍。
    #[must_use]
    pub fn kind_written(line: &[u8]) -> Option<&str> {
        let rest = line.strip_prefix(b"{\"seq\":")?;
        let digits = rest.iter().take_while(|byte| byte.is_ascii_digit()).count();
        let rest = rest.get(digits..).filter(|_| digits > 0)?;
        let rest = rest.strip_prefix(b",\"at\":\"")?;
        let rest = &rest[plain(rest)?..];
        let kind = rest.strip_prefix(b"\",\"kind\":\"")?;
        std::str::from_utf8(&kind[..plain(kind)?]).ok()
    }
}

/// 一个 JSON 字符串里没有转义、没有控制字符的那一截有多长：到下一个 `"` 为止；先碰到 `\`、控制字符、到了末尾的没有。
fn plain(text: &[u8]) -> Option<usize> {
    let end = text
        .iter()
        .position(|&byte| byte == b'"' || byte == b'\\' || byte < 0x20)?;
    (text[end] == b'"').then_some(end)
}

/// 写出去的样子。字段的顺序就是图纸上的顺序。
#[derive(Serialize)]
struct LineOut<'a> {
    seq: Seq,
    at: Timestamp,
    kind: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    turn: Option<TurnId>,
    by: &'a By,
    #[serde(skip_serializing_if = "Option::is_none")]
    cause: Option<&'a CommandId>,
    body: &'a Body,
}

impl Serialize for Event {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        LineOut {
            seq: self.seq,
            at: self.at,
            kind: self.body.kind(),
            turn: self.turn,
            by: &self.by,
            cause: self.cause.as_ref(),
            body: &self.body,
        }
        .serialize(s)
    }
}

#[cfg(test)]
mod tests;
