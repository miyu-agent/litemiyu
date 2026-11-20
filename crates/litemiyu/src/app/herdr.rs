//! 报给 herdr 的是什么（`crate::herdr`，照 miyu-tui `app/notify.rs` 的 `herdr_state`、`app/viewing.rs`）：主循环每画一帧以前
//! 问一次，变了才真的报。
//!
//! 一个窗口里有主会话、切进去看的子会话、派出去的子代理，照「人现在需不需要过来看」判：
//!
//! - 确认、提问的抽屉开着（子代理问的也在这里）：等你回复，比工作中要紧。
//! - 主会话的一轮在跑、正在看的子会话的一轮在跑、派出去的子代理还有没报回来的：工作中。后台命令不算（一个常驻的服务会让
//!   窗格一直显示工作中）。
//! - 都没有：空闲。
//!
//! 恢复命令照主会话的编号（切进子会话看的时候也是主会话的）；应急模式不落盘，没有能恢复的会话。

use super::App;
use crate::herdr::State;

impl App {
    /// 这一刻该报的状态和主会话；正在打开旧会话（屏幕上还是原来那个，或者在收补发）的是 `None`，这一下不报，开好了再报，
    /// 换会话时不会先报成没有会话。
    pub fn herdr_now(&self) -> Option<(State, Option<&str>)> {
        if self.talk.opening != super::Opening::None {
            return None;
        }
        let talk = &self.talk;
        // 主会话那一份正文：就地看，不调 `Talk::main_session`（它每次都拷一份编号，而且排在「说过话」才用到的那一片代码里，
        // 空闲的窗口每一帧调它要多装进一片邻居页）。
        let main = match &talk.child {
            Some(child) => child.main(),
            None => &talk.transcript,
        };
        let running = main.running.is_some()
            || talk.transcript.running.is_some()
            || talk.jobs.agents().any(|job| job.ended.is_none());
        let session = match self.rescue {
            Some(_) => None,
            None => main.session.as_deref(),
        };
        Some((state(talk.drawers.open(), running), session))
    }
}

/// 有抽屉开着就是等你回复，比工作中要紧；不然有在跑的是工作中，都没有是空闲。
fn state(drawer_open: bool, running: bool) -> State {
    if drawer_open {
        State::Blocked
    } else if running {
        State::Working
    } else {
        State::Idle
    }
}

#[cfg(test)]
mod tests {
    use super::state;
    use crate::herdr::State;

    #[test]
    fn an_open_drawer_outranks_running_work() {
        assert_eq!(state(true, true), State::Blocked);
        assert_eq!(state(true, false), State::Blocked);
        assert_eq!(state(false, true), State::Working);
        assert_eq!(state(false, false), State::Idle);
    }
}
