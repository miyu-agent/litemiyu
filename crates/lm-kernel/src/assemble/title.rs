//! 人要求重起标题的请求（施工 TT-1，`docs/blueprint/kernel/request.md`「起标题的请求」）：照回顾的请求的写法（`recap.rs`），
//! 单独一次辅助请求，不接主对话的前缀、不带 system 和工具面，一条 user：指令接对话记录。
//!
//! 喂最近几轮（[`recent`]）：从新往旧数，答过的至多 [`RECENT_TURNS`] 轮，还没走完的那一轮（有效历史里没有它的 `turn.ended`）
//! 说过的不算回答，最新那一轮还没答的不带；一轮怎么取、怎么写照回顾的（`recap::exchanges_of`）。标签、截断的记号借回顾的；
//! 整份连指令至多 `tokens` 个 token（字节/4）：放不下的先整轮去掉最老的、最前写回顾的那一行省略了，只剩一轮还放不下的两段
//! 各留头尾、截掉中间（`recap::transcript`），最后整份截到上限、只在字的边界上截。

use std::collections::BTreeSet;

use crate::kernel::block::{Block, Text};
use crate::kernel::estimate::BYTES_PER_TOKEN;
use crate::kernel::event::Body;
use crate::kernel::history::History;
use crate::kernel::id::{Seq, TurnId};
use crate::kernel::request::{Message, Request};

use crate::assemble::recap::{self, Exchange};
use crate::assemble::texts::Texts;

/// 人要求重起时喂最近几轮（2026-11-11 项目主人定：只喂最近几轮，不喂第一轮；三轮是施工 TT-1 定的）。
pub(crate) const RECENT_TURNS: usize = 3;

/// 人要求重起的请求（施工 TT-1），和它照到的那一条：喂进去的最新那个回答的序号。喂最近答过的至多 [`RECENT_TURNS`] 轮，
/// 还没走完的那一轮不带：它中间一步说的话不算回答，开它的那句话也就落在最后、跟着去掉。快照里没有起标题的字、没有回顾的
/// 标签的，她一个带正文的回复都没有的，没有。
pub(crate) fn recent(history: &History, texts: &Texts) -> Option<(Request, Seq)> {
    let ended: BTreeSet<TurnId> = history
        .events()
        .iter()
        .filter(|event| matches!(event.body, Body::TurnEnded(_)))
        .filter_map(|event| event.turn)
        .collect();
    let mut entries = recap::entries(history, texts);
    entries
        .retain(|entry| !entry.assistant || entry.turn.is_some_and(|turn| ended.contains(&turn)));
    let mut turns = recap::exchanges_of(&entries, RECENT_TURNS)?;
    if turns.last().is_some_and(|turn| turn.assistant.is_empty()) {
        turns.pop();
    }
    let upto = turns.iter().filter_map(|turn| turn.last).max()?;
    Some((written(&turns, texts)?, upto))
}

/// 照这几轮写成请求：指令接对话记录，整份至多 `tokens` 个 token。快照里没有起标题的字、没有回顾的标签的，没有。
fn written(turns: &[Exchange], texts: &Texts) -> Option<Request> {
    let title = texts.title.as_ref()?;
    let labels = texts.recap.as_ref()?;
    let limit = usize::try_from(title.tokens.saturating_mul(BYTES_PER_TOKEN)).unwrap_or(usize::MAX);
    let room = limit.saturating_sub(title.instruction.len());
    let mut transcript = recap::transcript(turns, labels, room);
    transcript.truncate(transcript.floor_char_boundary(room));
    let text = format!("{}{transcript}", title.instruction);
    Some(Request {
        tools: Vec::new(),
        system: String::new(),
        messages: vec![Message::User {
            blocks: vec![Block::Text(Text { text })],
        }],
        stable: 0,
        continuation: false,
        described: Default::default(),
        output_cap: None,
        used: None,
    })
}

#[cfg(test)]
mod tests;
