//! 会话的事件（`docs/designs/03-事件模型.md` 第三节「会话与回合的事件怎么写」）。

use std::num::NonZeroU32;

use serde::{Deserialize, Serialize};

use crate::kernel::id::{ContentHash, Seq, SessionId, TurnId};

/// `session.created`：会话创建。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionCreated {
    /// 策略快照（03 E5）。
    pub policy: ContentHash,
    /// 一次性的：没人看着的时候回报只记下、不开轮，不起标题（施工 7-2，3-9 下）。不是一次性的不写。
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub oneshot: bool,
    /// 开会话时实际干活的目录，人看到的那种写法（施工 4-9 再补三上）：还没开过回合的会话，核心重启以后照它载入。
    /// 之前的日志没有这一格。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    /// 父会话：派它的那个会话（施工 7-1，`agents.md`）。主会话没有；子会话不写 `oneshot`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<SessionId>,
    /// 第几层：父会话的加一，主会话是第 0 层、不写。和 `parent` 同有同无、至少是 1，由账本查（`kernel/history.md`）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depth: Option<u32>,
    /// 默认工作区用哪个会话的（施工 5-14，`11-权限与沙盒.md` 第四节「默认工作区每个会话一个」）：子会话、分叉出来的照父会话、
    /// 来源会话的那一个（它自己也是照别人的，就照它照的那个），一路追到主会话。主会话用自己的，不写；以前的日志没有这一格。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_from: Option<SessionId>,
    /// 会话用哪个模型（施工 8-8，`models.md`「事件」）：造会话时解析好的引用，模型 `<供应商>/<模型>`（LM-8 以前的日志里还可能是
    /// 池 `@<池>`，照样读进来，路由认不出、退回 `models.chat`）。协议造的照 `session.create` 的 `model`，没写的照那时的
    /// `models.chat`；子会话的照父会话那时的。那时连 `models.chat` 都没配的不写；以前的日志没有这一格，照没有读。内核只记不解读，
    /// 引用怎么认在 `miyu-models`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// 无功能那一档（`docs/00-设计.md` 第五节第 14 条）：这个会话一件工具都不给，回合开始什么都不注入。钉在会话上，中途不换。
    /// 全部功能的不写。
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub no_tools: bool,
    /// 从哪个会话的第几条分叉出来的（施工 C-8 上，`session/fork.md`）：历史引用那个会话到 `upto` 为止的有效历史，这一条的
    /// 序号是 `upto` 的下一个。不是分叉出来的不写；以前的日志没有这一格。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub forked: Option<Forked>,
}

/// 分叉的来源（施工 C-8 上）：父会话、分叉点。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Forked {
    /// 从哪个会话分叉。
    pub from: SessionId,
    /// 那个会话的第几条为止（含）。
    pub upto: Seq,
    /// 跑完几轮封存（施工 C-8 下）：接着开的一轮不另算。不封存的不写。LM-3 删了封存（只有分叉用，分叉 LM-2 删了），这一格
    /// 留着只为读得懂旧日志，读了不管。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seal_after: Option<NonZeroU32>,
    /// 分叉点在这一轮当中（fork 子代理，施工 A-F 上）：底子停在这一轮派它的那条 `message.assistant` 上，子会话头几条给那一步的
    /// 每个调用记占位结果、以 `forked` 收住这一轮。普通分叉不写：分叉点在一轮之外。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub within: Option<TurnId>,
}

/// `session.sealed`：分叉出来的会话跑满了 `seal_after` 轮，封存了（施工 C-8 下）。LM-3 删了封存：LiteMiyu 不再记它，旧日志里
/// 有的照读、不再有作用（不拒命令、照常开轮）。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSealed {}

/// `session.policy_changed`：换了策略快照，或者换了模型、思考强度、上下文上限、压缩触发线，也可以一起换。
/// 谁换的看事件的 `by`：人改的是配置，内核换的是目录变了、钉着的模型没了。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyChanged {
    /// 新的策略快照，下一个回合开始时生效（`02-内核.md` K3）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub policy: Option<ContentHash>,
    /// 换成的模型引用（施工 8-10，`models.md`「事件」）：模型 `<供应商>/<模型>`（LM-8 以前的日志里还可能是池 `@<池>`）。人换的 `by` 是人；钉着的没了、内核退回默认的 `by` 是内核，带着 `replaced`。下一个回合开始时生效，撤掉的回合里
    /// 的也算。以前的日志没有这一格。内核只记不解读。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// 钉着的引用没了、内核退回默认时写（施工 8-10）：原来那个。只和 `model` 一起出现（账本查）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replaced: Option<String>,
    /// 会话的思考强度换了（施工 SM-1，设计 51，`models.md`「事件」）：人换的，`by` 是人，下一个回合开始时生效，撤掉的回合里
    /// 的也算（照 `model`）。内核只记不解读。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<Effort>,
    /// 会话的上下文上限换了（施工 CTX-1，设计 68 第四节）：人换的，`by` 是人，下一次请求起生效，撤掉的回合里的也算（照
    /// `effort`）。写在最后一格，以前的日志没有这一格。内核只记，生效的窗口由执行器的路由取小的。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<ContextCap>,
    /// 会话钉的压缩触发线换了（施工 CT-1，`compaction.md` 第二条第 2 条）：窗口的百分之几，`null` 是去掉、回到跟着
    /// `compaction.line`。人换的，`by` 是人，下一个回合开始时会话那一层照它换策略快照里的数，撤掉的回合里的也算（照
    /// `context`）。写在最后一格，以前的日志没有这一格。内核只记不解读，范围协议那一头查过。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compaction_line: Option<LinePin>,
}

/// `session.policy_changed` 的 `compaction_line`（施工 CT-1）：会话钉的压缩触发线换成窗口的 `percent`%，`null` 是去掉（跟着配置）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinePin {
    /// 窗口的百分之几；`null` 是去掉。总是写。
    pub percent: Option<u64>,
}

/// `session.policy_changed` 的 `context`（施工 CTX-1，设计 68）：会话的上下文上限换成 `tokens`，`null` 是去掉（照模型的窗口）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextCap {
    /// 上限，token 数；`null` 是去掉。总是写。
    pub tokens: Option<u64>,
}

/// `session.policy_changed` 的 `effort`（施工 SM-1）：会话的那一档换成 `level`，`null` 是回到跟着配置。
///
/// 施工 8-18 记过同一格，多一个 `model`：会话给那一个模型记的一档；8-18（补）去掉了那一层，内核不再理它。那时的日志照样
/// 读得进、写回去一字不差，带 `model` 的照旧不理（[`Effort::session`]）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Effort {
    /// 施工 8-18 的日志才有：那一格是给哪个模型记的。SM-1 起不写。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// 换成的一档，规整过的名字（`off`、`on`，或者目录里的档位名）；`null` 是回到跟着配置。总是写。
    pub level: Option<String>,
}

impl Effort {
    /// 会话这一层的一档（SM-1 写的）：外层 `Some` 是这一条换了会话的一档，里面 `None` 是回到跟着配置；8-18 那种带 `model` 的
    /// 是 `None`，不算。
    pub fn session(&self) -> Option<Option<&str>> {
        self.model.is_none().then_some(self.level.as_deref())
    }
}

/// `session.meta_changed`：改了哪项写哪项。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetaChanged {
    /// 新的标题。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// 置顶还是取消置顶。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pinned: Option<bool>,
}

/// `session.workspace_changed`：人换了会话在哪个目录干活（施工 9-7 上，`docs/blueprint/kernel/events-bodies.md`）。工作区是会话的
/// 属性：只有人明确换才变（`session.set_workspace`、`/workspace`），头每句话报的目录不再换它。`by` 是换的人；回合进行中换的带上
/// 这个回合，这一轮里照旧用原来的，下一轮开始照新的。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceChanged {
    /// 新的工作目录：协议那一头判过太不太宽、换成了实际用的，人看到的那种写法。
    pub cwd: String,
    /// 新的加进来的目录：写了的整份换掉（空的是去掉全部），没写的照旧。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dirs: Option<Vec<String>>,
}

/// `command.ran`：人用了一个斜杠命令（施工 O-6，`docs/blueprint/kernel/events-bodies.md`）。谁发的写在 `by`，`cause` 是派生的
/// 命令编号（`<那个命令>/ran`）。只给人和聊天记录看，不进她的上下文；执行了的才记，被拒的什么都不记。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommandRan {
    /// 人打的原文，例如 `/reset`。
    pub text: String,
    /// 执行了哪个命令，照它的名字（别名换成正名），例如 `clear`。
    pub command: String,
}

/// `session.recapped`：一句回顾（施工 3-8 四补，`docs/blueprint/kernel/session.md`「回顾」）。头要的，推给所有订阅着的头，
/// 不进她的上下文；不带回合编号。`by` 是内核，`cause` 是要它的那个命令。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionRecapped {
    /// 那一句：她写的，去掉了前后空白，不是空的。
    pub text: String,
    /// 照到第几条：喂进回顾请求的最新那一条消息的序号，在这一条之前（账本查）。下一次要回顾时照到的还是它，交回这一句、
    /// 不再请求。
    pub upto: Seq,
}

#[cfg(test)]
mod tests;
