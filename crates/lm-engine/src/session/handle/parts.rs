//! 这个会话的上下文一块块的、累计用量照块摊（Miyu 施工 CX-1 上、CX-3，`session.context`、`session.usage_parts`；Miyu 的
//! `miyu-session/src/handle/context.rs`）：界面的 `/context` 明细（「现在装了多少」「这个会话一共用了多少」两段）、`/status` 照它。
//! 问 actor 要：内核照有效历史组装一次，各块用和压缩线同一个本地估算器估；system 照这时的快照拆出认得出的几块。只看不换。
//!
//! 和 Miyu 不一样的：system 只拆 LiteMiyu 快照里单存原文的几块（技能、「关于你」、MCP 服务给模型的说明），没有人设、场所、
//! 知识库那些（LiteMiyu 砍了）；Miyu 照原来的料重拼着全拆（CX-1 下），LiteMiyu 没有人格的料可重拼，照单存的几块拆（Miyu 的
//! `split`）。

use tokio::sync::oneshot;

use lm_kernel::kernel::session::Breakdown;

use super::{Handle, Message, Stopped};

/// system 里的一块（Miyu 的 `SystemPart`，LiteMiyu 有的几种）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SystemPart {
    /// 技能那一块。
    Skills,
    /// 「关于你」。
    Profile,
    /// 接在尾部的一段：键（LiteMiyu 只有 MCP 服务给模型的说明）。
    Appended(String),
    /// 剩下的：核心的几句、自己设的提示词……
    Other,
}

/// 这时的上下文一块块的。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Context {
    /// 内核算的明细：限额（窗口、压缩线、上限）、这时的用量（`used`，同底栏的口径）、锚、system、每件工具、摘要、对话照谁放进
    /// 去的拆（`said`）。
    pub breakdown: Breakdown,
    /// system 拆开：认得出的几块照快照里的先后，最后是其他；加起来就是 `breakdown.system`。一块都认不出的只有其他。
    pub system: Vec<(SystemPart, u64)>,
    /// 现在的工具面，照发的先后（名字）。
    pub offered: Vec<String>,
}

/// 一次主请求（Miyu 施工 CX-3）：发出去那一刻的上下文一块块的，和供应商报的输入（没命中、命中、写缓存加起来）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Charged {
    /// 那一刻的明细：限额、锚、用量不算。
    pub context: Context,
    /// 报的输入。
    pub input: u64,
}

/// 这个会话的累计用量照上下文的块摊（Miyu 施工 CX-3）：只算主请求，照日志一条条回放。摊的算法在头（Miyu 在协议那一层的
/// `session_context.rs` 的 `spent_shape`）：每次的 `input` 照那一刻各块的估算按比例分。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Spent {
    /// 主请求有几次（报没报用量都算）。
    pub requests: u64,
    /// 报了用量的那几次，每次一份。
    pub charged: Vec<Charged>,
    /// 输出加起来。
    pub output: u64,
}

/// 头问的是哪一样。
#[derive(Debug)]
pub(crate) enum Asked {
    /// 这时的上下文一块块的。
    Now(oneshot::Sender<Context>),
    /// 累计用量照上下文的块摊。
    Spent(oneshot::Sender<Spent>),
}

impl Handle {
    /// 这时的上下文一块块的（「现在装了多少」）。闲着收成槽的先从日志读回来再算（读不回的各块是空的），算完照旧收着。
    ///
    /// # Errors
    ///
    /// 会话停了。
    pub async fn context(&self) -> Result<Context, Stopped> {
        let (reply, answer) = oneshot::channel();
        self.send(Message::Context(Asked::Now(reply)))?;
        answer.await.map_err(|_| Stopped)
    }

    /// 累计用量照上下文的块摊（「这个会话一共用了多少」）：读整份日志回放，在阻塞线程里，只在问的时候算。会话的累计 token、
    /// 缓存命中、请求次数、压缩次数照 [`Handle::current`] 的 `tally`（不用回放）。
    ///
    /// # Errors
    ///
    /// 会话停了。
    pub async fn usage_parts(&self) -> Result<Spent, Stopped> {
        let (reply, answer) = oneshot::channel();
        self.send(Message::Context(Asked::Spent(reply)))?;
        answer.await.map_err(|_| Stopped)
    }
}
