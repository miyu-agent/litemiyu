//! 环境和状态的事实：写成什么、该不该注入（`docs/designs/08-上下文投影.md` 第五节 C10、
//! 「环境和状态的事实怎么写」）。
//!
//! 现在有三类，都由内核注入，一类一块：环境（`env`：时间、时区、工作目录）、会话编号（`session`，施工 1-13 再补）和
//! 模型（`model`）。每个边界查一遍：每一块和有效历史里同一个来源、同一类的最近一块比，逐字节相同就不注入（[`changed`]）。
//! 没有权限级别那一块：只有完全放开一种（`docs/00-设计.md` 第五节第 6 条）。在哪些边界查，是回合状态机和
//! 压缩的事（施工 2-3、2-7、M6）。
//! 会话编号一个会话里不变，所以只在第一轮、压缩以后、撤掉带着它的那一轮以后注入；它不并进环境那一块：环境每过整点
//! 重发，编号跟着重发就白花。
//!
//! 还有两块不是环境和状态，是发生了的事：回复被出错打断了（`reply_cut`，施工 3-5 下），回复写到了输出上限、调用没派
//! （`output_limit`，施工 3-10）。它们跟在半截回复后面，每次都注入，不和以前的比。
//!

use std::collections::BTreeMap;

mod env;
mod model;

pub use model::Identity;

use crate::kernel::event::{Body, ContextInjected};
use crate::kernel::history::History;
use crate::kernel::id::{FactKind, ModuleId, SessionId, TurnId};
use crate::kernel::origin::By;
use crate::kernel::template::{Template, TemplateError};
use crate::kernel::time::{Timestamp, UtcOffset};

/// 事实的模板：环境（`core/facts/env.txt`）、会话编号（`core/facts/session.txt`），和回复被出错打断的那一句
/// （`core/facts/reply-cut.txt`）。
///
/// 由执行器从资源目录读好交进来，造会话时读一次，冻结在会话上（内核 K3）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FactTemplates {
    /// 环境那一块，字段是 `time`、`timezone`、`cwd`。
    env: Template,
    /// 回复被出错打断的那一句，没有字段。
    reply_cut: Template,
    /// 会话编号那一块，字段是 `id`（施工 1-13 再补）。以前造的快照里没有这份模板，是没有：那些会话不注入这一块。
    session: Option<Template>,
    /// 回复写到了输出上限、调用没派的那一句，没有字段（施工 3-10）。以前造的快照里没有：那些会话照样不派、接着请求，不加
    /// 这一句。
    output_limit: Option<Template>,
    /// 模型那一块，字段是 `provider`、`name`、`effort`（施工 E-M，`facts/model.rs`）。以前造的快照里没有，没有这一块。
    model: Option<Template>,
    /// 安静（无功能那一档，`docs/00-设计.md` 第五节第 14 条）：回合开始的环境、会话编号、模型一块都不注入。
    quiet: bool,
}

/// 会话所在的环境：时区和工作目录（`02-内核.md` 第六节「回合怎么开、请求怎么发」第 6 条）。
///
/// 造会话时交进来，执行器报「环境变了」就换掉。时间不在这里，取边界上那条输入到的时刻。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Environment {
    /// 所在的时区，取自执行器。
    pub offset: UtcOffset,
    /// 工作目录，头报上来的、人看到的那种写法，例如 `~/src/miyu`。内核不改写它。
    pub cwd: String,
    /// 加进来的目录：和工作区一样能读能写（施工 5-10 上）。照头报的原样，内核不改写，也不写进给她看的事实。
    pub dirs: Vec<String>,
}

impl FactTemplates {
    /// 读三个模板；会话编号那一份没有的（以前造的快照），不注入会话编号那一块。读好以后拿全部字段试着换一次：少了字段当场
    /// 报错，回合里就不会再出这种错。
    ///
    /// # Errors
    ///
    /// 模板的写法坏了，或者要了这一类没有的字段，返回 [`TemplateError`]，写明哪里坏了。
    pub fn new(
        env: &str,
        reply_cut: &str,
        session: Option<&str>,
    ) -> Result<FactTemplates, TemplateError> {
        let templates = FactTemplates {
            env: Template::parse(env)?,
            reply_cut: Template::parse(reply_cut)?,
            session: session.map(Template::parse).transpose()?,
            output_limit: None,
            model: None,
            quiet: false,
        };
        templates.env.render(&env_fields("", "", ""))?;
        templates.reply_cut.render(&BTreeMap::new())?;
        if let Some(session) = &templates.session {
            session.render(&session_fields(""))?;
        }
        Ok(templates)
    }

    /// 一个边界上该注入的几块，照注入的先后：环境、会话编号（没有这份模板的不查）、模型。每一块和有效历史里内核记的同一类
    /// 最近一块比原文，一样的不注入（[`changed`]）。时刻取边界上那条输入的，环境、编号取会话现在的（`kernel/request.md`
    /// 「事实」第 2 到 4 条）。
    pub fn boundary(
        &self,
        history: &History,
        now: Timestamp,
        environment: &Environment,
        (session, identity): (&SessionId, Option<&Identity>),
    ) -> Vec<ContextInjected> {
        if self.quiet {
            return Vec::new();
        }
        let fresh = |fact: ContextInjected| unseen(history, &By::Kernel, &fact).then_some(fact);
        [
            fresh(self.env(now, environment)),
            self.session(session).and_then(fresh),
            identity
                .and_then(|identity| self.model(identity))
                .and_then(fresh),
        ]
        .into_iter()
        .flatten()
        .collect()
    }

    /// 带上回复写到了输出上限的那一句（施工 3-10）：`None` 的不加。读好以后试换一次。
    ///
    /// # Errors
    ///
    /// 写法坏了、要了字段，返回 [`TemplateError`]。
    pub fn with_output_limit(mut self, text: Option<&str>) -> Result<FactTemplates, TemplateError> {
        let template = text.map(Template::parse).transpose()?;
        if let Some(template) = &template {
            template.render(&BTreeMap::new())?;
        }
        self.output_limit = template;
        Ok(self)
    }

    /// 安静下来（无功能那一档）：回合开始一块事实都不注入（[`FactTemplates::boundary`] 交回没有）。
    #[must_use]
    pub fn quiet(mut self) -> FactTemplates {
        self.quiet = true;
        self
    }

    /// 回复被出错打断的那一块：跟在半截回复后面（施工 3-5 下）。
    ///
    /// # Panics
    ///
    /// 实际不会 panic：造的时候已经试换过；`reply_cut` 这个类别名也合写法。
    pub fn reply_cut(&self) -> ContextInjected {
        ContextInjected {
            refs: Vec::new(),
            after: false,
            kind: kind("reply_cut"),
            text: self
                .reply_cut
                .render(&BTreeMap::new())
                .expect("造的时候试换过，没有字段"),
        }
    }

    /// 回复写到了输出上限、调用没派的那一块（施工 3-10）：跟在记成被打断的回复后面。以前造的快照没有这份模板，没有这一块。
    ///
    /// # Panics
    ///
    /// 实际不会 panic：造的时候已经试换过；`output_limit` 这个类别名也合写法。
    pub fn output_limit(&self) -> Option<ContextInjected> {
        let template = self.output_limit.as_ref()?;
        Some(ContextInjected {
            refs: Vec::new(),
            after: false,
            kind: kind("output_limit"),
            text: template
                .render(&BTreeMap::new())
                .expect("造的时候试换过，没有字段"),
        })
    }

    /// 会话编号那一块：这个会话自己的编号，子会话写它自己的（施工 1-13 再补）。以前造的快照没有这份模板，没有这一块。
    ///
    /// # Panics
    ///
    /// 实际不会 panic：造的时候已经拿全部字段试换过；`session` 这个类别名也合写法。
    pub fn session(&self, id: &SessionId) -> Option<ContextInjected> {
        let template = self.session.as_ref()?;
        Some(ContextInjected {
            refs: Vec::new(),
            after: false,
            kind: kind("session"),
            text: template
                .render(&session_fields(id.as_str()))
                .expect("造的时候试换过，字段都有"),
        })
    }
}

/// 这几块里该注入的，照原来的先后：和有效历史里同一个来源、同一类的最近一块比，
/// 逐字节相同的去掉（08 C10）。
///
/// 比的是最近的那一块，不是随便哪一块：先是 A，一个边界变成 B，下一个边界又回到 A，
/// 她最近看到的是 B，A 要重新注入。压缩替掉的、撤销掉的不在有效历史里，也就不算。
///
/// 边界上的权限那一块不走它，比的是级别（[`FactTemplates::boundary`]，施工 2-7 补）。
pub fn changed(history: &History, by: &By, facts: Vec<ContextInjected>) -> Vec<ContextInjected> {
    facts
        .into_iter()
        .filter(|fact| unseen(history, by, fact))
        .collect()
}

/// 这一块和有效历史里同一个来源、同一类的最近一块不一样，或者那一类还没有过。
fn unseen(history: &History, by: &By, fact: &ContextInjected) -> bool {
    latest(history, by, &fact.kind) != Some(fact.text.as_str())
}

/// 有效历史里由模块注入的一块（施工 R-4 上）：回合开始交给挂接点（`Action::RunTurnStartHooks` 的 `present`），模块照它定
/// 这一段上下文里还要不要交。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Present {
    /// 哪个模块注入的。
    pub module: ModuleId,
    /// 这一块的类别。
    pub kind: FactKind,
    /// 这一块带着的编号（[`ContextInjected::refs`]），原样。
    pub refs: Vec<String>,
}

/// 有效历史里由模块注入的 `context.injected`，照日志的先后（施工 R-4 上）。和会话编号那一块同一个找法：最近一个检查点
/// 以后、撤掉的回合不算（[`History`] 就是这样的）；内核自己注入的不算。
pub fn present(history: &History) -> Vec<Present> {
    history
        .events()
        .iter()
        .filter_map(|event| match (&event.by, &event.body) {
            (By::Module(module), Body::ContextInjected(fact)) => Some(Present {
                module: module.id.clone(),
                kind: fact.kind.clone(),
                refs: fact.refs.clone(),
            }),
            _ => None,
        })
        .collect()
}

/// 触发 `turn` 这一轮的人话（施工 R-8，`memory.md` 第四条第 1 款）：回合开始交给挂接点（`Action::RunTurnStartHooks` 的
/// `said`），记忆照它联想。照这一轮 `turn.started` 的 `trigger` 在有效历史里找那条 `message.user`，是人说的
/// （[`By::is_person`]）才算，交回它的字（[`crate::kernel::block::words`]，原样，长短由挂接点自己截）。没有 `trigger` 的（人要的压缩
/// 单开的）、触发的不是消息的（回报）、不是人说的（别的会话、子代理、别的 harness）、只有附件没有字的，没有。排着的几句一起
/// 开的一轮只取触发的那一句（最后一句）：前面几句在她忙着时就进了上下文，回合索引也是这个口径。
pub fn said(history: &History, turn: TurnId) -> Option<String> {
    // 这一轮的 `turn.started` 和触发它的那句都在末尾附近：从后往前找，长会话每开一轮不用扫一整遍。
    let events = history.events();
    let trigger = events.iter().rev().find_map(|event| match &event.body {
        Body::TurnStarted(started) if event.seq == turn.started() => started.trigger,
        _ => None,
    })?;
    let words = events.iter().rev().find_map(|event| match &event.body {
        Body::MessageUser(message) if event.seq == trigger && event.by.is_person() => {
            Some(crate::kernel::block::words(&message.blocks))
        }
        _ => None,
    })?;
    (!words.is_empty()).then_some(words)
}

/// 有效历史里，这个来源、这一类的最近一块的原文。
fn latest<'a>(history: &'a History, by: &By, kind: &FactKind) -> Option<&'a str> {
    history
        .events()
        .iter()
        .rev()
        .find_map(|event| match &event.body {
            Body::ContextInjected(fact) if &event.by == by && &fact.kind == kind => {
                Some(fact.text.as_str())
            }
            _ => None,
        })
}

fn env_fields<'a>(time: &'a str, timezone: &'a str, cwd: &'a str) -> BTreeMap<&'a str, &'a str> {
    BTreeMap::from([("time", time), ("timezone", timezone), ("cwd", cwd)])
}

fn session_fields(id: &str) -> BTreeMap<&str, &str> {
    BTreeMap::from([("id", id)])
}

/// 内核自己用的类别名，都合写法。
fn kind(name: &str) -> FactKind {
    FactKind::parse(name).expect("内核自己用的类别名合写法")
}

#[cfg(test)]
mod tests;
