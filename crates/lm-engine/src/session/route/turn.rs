//! 回合开始时重新解析（施工 8-10，`docs/blueprint/models.md`「怎么走」第六条第 3、4 条）：照这一轮冻结的配置解析会话的
//! 引用，端点、限额换成解析出的。
//!
//! - 内核交了引用的，换成它；没交的（人回到了跟着配置，以前的日志、造的时候连 `models.chat` 都
//!   没配的）照这一轮的 `models.chat`（施工 8-10 再补；以前照路由记在内存里的）。
//! - 解析不出的（供应商删了；LM-8 以前的日志里记的 `@池`，砍了池以后认不出），退回这一轮的 `models.chat`，以后钉在它上面；
//!   内核交了引用的，交回原来的和退回的，内核记一条 `session.policy_changed`（头写一行「原模型不可用，已切换为默认」）。
//!   `models.chat` 也解析不出的不退，端口是「没有模型」，这一轮的请求当场 `no_model`。
//! - 限额照解析出的重算：配置里改了窗口、用出来的窗口，这一轮就用上。
//! - 这一轮的配置记下（施工 8-18），会话记的思考强度换成内核交的（施工 SM-1）：每次请求、给头看的那一档、限额里那一块
//!   事实写的档位照它们算（`route/effort.rs`）。

use std::sync::Arc;

use lm_kernel::kernel::session::Replaced;
use lm_kernel::models::provider;
use lm_kernel::models::reference::resolve;

use super::{Route, nothing};
use crate::session::config::TurnConfig;

impl Route {
    /// 回合开始：照这一轮的配置 `config` 重新解析，`recorded` 是内核记着的引用，`effort` 是内核记着的思考强度（见模块的
    /// 说明）。交回退回了默认的那一次。
    pub(super) fn begin(
        &self,
        config: &TurnConfig,
        recorded: Option<&str>,
        effort: Option<&str>,
    ) -> Option<Replaced> {
        let values = config.resolved.values();
        let data = &self.shared.data;
        let resolve = |text: &str| {
            data.with(|knowledge| resolve(&values, knowledge, text))
                .ok()
        };
        let mut pinned = self.lock();
        pinned.config = Arc::clone(config);
        // 内核没记引用的跟着这一轮的 `models.chat`（施工 8-10 再补：人能回到「跟着配置」）：配置换了，这一轮就换。
        let wanted = match recorded {
            Some(recorded) => Some(recorded.to_string()),
            None => provider::chat(&values),
        };
        if pinned.reference != wanted {
            pinned.reference = wanted;
        }
        let mut replaced = None;
        let resolved = match pinned.reference.as_deref().and_then(resolve) {
            Some(resolved) => Some(resolved),
            None => {
                let chat = provider::chat(&values);
                let fallback = chat.as_deref().and_then(resolve);
                if let (Some(_), Some(to)) = (&fallback, chat) {
                    replaced = recorded.map(|from| Replaced {
                        from: from.to_string(),
                        to: to.clone(),
                    });
                    pinned.reference = Some(to);
                }
                fallback
            }
        };
        pinned.effort = effort.map(str::to_string);
        let mut limits = match resolved {
            Some(target) => self.shared.limits(config, &target),
            None => nothing(),
        };
        self.shared.stamp(config, &mut limits, effort);
        pinned.last = limits.model.clone();
        pinned.limits = limits;
        replaced
    }
}
