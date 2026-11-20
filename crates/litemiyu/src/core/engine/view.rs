//! 切进子会话（Miyu 蓝图 `tui.md`「切进子会话」）在引擎线程这一头：人切进主会话派的一个子会话时，照它的把手从头订阅
//! （`subscribe_after(0)`：读日志里落了盘的和之后推来的接得上，不漏不重），日志里的照打开旧会话那样边读边交
//! （[`Update::Viewed`]，`handing.rs`；界面照主会话那一套只留几屏），之后推来的照它的编号一条条交（[`Update::ViewPush`]，
//! 在跑的接着出字）。日志的只读入口留着，界面翻上去要读回放掉的段时照它读（`readback.rs`）。退回主会话就掐掉，这一份
//! 订阅、界面那一份正文都不留：界面里不另存子会话的整份历史，要看时照日志读回来（内核线第三趟）。
//!
//! 一开始就订阅着的那一份（`children.rs`，确认、提问归抽屉）照旧，和这一份互不相干。

use std::sync::mpsc;

use lm_engine::session::{Backlog, Ended, Handle, Pushed};
use lm_kernel::kernel::id::SessionId;

use super::super::{Deliver, Push, Update, push};
use super::children::Followed;
use super::handing::{Handing, To};

/// 正在看的子会话：把手（输入框发的话、打断交给它），把它推来的交给界面的那个任务。
pub struct Viewing {
    /// 它的把手。
    pub handle: Handle,
    /// 它日志的只读入口：读回放掉的段照它取那一截（`readback.rs`）。
    pub reader: Backlog,
    /// 转推送的任务：退回主会话时掐掉。
    task: tokio::task::AbortHandle,
}

impl Drop for Viewing {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// 切进主会话 `main` 派的子会话 `id`：取不到它的（不是这张表里的、编号写错）是 `None`，界面照旧看着主会话。
pub async fn enter(main: &Handle, id: &str, deliver: Deliver) -> Option<Viewing> {
    let session = SessionId::parse(id).ok()?;
    let handle = main.child(&session).await?;
    let (subscription, backlog) = handle.subscribe_after(0).await.ok()?;
    let followed = Followed {
        subscription,
        rejoin: handle.rejoin(),
        seen: backlog.upto(),
    };
    // 先交一条空的：界面照它开始攒子会话的正文，后面一块块的接着办，读完（钟回到现在）才换上屏幕（`app/child.rs`）。
    let (done, _) = mpsc::channel();
    if !deliver(Update::Viewed {
        session: id.to_string(),
        pushes: Vec::new(),
        first: true,
        done,
    }) {
        return None;
    }
    let reader = backlog.slice(0, 0);
    let state = Handing::new(std::sync::Arc::clone(&deliver), To::Viewed(id.to_string()));
    let read = backlog
        .fold(state, |state, events| {
            let mut batch = Vec::new();
            for event in events {
                batch.extend(state.replay.read(event));
            }
            state.hand(batch);
        })
        .await;
    match read {
        Ok(mut state) => {
            let rest = state.replay.finish();
            state.hand(rest);
        }
        // 日志读不回来：照旧接着看推来的，正文里写一行为什么（同打开旧会话读不回来），钟拨回现在。
        Err(message) => {
            if !deliver(Update::Failed(message)) {
                return None;
            }
            let mut state =
                Handing::new(std::sync::Arc::clone(&deliver), To::Viewed(id.to_string()));
            state.hand(vec![Push::Clock(None)]);
        }
    }
    // 子会话的限额（窗口、压缩线）：同主会话订阅上时交一次（`forward.rs`），开会话时定好的不会另推 `model.changed`。
    let limits = Push::Limits(crate::core::Limits::of(&handle.limits()));
    Handing::new(std::sync::Arc::clone(&deliver), To::Viewed(id.to_string())).hand(vec![limits]);
    let task = tokio::spawn(forward(followed, id.to_string(), deliver)).abort_handle();
    Some(Viewing {
        handle,
        reader,
        task,
    })
}

/// 子会话推来的一条条照它的编号交给界面，直到它停了、界面不收了、人退回主会话（任务被掐掉）。掉了队的照落了盘的补上，补不上的
/// 告诉界面一声（[`Update::ChildBehind`]，审计 #063）。
async fn forward(mut followed: Followed, session: String, deliver: Deliver) {
    loop {
        let pushed = match followed.next().await {
            Ok(pushed) => pushed,
            Err(Ended::Stopped) => return,
            Err(Ended::Lagged) => {
                deliver(Update::ChildBehind);
                return;
            }
        };
        let pushes: Vec<Push> = match &*pushed {
            Pushed::Events(events) => events.iter().flat_map(push::read).collect(),
            Pushed::Transient(event) => push::read_transient(event),
        };
        for push in pushes {
            if !deliver(Update::ViewPush {
                session: session.clone(),
                push,
            }) {
                return;
            }
        }
    }
}
