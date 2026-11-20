//! 会话的账（底栏的累计、花费，`/status` 的用量、花费）：主会话的照引擎 `Handle::meter` 交来的（`engine/forward.rs`：开会话、
//! 每次请求说完、一轮结束时各问一次）；派出去的子代理（连孙代理）的照它们推来的用量、金额一笔笔加——子会话说完就放下了，
//! 引擎那一头问不着它的 `Handle::meter`。合起来照 Miyu：底栏连同子代理一起算（Miyu 终端 `tree_usage`、`tree_bill`，
//! 2026-09-30 项目主人：子代理用的也算进主会话），子代理的那一份用 `Meter::merge` 加上。
//!
//! 重开的会话：开它以前子代理用掉的照引擎交来的一份（`Handle::earlier_agents`，主会话的概要里记的）加上，不读子会话的日志
//! （第二十六趟：原来重开以后只剩主会话的，底栏、侧边栏、`/status` 都变少）。
//!
//! 累计的 token 数主会话那一份照旧照推来的用量当场加（`transcript.total`，和引擎的账同一份事件、同一个口径），不等这一头
//! 问回来；金额只有引擎的账里有。

use lm_engine::session::Meter;
use lm_kernel::kernel::event::Usage as Spent;

use super::App;
use crate::core::{Push, Update, Usage};

impl App {
    /// 会话的账：交回 `true` 的不再往下走。换了会话以后才到的旧账不要。
    pub(super) fn meter_update(&mut self, update: &Update) -> bool {
        let (session, meter, earlier) = match update {
            Update::Meter { session, meter } => (session, meter, false),
            Update::EarlierAgents { session, meter } => (session, meter, true),
            _ => return false,
        };
        if self.talk.main_session().as_deref() == Some(session.as_str()) {
            match earlier {
                true => self.talk.earlier_agents = Some(meter.clone()),
                false => self.talk.meter = Some(meter.clone()),
            }
        }
        true
    }

    /// 子会话 `session` 推来的：用量、金额记进它的账。
    pub(super) fn agent_meter(&mut self, session: &str, push: &Push) {
        let (usage, main) = match push {
            Push::Usage(usage) => (usage, true),
            Push::AuxUsage(usage) => (usage, false),
            Push::Billed(billed) => {
                let meter = self.agent_entry(session);
                match billed {
                    Some((currency, amount)) => {
                        *meter.amounts.entry(currency.clone()).or_default() += amount.get();
                    }
                    None => meter.unpriced += 1,
                }
                return;
            }
            _ => return,
        };
        let spent = Spent {
            uncached: usage.uncached,
            cache_read: usage.cache_read,
            cache_write: usage.cache_write,
            output: usage.output,
            reasoning: None,
        };
        let meter = self.agent_entry(session);
        meter.total = meter.total.plus(&spent);
        if main {
            meter.main = meter.main.plus(&spent);
            meter.context = spent.input() + spent.output;
        }
    }

    fn agent_entry(&mut self, session: &str) -> &mut Meter {
        self.talk
            .agent_meters
            .entry(session.to_string())
            .or_insert_with(empty)
    }
}

/// 一份空账。
pub(super) fn empty() -> Meter {
    const NONE: Spent = Spent {
        uncached: 0,
        cache_read: 0,
        cache_write: 0,
        output: 0,
        reasoning: None,
    };
    Meter {
        context: 0,
        window: None,
        total: NONE,
        main: NONE,
        amounts: std::collections::BTreeMap::new(),
        unpriced: 0,
    }
}

/// 子代理的账加进界面的累计：主请求的四项照它的 `main`，别的（辅助请求）算进 `aux`（同 `transcript.total` 的口径）。
pub(super) fn add_usage(total: &mut Usage, meter: &Meter) {
    let main = &meter.main;
    total.uncached += main.uncached;
    total.cache_read += main.cache_read;
    total.cache_write += main.cache_write;
    total.output += main.output;
    let all = meter.total.input() + meter.total.output;
    total.aux += all.saturating_sub(main.input() + main.output);
}

#[cfg(test)]
mod tests;
