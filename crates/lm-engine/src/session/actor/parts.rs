//! 头问上下文一块块的、累计用量照块摊（Miyu 施工 CX-1 上、CX-3；Miyu 的 `actor/persona/context.rs`、`actor/persona/spent.rs`）：
//! 内核的明细当场要；闲着收成槽的先从日志读回来（`shelf.rs`），算完照旧收回去。累计的读整份日志、在阻塞线程里回放，不挡
//! 收件箱：每次主请求在它看到的那一条（`seen`）记下那一刻的上下文一块块的，连同供应商报的输入。
//!
//! - 主请求：`model.called` 里不是辅助请求（没有 `purpose`）、不是压缩的摘要请求，发出去了（有供应商、模型）。
//! - 那一刻用的快照：造会话的那份；换了策略的（`session.policy_changed` 带 `policy`）照内核的规矩，到下一个回合开始才换上。
//! - 每份快照只备一次：system、工具照组装一段空的历史估，system 照 [`split`] 拆。

use std::collections::BTreeMap;

use lm_kernel::kernel::estimate;
use lm_kernel::kernel::event::{Body, Event, ModelCalled};
use lm_kernel::kernel::history::History;
use lm_kernel::kernel::id::{ContentHash, Seq};
use lm_kernel::kernel::session::{Frame, breakdown_in};
use lm_kernel::policy::Snapshot;

use super::{Actor, Stop, answer};
use crate::session::blocking::blocking;
use crate::session::handle::parts::Asked;
use crate::session::handle::{Charged, Context, Spent, SystemPart};

impl Actor {
    /// 办头问的那一样。读日志的线程 panic 了的停下（同读回历史）。
    pub(super) async fn parts(&mut self, asked: Asked) -> Result<(), Stop> {
        match asked {
            Asked::Now(reply) => {
                let shelved = self.session.shelved().is_some();
                if shelved {
                    self.unshelve().await?;
                }
                let breakdown = self.session.breakdown();
                if shelved {
                    self.rest();
                }
                let (system, offered) = match &self.kit {
                    Some(kit) => (
                        split(&kit.snapshot, breakdown.system),
                        lm_kernel::policy::sent_names(&kit.snapshot.tools),
                    ),
                    None => (vec![(SystemPart::Other, breakdown.system)], Vec::new()),
                };
                answer(
                    reply,
                    Context {
                        breakdown,
                        system,
                        offered,
                    },
                );
            }
            Asked::Spent(reply) => {
                let (Some(kit), Ok(events)) = (
                    self.kit.as_ref().map(|kit| kit.blobs.clone()),
                    self.events_from(Seq::FIRST).await?,
                ) else {
                    answer(reply, Spent::default());
                    return Ok(());
                };
                tokio::spawn(async move {
                    let spent = blocking(move || {
                        replay(events, |hash| {
                            let bytes = kit.get(hash).ok()?;
                            prepare(&Snapshot::from_bytes(&bytes).ok()?)
                        })
                    })
                    .await;
                    answer(reply, spent);
                });
            }
        }
        Ok(())
    }
}

/// 把 system 拆开（Miyu 的 `split`）：快照里单存的几块，原文在拼好的 system 里找得到的才算（找不到的是快照和发的对不上，
/// 不认），剩下的都是其他。
pub(super) fn split(snapshot: &Snapshot, total: u64) -> Vec<(SystemPart, u64)> {
    let stored = [
        (SystemPart::Skills, snapshot.skills.as_deref()),
        (SystemPart::Profile, snapshot.profile.as_deref()),
    ]
    .into_iter()
    .filter_map(|(part, text)| text.map(|text| (part, text)))
    .chain(
        snapshot
            .appended
            .iter()
            .map(|(id, text)| (SystemPart::Appended(id.clone()), text.as_str())),
    );
    let mut parts = Vec::new();
    let mut known = 0u64;
    for (part, text) in stored {
        if text.is_empty() || !snapshot.system.contains(text) {
            continue;
        }
        let tokens = estimate::text(text);
        known = known.saturating_add(tokens);
        parts.push((part, tokens));
    }
    parts.push((SystemPart::Other, total.saturating_sub(known)));
    parts
}

/// 一份快照备好的：不随对话变的几块、system 拆开、工具面。
pub(super) struct Prepared {
    pub(super) frame: Frame,
    pub(super) system: Vec<(SystemPart, u64)>,
    pub(super) offered: Vec<String>,
}

/// 备一份快照：造得出策略的才有。
fn prepare(snapshot: &Snapshot) -> Option<Prepared> {
    let frame = Frame::of(&snapshot.policy().ok()?);
    Some(Prepared {
        system: split(snapshot, frame.system),
        offered: lm_kernel::policy::sent_names(&snapshot.tools),
        frame,
    })
}

/// 照先后回放 `events`：每次主请求一份。快照照编号用 `prepare` 备，一份只备一次；备不出来的那几次 system 照 0、没有工具面。
pub(super) fn replay(
    events: Vec<Event>,
    mut prepare: impl FnMut(&ContentHash) -> Option<Prepared>,
) -> Spent {
    // 先把主请求照它看到的那一条排好：回放到那一条时记下那一刻。
    let mut waiting: BTreeMap<Seq, Vec<Option<u64>>> = BTreeMap::new();
    let mut spent = Spent::default();
    for event in &events {
        if let Body::ModelCalled(called) = &event.body
            && main(called)
        {
            spent.requests += 1;
            let usage = called.usage.as_ref();
            spent.output += usage.map_or(0, |usage| usage.output);
            let input = usage.map(|usage| usage.uncached + usage.cache_read + usage.cache_write);
            waiting.entry(called.seen).or_default().push(input);
        }
    }
    let mut prepared: BTreeMap<ContentHash, Option<Prepared>> = BTreeMap::new();
    let (mut active, mut pending): (Option<ContentHash>, Option<ContentHash>) = (None, None);
    let mut history = History::default();
    let mut last: Option<Seq> = None;
    for event in events {
        let seq = event.seq;
        match &event.body {
            Body::SessionCreated(created) => active = Some(created.policy.clone()),
            Body::PolicyChanged(changed) if changed.policy.is_some() => {
                pending.clone_from(&changed.policy);
            }
            Body::TurnStarted(_) => {
                if let Some(next) = pending.take() {
                    active = Some(next);
                }
            }
            _ => {}
        }
        history.append(event);
        last = Some(seq);
        if let Some(inputs) = waiting.remove(&seq) {
            let ready = ready(active.as_ref(), &mut prepared, &mut prepare);
            charge(&mut spent, inputs, &history, ready);
        }
    }
    // 看到的那一条日志里没有的（读到一半的日志），照最后那一刻算，总数不丢。
    let rest: Vec<Option<u64>> = waiting.into_values().flatten().collect();
    if !rest.is_empty() && last.is_some() {
        let ready = ready(active.as_ref(), &mut prepared, &mut prepare);
        charge(&mut spent, rest, &history, ready);
    }
    spent
}

/// 这一刻用的那份快照备好的：一份只备一次。
fn ready<'a>(
    active: Option<&ContentHash>,
    prepared: &'a mut BTreeMap<ContentHash, Option<Prepared>>,
    prepare: &mut impl FnMut(&ContentHash) -> Option<Prepared>,
) -> Option<&'a Prepared> {
    let hash = active?;
    prepared
        .entry(hash.clone())
        .or_insert_with(|| prepare(hash))
        .as_ref()
}

/// 这一刻的几次主请求：报了用量的各记一份。
fn charge(
    spent: &mut Spent,
    inputs: Vec<Option<u64>>,
    history: &History,
    ready: Option<&Prepared>,
) {
    let inputs: Vec<u64> = inputs.into_iter().flatten().collect();
    if inputs.is_empty() {
        return;
    }
    let context = match ready {
        Some(ready) => Context {
            breakdown: breakdown_in(&ready.frame, history, &ready.frame.price),
            system: ready.system.clone(),
            offered: ready.offered.clone(),
        },
        None => {
            let frame = Frame {
                system: 0,
                tools: Vec::new(),
                demos: 0,
                price: estimate::Flat { image: 0, file: 0 },
            };
            Context {
                breakdown: breakdown_in(&frame, history, &frame.price),
                system: Vec::new(),
                offered: Vec::new(),
            }
        }
    };
    for input in inputs {
        spent.charged.push(Charged {
            context: context.clone(),
            input,
        });
    }
}

/// 主请求：不是辅助请求、不是压缩的摘要请求，发出去了（有供应商、模型）。
fn main(called: &ModelCalled) -> bool {
    called.purpose.is_none()
        && called.compaction.is_none()
        && called.endpoint.is_some()
        && called.model.is_some()
}

#[cfg(test)]
mod tests;
