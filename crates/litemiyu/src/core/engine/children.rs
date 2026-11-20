//! 子代理和后台命令在引擎线程这一头（Miyu 蓝图 `tui.md`「后台命令、子代理和侧边栏」「切进子会话」）：主会话派出去的子会话
//! 一开始就订阅着（它要人确认的高危命令、它在做什么都从它自己的会话里来），推送照子会话的编号交给界面；读后台命令的
//! 输出、人停一个任务照主会话的把手办（`Handle::job_output`、`Handle::stop_job`）。
//!
//! 子会话的把手照主会话的把手按编号取（`Handle::child`），订阅好就放下：订阅不拖住它的 actor（主会话放下了，会话表和子会话
//! 跟着没，订阅跟着断）。子会话再派的（孙代理）照样订阅：它们的推送里有 `job.started`，交回这里再取。

use std::io::{Read, Seek, SeekFrom};
use std::sync::Arc;

use lm_engine::session::{Ended, Handle, Pushed, Rejoin, Subscription, Unreadable};
use lm_kernel::kernel::event::Event;
use lm_kernel::kernel::id::{JobId, SessionId};
use lm_kernel::kernel::origin::{By, Person};
use tokio::sync::mpsc;

use super::super::{Deliver, Push, Update, push};

/// 后台命令的输出最多读末尾这么多字节：面板里只看最后几屏。
const TAIL: u64 = 64 * 1024;

/// 推送里派出去的子代理：要订阅的子会话。
pub fn agents(pushes: &[Push]) -> impl Iterator<Item = String> + '_ {
    pushes.iter().filter_map(|push| match push {
        Push::JobStarted(started) if started.agent => started.session.clone(),
        _ => None,
    })
}

/// 订阅子会话 `id`：主会话的把手取得到它才订阅，取不到的（已经停了、不是这张表里的）不管。
pub async fn watch(main: &Handle, id: &str, deliver: Deliver, more: mpsc::UnboundedSender<String>) {
    let Ok(session) = SessionId::parse(id) else {
        return;
    };
    let Some(child) = main.child(&session).await else {
        return;
    };
    // 从头补：派出去到订阅上之间它可能已经在问人了（只推订阅以后的那一份里就没有那一句）。日志里落了盘的照先后交一遍，
    // 问过又答了的抽屉照推送收（`app/asking.rs`）。
    let Ok((subscription, backlog)) = child.subscribe_after(0).await else {
        return;
    };
    let rejoin = child.rejoin();
    drop(child);
    let seen = backlog.upto();
    if let Ok(events) = backlog.read().await {
        let pushes: Vec<Push> = events.iter().flat_map(push::read).collect();
        if !hand_over(&pushes, id, &deliver, &more) {
            return;
        }
    }
    let followed = Followed {
        subscription,
        rejoin,
        seen,
    };
    tokio::spawn(forward(followed, id.to_string(), deliver, more));
}

/// 跟着的一个会话：订阅，掉了队照它重新订阅（不拖住会话），落了盘的看到了第几条。
pub struct Followed {
    /// 订阅。
    pub subscription: Subscription,
    /// 掉了队照它重新订阅。
    pub rejoin: Rejoin,
    /// 落了盘的最后看到的序号：重新订阅从它后面补。
    pub seen: u64,
}

impl Followed {
    /// 下一份推送；掉了队的照落了盘的补（审计 #063）：重新订阅、交回补上的那一截事件（当一份推送交），之后照新的订阅接着推。
    /// 交回 `Err(Ended::Lagged)` 的是补不上（掉队以后会话停了、日志读不回来），`Err(Ended::Stopped)` 的是会话停了。
    pub async fn next(&mut self) -> Result<Arc<Pushed>, Ended> {
        let pushed = match self.subscription.next().await {
            Err(Ended::Lagged) => self.rejoined().await?,
            next => next?,
        };
        if let Pushed::Events(events) = &*pushed
            && let Some(last) = events.last()
        {
            self.seen = self.seen.max(last.seq.get());
        }
        Ok(pushed)
    }

    /// 掉了队：照最后看到的序号重新订阅，交回补上的那一截（一条都没有的也是一份空的）。
    async fn rejoined(&mut self) -> Result<Arc<Pushed>, Ended> {
        let (subscription, backlog) = self
            .rejoin
            .subscribe_after(self.seen)
            .await
            .map_err(|_| Ended::Lagged)?;
        let events: Vec<Event> = backlog.read().await.map_err(|_| Ended::Lagged)?;
        self.subscription = subscription;
        Ok(Arc::new(Pushed::Events(events)))
    }
}

/// 一批推送照子会话的编号交给界面，派出去的子代理交回引擎线程订阅；界面、引擎线程不在了的交回假的。
fn hand_over(
    pushes: &[Push],
    session: &str,
    deliver: &Deliver,
    more: &mpsc::UnboundedSender<String>,
) -> bool {
    for id in agents(pushes) {
        if more.send(id).is_err() {
            return false;
        }
    }
    pushes.iter().all(|push| {
        deliver(Update::Child {
            session: session.to_string(),
            push: push.clone(),
        })
    })
}

/// 一个子会话的推送照它的编号交给界面；它又派了子代理的，编号交回引擎线程去订阅。它停了（会话表没了）就收。
/// 掉了队补不上的告诉界面一声（[`Update::ChildBehind`]），不一声不吭地收（审计 #063）。
async fn forward(
    mut followed: Followed,
    session: String,
    deliver: Deliver,
    more: mpsc::UnboundedSender<String>,
) {
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
        if !hand_over(&pushes, &session, &deliver, &more) {
            return;
        }
    }
}

/// 读后台命令 `job` 的输出的末尾（[`TAIL`]），交回字；读不到的（不认识、是子代理、没存下来）是 `None`。读文件放在阻塞线程上，
/// 不占引擎线程的运行时。
pub async fn output(main: &Handle, job: &str) -> Option<String> {
    let job = JobId::parse(job).ok()?;
    let read = main.job_output(job).await.ok()?;
    let output = match read {
        Ok(output) => output,
        Err(Unreadable::Unknown | Unreadable::Agent) => return None,
    };
    let mut text = output.text?;
    tokio::task::spawn_blocking(move || {
        let end = text.seek(SeekFrom::End(0)).ok()?;
        text.seek(SeekFrom::Start(end.saturating_sub(TAIL))).ok()?;
        let mut bytes = Vec::new();
        text.read_to_end(&mut bytes).ok()?;
        // 从中间截的：头上半个字照替换字符读，不报错。
        Some(String::from_utf8_lossy(&bytes).into_owned())
    })
    .await
    .ok()
    .flatten()
}

/// 人停掉任务 `job`：交回停不了的原因（不认识、已经结束了）；停好了是 `None`。
pub async fn stop(
    main: &Handle,
    job: &str,
    cause: lm_kernel::kernel::id::CommandId,
) -> Option<String> {
    let Ok(id) = JobId::parse(job) else {
        return Some("unknown_job".to_string());
    };
    match main
        .stop_job(id, By::Person(Person::default()), cause)
        .await
    {
        Ok(Ok(())) => None,
        Ok(Err(lm_engine::tool::JobError::Ended)) => Some("job_ended".to_string()),
        Ok(Err(lm_engine::tool::JobError::Unknown)) => Some("unknown_job".to_string()),
        Err(_) => Some("session stopped".to_string()),
    }
}

#[cfg(test)]
mod audit {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use lm_engine::session::{Pushed, Rejoin, Subscription};
    use tokio::sync::{broadcast, mpsc};

    use super::{Followed, forward};
    use crate::core::{Deliver, Update};

    /// 审计 2026-11-18 #063：子会话的推送掉了队（广播的队满了、旧的被挤掉），转发的任务原来一声不吭就收了：之后子代理要人确认的
    /// 高危命令再也到不了界面，子代理一直等。现在照落了盘的重新订阅补上（`lm-engine` 的 `tests/session/backlog.rs` 演补上的
    /// 那一半）；补不上的（这里演的：会话已经停了）告诉界面一声「子代理的消息可能缺了一段」（[`Update::ChildBehind`]）。
    #[tokio::test]
    async fn a_child_that_falls_behind_is_not_dropped_silently() {
        let (tx, rx) = broadcast::channel(16);
        let subscription = Subscription::from_receiver(rx);
        for _ in 0..40 {
            drop(tx.send(Arc::new(Pushed::Events(Vec::new()))));
        }
        drop(tx);
        let told = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&told);
        let deliver: Deliver = Arc::new(move |update| {
            if matches!(&update, Update::ChildBehind) {
                count.fetch_add(1, Ordering::Relaxed);
            }
            true
        });
        let (more, _agents) = mpsc::unbounded_channel();
        let followed = Followed {
            subscription,
            rejoin: Rejoin::gone(),
            seen: 0,
        };
        drop(
            tokio::time::timeout(
                Duration::from_secs(5),
                forward(followed, "s".to_string(), deliver, more),
            )
            .await,
        );
        assert_eq!(
            told.load(Ordering::Relaxed),
            1,
            "掉了队补不上，界面收到一声"
        );
    }
}
