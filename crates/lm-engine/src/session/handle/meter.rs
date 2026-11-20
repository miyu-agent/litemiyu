//! 底栏那一行要的几样一次拿齐（核心线第二趟：会话花费，主人 2026-11-14 定的式样 `5.6k/1M(1%) · Σ120M(C99%) · $0.42`）：这时装了多少、
//! 窗口多大、会话累计、缓存命中率、花了多少。都是现成的：累计照 actor 记着的 [`Tally`]（载入时照日志算一次，之后每落一批盘
//! 加上），窗口照 [`Handle::limits`]；问的时候不读日志、不取价。
//!
//! 只算这一个会话。Miyu 的底栏连同子代理的（miyu-tui `app/sessions/notes.rs` 的 `tree_usage`、`tree_bill`，2026-09-30 项目主人：
//! 子代理用的也算进主会话）：界面把子会话的这一份加上（[`Meter::merge`]），或者照子会话推来的 `model.called` 一笔笔加。

use std::collections::BTreeMap;

use super::{Handle, Stopped};
use crate::session::current::Tally;
use lm_kernel::kernel::event::Usage;

/// 底栏那一行的几样。
#[derive(Debug, Clone, PartialEq)]
pub struct Meter {
    /// 这时上下文里装了多少（[`Tally::context`]）。
    pub context: u64,
    /// 窗口，套过会话的上限（[`crate::session::Handle::limits`]）。不知道的没有。
    pub window: Option<u64>,
    /// 会话累计用了多少：四项各加各的（底栏的 `Σ` 照 `uncached + cache_read + cache_write + output`）。
    pub total: Usage,
    /// 主请求的用量（[`Tally::main`]）：命中率照它算（[`Meter::hit_rate`]）。
    pub main: Usage,
    /// 花了多少，照币种各加各的（[`Meter::spent`] 排好先后）。
    pub amounts: BTreeMap<String, f64>,
    /// 有用量、没金额的几次（模型没有价格）。
    pub unpriced: u64,
}

impl Meter {
    /// 照累计和窗口拼。
    pub fn of(tally: &Tally, window: Option<u64>) -> Meter {
        Meter {
            context: tally.context(),
            window,
            total: tally.usage,
            main: tally.main,
            amounts: tally.amounts().clone(),
            unpriced: tally.unpriced(),
        }
    }

    /// 把子会话的那一份加进来：累计、金额各加各的；这时装了多少、窗口照主会话的。
    pub fn merge(&mut self, child: &Meter) {
        self.total = Usage {
            reasoning: self.total.reasoning,
            ..self.total.plus(&child.total)
        };
        self.main = Usage {
            reasoning: self.main.reasoning,
            ..self.main.plus(&child.main)
        };
        for (currency, amount) in &child.amounts {
            *self.amounts.entry(currency.clone()).or_default() += amount;
        }
        self.unpriced += child.unpriced;
    }

    /// 缓存命中率，百分之几（底栏的 `C99%`）：只算主请求，读缓存的占输入（没命中、读缓存、写缓存三项）的多少，同
    /// miyu-tui 的底栏。主请求还没报过输入的没有。
    pub fn hit_rate(&self) -> Option<f64> {
        let input = self.main.input();
        (input > 0).then(|| self.main.cache_read as f64 * 100.0 / input as f64)
    }

    /// 花了多少，一种币一项：`USD` 排最前，别的照币种代码的字母先后（Miyu `price::ordered`；Miyu 照配置项 `usage.currency`
    /// 排，LiteMiyu 没有这一项）。不换算。一笔金额都没有的是空的：价格都不知道的模型是「没有数」，不是 0。
    pub fn spent(&self) -> Vec<(String, f64)> {
        let mut amounts: Vec<(String, f64)> = self
            .amounts
            .iter()
            .map(|(currency, amount)| (currency.clone(), *amount))
            .collect();
        lm_kernel::models::price::ordered(&mut amounts, lm_kernel::models::catalog::USD);
        amounts
    }
}

impl Handle {
    /// 底栏那一行要的几样（[`Meter`]），只算这个会话。界面在开会话、每收到一条 `model.called`（或一轮结束）时问一次；不读日志。
    ///
    /// # Errors
    ///
    /// 会话停了。
    pub async fn meter(&self) -> Result<Meter, Stopped> {
        let current = self.current().await?;
        Ok(Meter::of(&current.tally, self.limits().window))
    }

    /// 这个窗口开主会话以前，它派过的子代理（连孙代理）一共用掉的：主会话的概要里记的（`window/summary.rs` 的 `agents`），
    /// 不读子会话的日志。底栏、`/status` 连同子代理一起算时加上它（Miyu 终端 `tree_usage`，核心照用量索引连子会话一起查）；
    /// 这个窗口里派的照子会话推来的另加。上下文、窗口不算（都是 0、没有）。没派过、概要里没记的、子会话的把手没有。
    pub fn earlier_agents(&self) -> Option<Meter> {
        let tally = self.table()?.earlier_agents()?;
        Some(Meter::of(&tally, None))
    }
}

#[cfg(test)]
mod tests;
