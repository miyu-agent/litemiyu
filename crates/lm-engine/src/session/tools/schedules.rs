//! 循环（L5 四）：执行器这一头的循环端口。`schedule_wakeup` 交来要做的 → 经回传的通道送回 actor（[`Back::Schedule`]）→ actor
//! 照内存里的循环当场答（`actor/looping.rs`）。Miyu 的端口在核心，直接读写唤醒表；LiteMiyu 的循环在 actor 里，端口只传话。
//! 一个会话一个，交给每一次调用（同 Miyu：端口总是交，没开着循环的由 actor 答「没有」）。

use std::sync::Arc;

use tokio::sync::{mpsc, oneshot};

use crate::session::port::Back;
use crate::tool::{ScheduleAnswering, ScheduleAsk, SchedulePort, ScheduleRefused};

/// 交来的送回 actor。
struct Scheduler {
    backs: mpsc::UnboundedSender<Back>,
}

impl SchedulePort for Scheduler {
    fn schedule(&self, ask: ScheduleAsk) -> ScheduleAnswering<'_> {
        let (reply, answered) = oneshot::channel();
        // actor 停了就没人收：等的这一头拿到「会话停了」。
        let sent = self.backs.send(Back::Schedule { ask, reply }).is_ok();
        Box::pin(async move {
            match sent {
                true => answered.await.unwrap_or(Err(ScheduleRefused::Unavailable)),
                false => Err(ScheduleRefused::Unavailable),
            }
        })
    }
}

/// 这个会话的循环端口。
pub(super) fn port(backs: &mpsc::UnboundedSender<Back>) -> Arc<dyn SchedulePort> {
    Arc::new(Scheduler {
        backs: backs.clone(),
    })
}
