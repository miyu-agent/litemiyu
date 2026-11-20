//! 把会话推来的交回界面（[`forward`]）、安静下来以后还堆（[`settle`]）：从 `engine.rs` 挪出来，那一页放不下打开旧会话了。

use lm_engine::session::{Ended, Handle, Pushed, Subscription};
use tokio::sync::mpsc;

use super::super::{Deliver, Limits, Push, Update, push};
use super::{QUIET, children};

/// 把会话推来的一直交回界面，直到会话停了、界面不收了。`stirred`：一开始就当刚来过推送，安静 [`QUIET`] 以后还一次堆（打开
/// 旧会话补发完）。
pub(super) async fn forward(
    mut subscription: Subscription,
    session: Handle,
    (deliver, agents): (Deliver, mpsc::UnboundedSender<String>),
    mut stirred: bool,
) {
    // 循环照把手读（`Handle::looping`）：一批推完看一次，变了才交。
    let mut looping = session.looping();
    // 限额（窗口、压缩线）：订阅上时交一次（同 Miyu 订阅的回应），一批推完看一次，变了才交（`Handle::limits`）。
    let mut limits = session.limits();
    if !deliver(Update::Push(Push::Limits(Limits::of(&limits)))) {
        return;
    }
    // 会话的账（底栏的累计、花费）：开会话时交一次，以后每批推送里有请求说完、一轮结束的再交（`Handle::meter`）。
    if !meter(&session, &deliver).await {
        return;
    }
    // 重开的会话：开它以前子代理用掉的（主会话的概要里记的）交一次，之后这个窗口里派的照子会话推来的另加（`app/meter.rs`）。
    if let Some(earlier) = session.earlier_agents()
        && !deliver(Update::EarlierAgents {
            session: session.id().to_string(),
            meter: Box::new(earlier),
        })
    {
        return;
    }
    // 上一次还过以后又来过推送：安静 [`QUIET`] 以后再还一次。没来过的一直睡着等，不定时醒（设计第四节第 1 条）。
    loop {
        let next = if stirred {
            match tokio::time::timeout(QUIET, subscription.next()).await {
                Ok(next) => next,
                Err(_) => {
                    stirred = false;
                    settle();
                    continue;
                }
            }
        } else {
            subscription.next().await
        };
        let pushed = match next {
            Ok(pushed) => pushed,
            Err(Ended::Stopped) => return,
            Err(Ended::Lagged) => {
                deliver(Update::Failed("fell behind the session".to_string()));
                return;
            }
        };
        stirred = true;
        let pushes: Vec<Push> = match &*pushed {
            Pushed::Events(events) => events.iter().flat_map(push::read).collect(),
            Pushed::Transient(event) => push::read_transient(event),
        };
        // 派出去的子代理交回引擎线程订阅；引擎线程不在了的是界面在退出。
        for agent in children::agents(&pushes) {
            if agents.send(agent).is_err() {
                return;
            }
        }
        let billed = pushes.iter().any(|push| {
            matches!(
                push,
                Push::Usage(_) | Push::AuxUsage(_) | Push::TurnEnded(_)
            )
        });
        for one in pushes {
            if !deliver(Update::Push(one)) {
                return;
            }
        }
        if billed && !meter(&session, &deliver).await {
            return;
        }
        let now = session.limits();
        if now != limits {
            limits = now;
            if !deliver(Update::Push(Push::Limits(Limits::of(&limits)))) {
                return;
            }
        }
        let now = session.looping();
        if now != looping {
            looping = now;
            if !deliver(Update::LoopNow(looping.clone())) {
                return;
            }
        }
    }
}

/// 问一次会话的账交给界面；会话停了的不交。交回界面还收不收。
async fn meter(session: &Handle, deliver: &Deliver) -> bool {
    match session.meter().await {
        Ok(meter) => deliver(Update::Meter {
            session: session.id().to_string(),
            meter: Box::new(meter),
        }),
        Err(_) => true,
    }
}

/// 一阵推送过去、安静下来以后把堆里空着的还给系统（照 Miyu 核心 `settle.rs`）：一轮里拼请求、读回答、写日志放掉的，
/// 留在 glibc 手里不还。引擎这时闲着，就在这条线程上还：堆只有几兆，走一遍不值得为它起一条阻塞线程。
fn settle() {
    lm_heap::trim();
}
