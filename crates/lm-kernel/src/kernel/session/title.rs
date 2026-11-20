//! 人要求重起标题（`session.retitle`，施工 TT-1，`docs/blueprint/kernel/session.md`「起标题」）：内核照这一刻落了盘的有效历史
//! 组装一次单独的辅助请求（`Assembler::retitle`，`kernel/request.md`「起标题的请求」），喂最近几轮，说完了记 `model.called`
//! （`purpose` 是 `title`）和 `session.meta_changed`。会话不自己起标题（2026-11-14 项目主人定，发版前砍的一轮）。
//!
//! - 什么时候起：收下就起，人起过名的也盖。有一次在路上的并进去，不再发；这时还组装不出来、回合正在进行的，等这一轮的
//!   `turn.ended` 落了盘再起；空闲着也组装不出来的拒绝，`nothing_to_title`。
//! - 不属于哪一轮：两条事件都不带回合编号，没有 `cause`，撤哪一轮都不跟着拿走，不进她的上下文。
//! - 说完了：正文取第一行、去掉前后空白，超过上限的截掉（不加省略号）。出错、没有正文的只记那一条 `model.called`；在路上
//!   时人改了名的，也只记那一条，不盖掉人起的。
//! - 人要的标题算人起的（施工 TT-2，2026-11-13 定）：`session.meta_changed` 的 `by` 是要它的那个命令的 `by`，同
//!   `set_meta`（会话列表照它算 `named`）；`model.called` 照旧是内核的。不带 `cause`：命令收下就回了，载入时照 `cause`
//!   认回命令编号会附上这一条，活着时却没有，两边对不上。
//! - 人要的没起成（施工 TT-3，2026-11-13 项目主人定）：超时、出错、取不出标题，标题不动，告诉要它的那一头（`by` 是要它
//!   的人）：Miyu 推一条 `session.retitle_failed` 的瞬时事件，LiteMiyu 交一个 [`Action::RetitleFailed`]（为什么见
//!   [`RetitleFailed`]）。辅助请求最多等多久归会话那一层（`lm-engine` 的 `session::port::ASIDE_LIMIT`），到时照出错送进来，
//!   这里不分。
//! - 旧日志里内核自己起过的标题（`by` 是内核的 `session.meta_changed`）和那几条 `model.called` 照旧载入：标题照它们算，
//!   不再数试过几次。

use super::Session;
use super::action::{Action, Reason, accepted, rejected};
use super::aside::{Aside, Finished};
use crate::kernel::block::Block;
use crate::kernel::event::{
    Body, CallError, Cost, Event, MetaChanged, Purpose, RetitleFailed, Said, Usage,
};
use crate::kernel::id::{CommandId, Seq, TurnId};
use crate::kernel::origin::By;
use crate::kernel::time::Timestamp;

/// 人要求的重起标题没起成时给人看的那一句（施工 TT-3）：字在 `core/human/<语言>.json` 的 `said` 下。
const RETITLE_FAILED: &str = "core/sessions/retitle-failed";

/// 起标题的账：从日志一条条算，活着时每追加一条记一次，载入时照日志再走一遍，算出来的一样。
#[derive(Debug, Default)]
pub(super) struct Naming {
    /// 最近一条有正文的回复在哪一轮。
    answered: Option<TurnId>,
    /// 人要求重起、还没起成（施工 TT-1），记着要它的是谁（施工 TT-2：起好的标题记成他起的）。只在内存里，载入以后没有。
    /// 标题一动（起好了、人又改了）就放下；没起成的也放下。
    asked: Option<By>,
}

impl Naming {
    /// 记下追加的一条。
    pub(super) fn note(&mut self, event: &Event) {
        match &event.body {
            Body::MetaChanged(changed) if changed.title.is_some() => self.asked = None,
            Body::MessageAssistant(reply) if said(&reply.blocks) => self.answered = event.turn,
            _ => {}
        }
    }
}

/// 这几块里有正文：去掉空白不是空的。
fn said(blocks: &[Block]) -> bool {
    blocks.iter().any(|block| match block {
        Block::Text(text) => !text.text.trim().is_empty(),
        _ => false,
    })
}

impl Session {
    /// 人要求重起、等着这一轮完的（施工 TT-1）就起（「落了盘」里，`turn.ended` 落了盘的回合是 `closed`）：随便哪一轮的
    /// `turn.ended` 落了盘、有效历史里有过带正文的回答就起，照这一刻落了盘的有效历史组装，名字是照到的那一条。组装不出来的
    /// 不起。
    pub(super) fn title_up(&mut self, closed: &[TurnId]) -> Option<Action> {
        self.policy.titles?;
        self.naming.answered?;
        let due = self.naming.asked.is_some() && !closed.is_empty();
        if self.titling.is_some() || self.restarting || !due {
            return None;
        }
        self.title_now()
    }

    /// 人要求重起标题（`session.retitle`，施工 TT-1）：收下就回，不等模型；标题经 `session.meta_changed` 到，`by` 是这里的
    /// `by`（施工 TT-2）。
    ///
    /// 有一次在路上的并进去；没有的当场照 [`Session::title_now`] 起；这时组装不出来、回合正在进行的等这一轮完
    /// （[`Session::title_up`]）；空闲着也组装不出来、策略里没有起标题的数的，拒绝，`nothing_to_title`。
    pub(super) fn retitle(&mut self, id: CommandId, by: By) -> Vec<Action> {
        if self.policy.titles.is_none() {
            return vec![rejected(id, Reason::NothingToTitle)];
        }
        self.naming.asked = Some(by);
        let started = match self.titling.is_some() {
            true => None,
            false => self.title_now(),
        };
        if started.is_none() && self.titling.is_none() && self.turn.is_none() {
            self.naming.asked = None;
            return vec![rejected(id, Reason::NothingToTitle)];
        }
        self.recent.insert(id.clone(), Vec::new());
        let mut actions = vec![accepted(id, Vec::new())];
        actions.extend(started);
        actions
    }

    /// 照这一刻落了盘的有效历史起一次：组装请求发出去（喂最近几轮）；组装不出来的没有。
    fn title_now(&mut self) -> Option<Action> {
        let history = self.history.until(self.stored?);
        let (request, upto) = self.policy.assembler.retitle(&history)?;
        self.titling = Some(Aside::new(upto, request.messages.len()));
        Some(Action::Aside {
            purpose: Purpose::Title,
            upto,
            request,
        })
    }

    /// 起标题的请求说完了：记 `model.called`（`by` 是内核）；写成了、要它的人还等着的（在路上时人改了名的不等了），接着记
    /// `session.meta_changed`（`by` 是要它的人，施工 TT-2），同一批。两条都不带回合编号、没有 `cause`。
    pub(super) fn title_ended(
        &mut self,
        at: Timestamp,
        upto: Seq,
        spent: (Option<Usage>, Option<Cost>),
        error: Option<CallError>,
    ) -> Vec<Action> {
        let Some(aside) = self.titling.take_if(|aside| aside.upto == upto) else {
            return Vec::new();
        };
        let next = self.ledger.next_seq();
        let Finished { called, text } = aside.finish(
            at,
            Purpose::Title,
            spent,
            error,
            next,
            "the title reply has no text",
        );
        let mut events = vec![self.record_aside(at, None, Body::ModelCalled(Box::new(called)))];
        let chars = self.policy.titles.map_or(0, |titles| titles.chars);
        // 人要求重起的那一次盖得掉人起的标题（施工 TT-1）；没起成的也放下，不留到下一轮。
        let asked = std::mem::take(&mut self.naming.asked);
        let failed = match (text, asked) {
            (Ok(text), Some(by)) => {
                events.push(self.titled(at, by, shorten(&text, chars)));
                None
            }
            // 人要的没起成（施工 TT-3）：告诉头，标题不动。
            (Err(error), Some(by)) => Some(retitle_failed(at, by, error)),
            // 在路上时人改了名（`asked` 跟着放下了）：不盖。
            (_, None) => None,
        };
        let mut actions = vec![Action::Append(events)];
        actions.extend(failed);
        actions
    }

    /// 记下起好的标题：一条只写 `title` 的 `session.meta_changed`，不带回合编号、没有 `cause`。`by` 是要它的人（施工
    /// TT-2：人要的算他起的，同 `set_meta`）。
    fn titled(&mut self, at: Timestamp, by: By, title: String) -> Event {
        let changed = MetaChanged {
            title: Some(title),
            pinned: None,
        };
        self.meta.note(&changed);
        let seq = self.ledger.next_seq();
        self.stamped(seq, at, None, (by, None), Body::MetaChanged(changed))
    }
}

/// 人要求的重起标题没起成（施工 TT-3）：Miyu 的 `session.retitle_failed`，`by` 是要它的人，不属于哪一轮、没有 `cause`
/// （命令收下就回过了）。给人看的那一句是 [`RETITLE_FAILED`]。
fn retitle_failed(at: Timestamp, by: By, error: CallError) -> Action {
    Action::RetitleFailed {
        at,
        by,
        failed: RetitleFailed {
            error,
            human: Said::new(RETITLE_FAILED),
        },
    }
}

/// 回复里的标题：第一行，去掉前后空白，超过 `chars` 个字（Unicode 字符）的截到 `chars` 个、再去掉末尾的空白，不加省略号。
/// `text` 已经去掉了前后空白、不是空的，第一行也就不是空的。
fn shorten(text: &str, chars: usize) -> String {
    let line = text.lines().next().unwrap_or_default().trim();
    let cut: String = line.chars().take(chars).collect();
    cut.trim_end().to_string()
}
