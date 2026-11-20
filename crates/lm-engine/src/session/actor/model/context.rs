//! 会话的上下文上限换了（Miyu 施工 CTX-1，设计 68 第四节「下一次请求起生效」）：Miyu 写在 `actor/store.rs`，LiteMiyu 那一页
//! 放不下，挪到这里。

use lm_kernel::kernel::event::{Body, Event};

use super::super::Actor;

impl Actor {
    /// 这一批里换了会话的上下文上限的：内核记下就已经是新的，当场交给端口，再把端口套过的限额交给内核：压缩线、给头看的
    /// 限额马上换，不等下一轮。在写盘、回应之前做：回了的命令读得到新的限额。
    pub(in crate::session::actor) fn follow_context(&mut self, events: &[Event]) {
        let changed = events.iter().any(
            |event| matches!(&event.body, Body::PolicyChanged(changed) if changed.context.is_some()),
        );
        if changed {
            self.model.cap(self.session.context());
            self.hand(self.model.limits());
        }
        // 换了策略快照的（Miyu 施工 CT-1：压缩触发线在快照里），给头看的限额照新的重写（`line.rs`）。
        let swapped = events.iter().any(
            |event| matches!(&event.body, Body::PolicyChanged(changed) if changed.policy.is_some()),
        );
        if swapped {
            self.follow_policy();
        }
    }
}
