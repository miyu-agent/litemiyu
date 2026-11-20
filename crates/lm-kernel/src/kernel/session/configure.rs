//! 会话里换模型（施工 8-10，`docs/blueprint/models.md`「怎么走」第六条，`kernel/session.md`「换模型」）、换思考强度（施工
//! SM-1，设计 51，「怎么走」第十一条）：人换，内核记；和现在的一样的不记，下一个回合开始时生效。
//!
//! 会话的引用从日志算：`session.created` 的 `model`，被后来带 `model` 的 `session.policy_changed` 盖掉，撤掉的回合里的也算
//! （换模型不是对话的一部分，照改标题）。内核只存字，不解读：回合开始时交给执行器照这一轮的配置重新解析，钉着的没了、
//! 执行器退回了默认的，内核在注入前面记一条，`by` 是内核。
//!
//! 思考强度一样从日志算（施工 SM-1）：每条带 `effort` 的 `session.policy_changed` 换会话的那一档，`null` 是回到跟着配置；
//! 撤掉的回合里的也算，换模型不清它。施工 8-18 那时记的每个模型一格（带 `model`）不认（`event/session.rs` 的
//! `Effort::session`）。回合开始时执行器照 [`Session::effort`] 取，一次请求用不用它由执行器照真发的那个模型的档位定。
//!
//! 上下文上限照同一个办法从日志算（施工 CTX-1，设计 68 第四节）：每条带 `context` 的 `session.policy_changed` 换上限，`null`
//! 是去掉；撤掉的回合里的也算，换模型不清它。执行器照 [`Session::context`] 交给路由，生效的窗口由路由取小的。
//!
//! 钉的压缩触发线照同一个办法从日志算（施工 CT-1，`compaction.md` 第二条第 2 条）：每条带 `compaction_line` 的
//! `session.policy_changed` 换会话钉的百分数，`null` 是回到跟着 `compaction.line`；撤掉的回合里的也算，换模型不清它。回合开始
//! 时会话那一层照 [`Session::pinned_line`]（没钉的照配置）换策略快照里的数，换了的记成换快照。

use super::action::Action;
use super::{Session, accepted};
use crate::kernel::event::{Body, ContextCap, Effort, Event, LinePin, PolicyChanged};
use crate::kernel::id::{CommandId, Seq};
use crate::kernel::origin::By;
use crate::kernel::time::Timestamp;

/// 会话的引用，和最近一次换模型写在第几条：每追加一条记一次，载入时照整份日志一路算（`load.rs`）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Reference {
    /// 会话现在的引用；以前的日志没有记下的、造的时候连 `models.chat` 都没配的没有。
    model: Option<String>,
    /// 最近一条带 `model` 的 `session.policy_changed` 的序号：写在它前面的自动压缩的暂停、失败不再算（`compaction.md`
    /// 第十条第 6 条，「施工时定的」8-10）。没换过的没有。只换思考强度的不算。
    changed: Option<Seq>,
    /// 会话记的思考强度（施工 SM-1）：没记的、回到跟着配置的没有。
    effort: Option<String>,
    /// 会话的上下文上限，token 数（施工 CTX-1）：没设的、去掉了的没有。
    context: Option<u64>,
    /// 会话钉的压缩触发线，窗口的百分之几（施工 CT-1）：没钉的、去掉了的没有（跟着 `compaction.line`）。
    line: Option<u64>,
}

impl Reference {
    /// 记下一条：`session.created` 定开头的引用，带 `model` 的 `session.policy_changed` 换掉它。
    pub(super) fn note(&mut self, event: &Event) {
        match &event.body {
            Body::SessionCreated(created) => {
                self.model.clone_from(&created.model);
            }
            // 空的是回到跟着 `models.chat`（施工 8-10 再补）。
            Body::PolicyChanged(changed) => {
                if let Some(model) = &changed.model {
                    self.model = Some(model.clone()).filter(|model| !model.is_empty());
                    self.changed = Some(event.seq);
                }
            }
            _ => {}
        }
        if let Body::PolicyChanged(changed) = &event.body {
            // 换思考强度（施工 SM-1）：和换模型可以在同一条里，8-18 那时的不认。
            if let Some(level) = changed.effort.as_ref().and_then(Effort::session) {
                self.effort = level.map(str::to_string);
            }
            // 换上下文上限（施工 CTX-1）：同上，可以和别的一起。
            if let Some(cap) = &changed.context {
                self.context = cap.tokens;
            }
            // 钉压缩触发线（施工 CT-1）：同换上下文上限。
            if let Some(pin) = &changed.compaction_line {
                self.line = pin.percent;
            }
        }
    }

    /// 会话现在的引用。
    pub(super) fn current(&self) -> Option<&str> {
        self.model.as_deref()
    }

    /// 会话记的思考强度：没记的没有。
    pub(super) fn effort(&self) -> Option<&str> {
        self.effort.as_deref()
    }

    /// 会话的上下文上限：没设的没有。
    pub(super) fn context(&self) -> Option<u64> {
        self.context
    }

    /// 会话钉的压缩触发线：没钉的没有。
    pub(super) fn line(&self) -> Option<u64> {
        self.line
    }

    /// 第 `seq` 条写在最近一次换模型后面；没换过的都算。熔断照它只看换过去以后的暂停、失败。
    pub(super) fn after_change(&self, seq: Seq) -> bool {
        self.changed.is_none_or(|changed| seq > changed)
    }
}

impl Session {
    /// 会话现在的引用：模型或 `@池`（施工 8-10）。`session.created` 的 `model`，被后来带 `model` 的 `session.policy_changed`
    /// 盖掉，撤掉的回合里的也算；以前的日志没有的、造的时候连 `models.chat` 都没配的没有（跟着 `models.chat`）。只读：执行器
    /// 载入时照它造路由（`session/actor.md` 第 2 条）。
    pub fn reference(&self) -> Option<&str> {
        self.reference.current()
    }

    /// 换模型：和现在的一样的接受，什么都不记，编号照样记下；不一样的记一条 `session.policy_changed`，`by` 是换的人，回合
    /// 进行中的带上这个回合，空闲时没有。`model` 空的是回到跟着 `models.chat`（施工 8-10 再补），本来就跟着的什么都不记。什么
    /// 时候来都收，改回文件的时候照「命令和回应」第 5 条拒（`session.rs`）。
    pub(super) fn configure(
        &mut self,
        id: CommandId,
        by: By,
        at: Timestamp,
        model: String,
    ) -> Vec<Action> {
        if self.reference() == Some(model.as_str()).filter(|model| !model.is_empty()) {
            self.recent.insert(id.clone(), Vec::new());
            return vec![accepted(id, Vec::new())];
        }
        let body = Body::PolicyChanged(Box::new(PolicyChanged {
            model: Some(model),
            ..PolicyChanged::default()
        }));
        let event = self.record(at, by, Some(id.clone()), body);
        self.accept(id, vec![event.seq]);
        vec![Action::Append(vec![event])]
    }

    /// 会话记的思考强度（施工 SM-1，设计 51）：带 `effort` 的 `session.policy_changed` 里最后那一档，撤掉的回合里的也算；
    /// 没记的、回到跟着配置的没有（照配置）。只读：执行器载入时照它造路由，回合开始时交给路由；这个模型有没有这一档由路由查。
    pub fn effort(&self) -> Option<&str> {
        self.reference.effort()
    }

    /// 换思考强度（施工 SM-1）：`level` 是 `None` 的回到跟着配置。和现在的一样的接受，什么都不记，编号照样记下；不一样的记
    /// 一条 `session.policy_changed`，只写 `effort`，`by` 是换的人，回合进行中的带上这个回合。档位协议那一头查过。
    pub(super) fn set_effort(
        &mut self,
        id: CommandId,
        by: By,
        at: Timestamp,
        level: Option<String>,
    ) -> Vec<Action> {
        if self.effort() == level.as_deref() {
            self.recent.insert(id.clone(), Vec::new());
            return vec![accepted(id, Vec::new())];
        }
        let body = Body::PolicyChanged(Box::new(PolicyChanged {
            effort: Some(Effort { model: None, level }),
            ..PolicyChanged::default()
        }));
        let event = self.record(at, by, Some(id.clone()), body);
        self.accept(id, vec![event.seq]);
        vec![Action::Append(vec![event])]
    }

    /// 会话的上下文上限，token 数（施工 CTX-1，设计 68）：带 `context` 的 `session.policy_changed` 里最后那一个，撤掉的回合里的
    /// 也算；没设的、去掉了的没有（照模型的窗口）。只读：执行器载入时照它造路由，变了交给路由，生效的窗口由路由取小的。
    pub fn context(&self) -> Option<u64> {
        self.reference.context()
    }

    /// 会话钉的压缩触发线，窗口的百分之几（施工 CT-1）：带 `compaction_line` 的 `session.policy_changed` 里最后那一个，撤掉的回合里的也算；
    /// 没钉的、去掉了的没有（照 `compaction.line`）。只读：回合开始时会话那一层照它换策略快照里的数；也写进给头看的会话状态。
    pub fn pinned_line(&self) -> Option<u64> {
        self.reference.line()
    }

    /// 钉压缩触发线（施工 CT-1）：`percent` 是 `None` 的去掉。和现在的一样的接受，什么都不记，编号照样记下；不一样的记一条
    /// `session.policy_changed`，只写 `compaction_line`，`by` 是换的人，回合进行中的带上这个回合。范围协议那一头查过。
    pub(super) fn set_line(
        &mut self,
        id: CommandId,
        by: By,
        at: Timestamp,
        percent: Option<u64>,
    ) -> Vec<Action> {
        if self.pinned_line() == percent {
            self.recent.insert(id.clone(), Vec::new());
            return vec![accepted(id, Vec::new())];
        }
        let body = Body::PolicyChanged(Box::new(PolicyChanged {
            compaction_line: Some(LinePin { percent }),
            ..PolicyChanged::default()
        }));
        let event = self.record(at, by, Some(id.clone()), body);
        self.accept(id, vec![event.seq]);
        vec![Action::Append(vec![event])]
    }

    /// 换上下文上限（施工 CTX-1）：`tokens` 是 `None` 的去掉。和现在的一样的接受，什么都不记，编号照样记下；不一样的记一条
    /// `session.policy_changed`，只写 `context`，`by` 是换的人，回合进行中的带上这个回合。太小的协议那一头拒过。
    pub(super) fn set_context(
        &mut self,
        id: CommandId,
        by: By,
        at: Timestamp,
        tokens: Option<u64>,
    ) -> Vec<Action> {
        if self.context() == tokens {
            self.recent.insert(id.clone(), Vec::new());
            return vec![accepted(id, Vec::new())];
        }
        let body = Body::PolicyChanged(Box::new(PolicyChanged {
            context: Some(ContextCap { tokens }),
            ..PolicyChanged::default()
        }));
        let event = self.record(at, by, Some(id.clone()), body);
        self.accept(id, vec![event.seq]);
        vec![Action::Append(vec![event])]
    }

    /// 回合开始时执行器退回了默认（施工 8-10）：`from` 正是现在的引用、`to` 和它不一样的，记一条 `session.policy_changed`
    /// （`model` 是退回的，`replaced` 是原来的），`by` 是内核，`cause` 是回合的。对不上的不理：执行器交回来之前人又换了，
    /// 人换的算数，下一轮再解析（「施工时定的」8-10）。
    pub(super) fn fall_back(
        &mut self,
        at: Timestamp,
        cause: Option<CommandId>,
        replaced: Option<Replaced>,
    ) -> Option<Event> {
        let Replaced { from, to } = replaced?;
        if self.reference() != Some(from.as_str()) || from == to {
            return None;
        }
        let body = Body::PolicyChanged(Box::new(PolicyChanged {
            model: Some(to),
            replaced: Some(from),
            ..PolicyChanged::default()
        }));
        Some(self.record(at, By::Kernel, cause, body))
    }
}

/// 钉着的引用没了、执行器退回了默认（施工 8-10）：交回 [`super::Input::TurnStartHooksDone`] 的 `replaced`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Replaced {
    /// 原来的引用：回合开始时内核交出去的那一个。
    pub from: String,
    /// 退回的引用：这一轮的 `models.chat`。
    pub to: String,
}
