//! 从日志边读边交（第十二趟打开旧会话，第十三趟切进子会话也照这样）：日志一块块读、当场补发，一块读成的推送装成一条交给
//! 界面；界面办完一块回一声，在路上的最多两块（[`IN_FLIGHT`]），界面办得慢时读日志的那一头等着，推送不在通道里越攒越多。

use std::collections::VecDeque;
use std::sync::mpsc;

use super::super::{Deliver, Push, Update, push};
use super::children;

/// 最多几块在路上。
const IN_FLIGHT: usize = 2;

/// 交给谁。
pub(super) enum To {
    /// 打开的旧会话（[`Update::Replayed`]）：派出去的子代理交回引擎线程订阅（同原来先订阅、再推）。
    Opened(tokio::sync::mpsc::UnboundedSender<String>),
    /// 切进的子会话（[`Update::Viewed`]）：它的整个编号。
    Viewed(String),
}

/// 边读边交的那一份：补发到哪了、怎么交给界面、在路上的几块。
pub(super) struct Handing {
    /// 补发到哪了。
    pub replay: push::replay::Replay,
    deliver: Deliver,
    to: To,
    /// 交过去、界面还没回声的几块：每块一条只用一次的通道，照先后。
    in_flight: VecDeque<mpsc::Receiver<()>>,
}

impl Handing {
    pub(super) fn new(deliver: Deliver, to: To) -> Handing {
        Handing {
            replay: push::replay::Replay::default(),
            deliver,
            to,
            in_flight: VecDeque::new(),
        }
    }

    /// 交一块推送：在路上的满了先等最早那一块办完。界面没办就扔了那一块的（退出了、换了会话），回声的那一头跟着没了，
    /// 不再等。
    pub(super) fn hand(&mut self, batch: Vec<Push>) {
        if batch.is_empty() {
            return;
        }
        if let To::Opened(agents) = &self.to {
            for agent in children::agents(&batch) {
                // 引擎线程自己拿着收的一头：发得出去。
                if agents.send(agent).is_err() {}
            }
        }
        if self.in_flight.len() >= IN_FLIGHT
            && let Some(oldest) = self.in_flight.pop_front()
        {
            // 等它办完；界面扔了那一块的，回声那一头没了，当场交回错，不再等。
            if oldest.recv().is_err() {}
        }
        let (done, back) = mpsc::channel();
        let update = match &self.to {
            To::Opened(_) => Update::Replayed {
                pushes: batch,
                done,
            },
            To::Viewed(session) => Update::Viewed {
                session: session.clone(),
                pushes: batch,
                first: false,
                done,
            },
        };
        if (self.deliver)(update) {
            self.in_flight.push_back(back);
        }
    }
}
