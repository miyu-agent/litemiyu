//! actor 的一生（施工 V-2 再补从 `actor.rs` 挪来，那个文件到了行数上限）：起任务、看着它；闲够了退下（`docs/designs/07-存储.md`
//! 第七节「会话按需载入」第 3 条，`docs/blueprint/session/actor.md`「退下」）。
//!
//! 退不退由 actor 自己照账答：最后一次有动静以后过了多久，内核空不空（没有回合、没在重启、没有在等的子代理），还有没有头看着、
//! 等着回应的命令、在路上的请求、派出去还没回报或者结束了还没落盘的任务、记忆的闹钟、它在等的和等它空下来的会话，收件箱、
//! 回报的路里还有没有没办的。都没有、闲够了的答「退下了」就退出；回答交不出去的（会话表不等了）不退。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use lm_kernel::kernel::session::Action;

use super::Actor;
use crate::session::shown::Shown;

/// 起一个 actor 的任务，外面再套一个看着它的：它停了（不论怎么停，panic 了也一样：只在测试的配置下走得到，发行配置
/// panic = abort（`docs/00-设计.md` 第九节）发出去的程序崩了整个进程退出）收尾。
pub(crate) fn spawn(actor: Actor, first: Vec<Action>) {
    let busy = actor.busy();
    let task = tokio::spawn(actor.run(first));
    tokio::spawn(async move {
        let _ended = task.await;
        // 停了的会话不算在跑：核心不为它不肯空闲退出。
        busy.store(false, Ordering::Release);
        // 会话放下的还给系统（23 F3，施工 V-2 再补）：大会话关掉以后 glibc 不还，核心的内存一直不回落。
        drop(tokio::task::spawn_blocking(lm_heap::trim));
    });
}

impl Actor {
    /// 有没有在跑的回合：交给 `Handle` 的那一份（施工 V-51 从 `actor.rs` 挪来，那个文件到了行数上限）。
    pub(crate) fn busy(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.busy)
    }

    /// 给头看的限额和模型：交给 `Handle` 的那一份（施工 8-9、8-10）。
    pub(crate) fn shown(&self) -> Arc<Mutex<Shown>> {
        Arc::clone(&self.shown)
    }

    /// 这时有没有至少一个头订阅着：交给 `Handle` 的那一份（施工 C-5）。
    pub(crate) fn watched(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.watched)
    }
}
