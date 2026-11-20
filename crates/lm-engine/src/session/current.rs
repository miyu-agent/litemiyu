//! 订阅那一刻「当前的」几样（施工 9-6 上，`docs/blueprint/session/actor.md`「推送和订阅」）：头按页读以后只拿最新一页，照
//! 整份日志算的几样算不对了，订阅的回应直接给：人设的权限、还在跑的任务、这个会话累计的用量和几样计数。和订阅在 actor 的
//! 同一步里拿：头之后照推过来的事件往上加，不重不漏。
//!
//! 累计的那几样（[`Tally`]）载入时照整份日志算一次，之后每落一批盘加上这一批，不为订阅读日志。口径同用量汇总的一行
//! （`miyu_store::usage::Spent::called`）：只算发出去了的请求（带供应商、模型的）；打断了、没报用量的照样算一次、用量记 0；
//! 有用量、没金额的算一次「没金额」。金额只算这个会话到现在花了多少（核心线第二趟：会话花费，主人 2026-11-14 恢复这一样；按天、按模型的
//! 汇总、换算成一个币种不做）。

use std::collections::BTreeMap;

use lm_kernel::kernel::event::{Body, Event, JobStarted, ModelCalled, Usage};

/// 订阅那一刻的几样。
#[derive(Debug, Clone, PartialEq)]
pub struct Current {
    /// 还在跑的后台命令和子代理，照编号。
    pub jobs: Vec<JobStarted>,
    /// 会话在哪个目录干活（施工 9-7 上）：工作目录。
    pub cwd: String,
    /// 加进来的目录。
    pub dirs: Vec<String>,
    /// 这个会话累计的。
    pub tally: Tally,
    /// 会话记着的模型引用（模型或 `@池`，SM-1 补）：内核照日志算的，刚换了、还没开回合的也是换成的；没记的（跟着
    /// `models.chat`）没有。`session.configure` 的 `effort` 照它查。
    pub model: Option<String>,
    /// 这一刻有没有能压的（施工 W-12）：`session.compact` 会不会回 `nothing_to_compact`，判法同内核。
    pub compactable: bool,
    /// 会话的上下文上限至少要多少（施工 CTX-1，设计 68）：照这时的策略和端口交的最大输出，压缩线算得出的最小窗口
    /// （`Session::least_context`）。策略里没有压缩的没有。设上限（[`crate::session::Handle::set_context`]）照它拒太小的。
    pub least_context: Option<u64>,
    /// 会话钉的压缩触发线，窗口的百分之几（Miyu 施工 CT-1、CT-1 补）：内核照日志算的，刚钉了、还没开回合的也是钉的；没钉的
    /// （跟着 `compaction.line`）没有。Miyu 订阅会话的回应、`view.status` 的 `compaction_line` 照它写；生效的线是
    /// [`crate::session::Handle::limits`] 的 `compaction_line`（token 数）。
    pub compaction_line: Option<u64>,
}

/// 这个会话累计的：用量、金额照用量汇总的口径，另数压缩、缓存断了几次。
#[derive(Debug, Clone, PartialEq)]
pub struct Tally {
    /// 发出去了的请求。
    pub requests: u64,
    /// 用量，四项各加各的。
    pub usage: Usage,
    /// 只算主请求的用量（施工 9-6 上补）：`purpose` 是空的，压缩的摘要请求也算；回顾、起标题这些辅助请求不算。头照它算命中率、
    /// 上下文，和终端的底栏一个口径。
    pub main: Usage,
    /// 金额、这时装了多少（核心线第二趟：会话花费）：Miyu 的 `amounts`、`unpriced` 两格直接放在这里；LiteMiyu 装在盒子里，
    /// 一次请求都还没有的没有（原来 `Tally` 跟着长期目标进主程序的消息枚举，不装盒就过了 clippy 的大小差；目标 2026-11-14
    /// 砍了，盒子没拆）。读照 [`Tally::amounts`]、[`Tally::unpriced`]、[`Tally::context`]。
    spend: Option<Box<Spend>>,
    /// 压缩的检查点（`context.compacted`）有几个。
    pub compactions: u64,
    /// 意外断了几次缓存（施工 9-6 再补，口径照终端的侧边栏）：只看主请求（`purpose` 空的）；压缩的摘要请求（带 `compaction`，
    /// 以前的日志照「看到的比之前最远的少」认）不算、也不用掉免数；压缩、撤销以后的第一个主请求本来就会断，不算；别的带
    /// `first_difference` 的主请求算一次。
    pub cache_breaks: u64,
    /// 刚压缩、撤销过：下一个主请求本来就会断，不算。
    excused: bool,
    /// 之前的主请求最多看到第几条。
    furthest: Option<u64>,
}

/// 什么都没用。
const NOTHING: Usage = Usage {
    uncached: 0,
    cache_read: 0,
    cache_write: 0,
    output: 0,
    reasoning: None,
};

impl Default for Tally {
    /// 什么都没有。
    fn default() -> Tally {
        Tally {
            requests: 0,
            usage: NOTHING,
            main: NOTHING,
            spend: None,
            compactions: 0,
            cache_breaks: 0,
            excused: false,
            furthest: None,
        }
    }
}

impl Tally {
    /// 照 `events` 从头算（测试里核对一批批加的和它一样；载入时一路加，施工 V-15）。
    #[cfg(test)]
    pub(crate) fn of(events: &[Event]) -> Tally {
        let mut tally = Tally::default();
        tally.add(events);
        tally
    }

    /// 加上 `events` 这一批。
    pub(crate) fn add(&mut self, events: &[Event]) {
        for event in events {
            match &event.body {
                Body::ModelCalled(called) => {
                    if !called.aside() {
                        self.watch(called);
                    }
                    if called.endpoint.is_none() || called.model.is_none() {
                        continue;
                    }
                    self.requests += 1;
                    let spend = self.spend.get_or_insert_default();
                    if let Some(usage) = called.usage {
                        plus(&mut self.usage, usage);
                        if !called.aside() {
                            plus(&mut self.main, usage);
                            spend.context = usage.input() + usage.output;
                        }
                    }
                    match called.cost.as_deref() {
                        Some(cost) => {
                            *spend.amounts.entry(cost.currency.clone()).or_default() +=
                                cost.amount.get();
                        }
                        None if called.usage.is_some() => spend.unpriced += 1,
                        None => {}
                    }
                }
                Body::ContextCompacted(_) => {
                    self.compactions += 1;
                    self.excused = true;
                }
                Body::TurnReverted(_) => self.excused = true,
                _ => {}
            }
        }
    }
}

impl Tally {
    /// 金额，照币种各加各的；一笔都没有的是空的。
    pub fn amounts(&self) -> &BTreeMap<String, f64> {
        static NONE: BTreeMap<String, f64> = BTreeMap::new();
        self.spend.as_ref().map_or(&NONE, |spend| &spend.amounts)
    }

    /// 有用量、没金额的几次（模型没有价格）。
    pub fn unpriced(&self) -> u64 {
        self.spend.as_ref().map_or(0, |spend| spend.unpriced)
    }

    /// 这时上下文里装了多少（底栏的「当前」）：最近一次主请求（压缩的摘要请求也算，同 [`Tally::main`]）的输入加输出，同
    /// miyu-tui `transcript/spent.rs`。还没有报过用量的主请求是 0。
    pub fn context(&self) -> u64 {
        self.spend.as_ref().map_or(0, |spend| spend.context)
    }

    /// 一次主请求：断没断照 [`Tally::cache_breaks`] 的口径数。
    fn watch(&mut self, called: &ModelCalled) {
        let seen = called.seen.get();
        if called.compaction.is_some() || self.furthest.is_some_and(|furthest| seen < furthest) {
            return;
        }
        if called.first_difference.is_some() && !self.excused {
            self.cache_breaks += 1;
        }
        self.excused = false;
        self.furthest = Some(seen);
    }
}

/// 派出去的子代理（连孙代理）一共用掉的，记进主会话的概要（`window/summary.rs` 的 `agents`）：重开主会话时照它接着算，不读
/// 子会话的日志（Miyu 照核心的用量索引连子会话一起查，`miyu-endpoint/src/usage.rs` 的 `branch`；LiteMiyu 没有那份索引）。
/// 只记底栏、`/status` 要的那几样：请求数、四项（全部的、主请求的）、金额、没金额的次数；上下文、缓存断了几次不记。
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub(crate) struct Carried {
    pub(crate) requests: u64,
    pub(crate) usage: Usage,
    pub(crate) main: Usage,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) amounts: BTreeMap<String, f64>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub(crate) unpriced: u64,
}

/// 是 0（概要里不写这一格）。
fn is_zero(n: &u64) -> bool {
    *n == 0
}

impl Tally {
    /// 记进概要的那几样；一次请求都没有的没有。
    pub(crate) fn carried(&self) -> Option<Carried> {
        (self.requests > 0).then(|| Carried {
            requests: self.requests,
            usage: self.usage,
            main: self.main,
            amounts: self.amounts().clone(),
            unpriced: self.unpriced(),
        })
    }

    /// 照概要里记的接着算（[`Tally::carried`] 的反面）。
    pub(crate) fn resumed(carried: &Carried) -> Tally {
        Tally {
            requests: carried.requests,
            usage: carried.usage,
            main: carried.main,
            spend: Some(Box::new(Spend {
                amounts: carried.amounts.clone(),
                unpriced: carried.unpriced,
                context: 0,
            })),
            ..Tally::default()
        }
    }
}

/// 金额和这时装了多少（[`Tally`] 的盒子里那一份）。
#[derive(Debug, Clone, Default, PartialEq)]
struct Spend {
    amounts: BTreeMap<String, f64>,
    unpriced: u64,
    context: u64,
}

/// 四项各加各的。
fn plus(total: &mut Usage, usage: Usage) {
    let reasoning = total.reasoning;
    *total = Usage {
        reasoning,
        ..total.plus(&usage)
    };
}

#[cfg(test)]
mod tests;
