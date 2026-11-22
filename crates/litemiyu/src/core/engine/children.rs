//! 子代理和后台命令在引擎线程这一头（Miyu 蓝图 `tui.md`「后台命令、子代理和侧边栏」「切进子会话」）：主会话派出去的子会话
//! 一开始就订阅着（它要人确认的高危命令、它在做什么都从它自己的会话里来），推送照子会话的编号交给界面；读后台命令的
//! 输出、人停一个任务照主会话的把手办（`Handle::job_output`、`Handle::stop_job`）。
//!
//! 子会话的把手照主会话的把手按编号取（`Handle::child`），订阅好就放下：订阅不拖住它的 actor（主会话放下了，会话表和子会话
//! 跟着没，订阅跟着断）。子会话再派的（孙代理）照样订阅：它们的推送里有 `job.started`，交回这里再取。

use std::io::{Read, Seek, SeekFrom};

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

/// 订阅子会话 `id`：主会话的把手取得到它才订阅，取不到的（已经停了、不是这张表里的）不管。交回转推送的那个任务：放下
/// 主会话时掐掉（`Engine::leave`）。
pub async fn watch(
    main: &Handle,
    id: &str,
    deliver: Deliver,
    more: mpsc::UnboundedSender<String>,
) -> Option<tokio::task::AbortHandle> {
    let session = SessionId::parse(id).ok()?;
    let child = main.child(&session).await?;
    // 从头补：派出去到订阅上之间它可能已经在问人了（只推订阅以后的那一份里就没有那一句）。日志里落了盘的照先后交一遍，
    // 问过又答了的抽屉照推送收（`app/asking.rs`）；它已经说完的那几步照补发的读法读（[`landed`]）。
    let (subscription, backlog) = child.subscribe_after(0).await.ok()?;
    let rejoin = child.rejoin();
    drop(child);
    let seen = backlog.upto();
    if let Ok(events) = backlog.read().await
        && !hand_over(&landed(events), id, &deliver, &more)
    {
        return None;
    }
    let followed = Followed {
        subscription,
        rejoin,
        seen,
        heard: 0,
    };
    Some(tokio::spawn(forward(followed, id.to_string(), deliver, more)).abort_handle())
}

/// 订阅上以前子会话已经落了盘的那一截读成推送，照补发的读法（`push::replay`）：回答照 `message.assistant` 的块开一块、写字、
/// 收一块，和看着流出来时一样。流式的增量只推给当时订阅着的（正在流的那一次订阅时另补，`Handle::subscribe_after`），已经
/// 说完落了盘的那几步照平常读只读得出调用的编号：子代理状态行照块记它在做什么（`jobs/activity.rs`），没有这几块就不知道，
/// 它跑着命令也只写名字和计时。补发垫的钟这里没人看，不交。
fn landed(events: Vec<Event>) -> Vec<Push> {
    let mut replay = push::replay::Replay::default();
    let mut pushes: Vec<Push> = events
        .into_iter()
        .flat_map(|event| replay.read(event))
        .collect();
    pushes.extend(replay.finish());
    pushes.retain(|push| !matches!(push, Push::Clock(_)));
    pushes
}

/// 跟着的一个会话：订阅，掉了队照它重新订阅（不拖住会话），落了盘的看到了第几条，流式的回答听到了哪一次请求的。
pub struct Followed {
    /// 订阅。
    pub subscription: Subscription,
    /// 掉了队照它重新订阅。
    pub rejoin: Rejoin,
    /// 落了盘的最后看到的序号：重新订阅从它后面补。
    pub seen: u64,
    /// 听到过流式的块的最后一次请求（[`Push::Heard`] 的 `seen`）：重新订阅补上的回答里，这一次以前的已经照流式画过了
    /// （[`rejoined`]）。
    pub heard: u64,
}

impl Followed {
    /// 下一份推送读成的几条；掉了队的照落了盘的补（审计 #063）：重新订阅、交回补上的那一截（[`rejoined`] 的读法），之后照新的
    /// 订阅接着推。交回 `Err(Ended::Lagged)` 的是补不上（掉队以后会话停了、日志读不回来），`Err(Ended::Stopped)` 的是会话停了。
    pub async fn next(&mut self) -> Result<Vec<Push>, Ended> {
        let pushes = match self.subscription.next().await {
            Err(Ended::Lagged) => self.rejoin_after().await?,
            next => match &*next? {
                Pushed::Events(events) => {
                    if let Some(last) = events.last() {
                        self.seen = self.seen.max(last.seq.get());
                    }
                    events.iter().flat_map(push::read).collect()
                }
                Pushed::Transient(event) => push::read_transient(event),
            },
        };
        for one in &pushes {
            if let Push::Heard(seen) = one {
                self.heard = self.heard.max(*seen);
            }
        }
        Ok(pushes)
    }

    /// 掉了队：照最后看到的序号重新订阅，交回补上的那一截读成的推送（一条都没有的也是一份空的）。
    async fn rejoin_after(&mut self) -> Result<Vec<Push>, Ended> {
        let (subscription, backlog) = self
            .rejoin
            .subscribe_after(self.seen)
            .await
            .map_err(|_| Ended::Lagged)?;
        let events: Vec<Event> = backlog.read().await.map_err(|_| Ended::Lagged)?;
        self.subscription = subscription;
        if let Some(last) = events.last() {
            self.seen = self.seen.max(last.seq.get());
        }
        Ok(rejoined(events, self.heard))
    }
}

/// 掉了队重新订阅时补上的那一截读成推送（LM-36，同 Miyu LM-25 710203654「晚订阅的一方拿不到正在做什么」；LM-24 留下的那一处）：
/// 掉队期间说完、落了盘的回答，流式的块一条都没收到，照补发的读法读成「开一块、写字、收一块」（同 [`landed`]）：子代理状态行
/// 才知道它在调什么，正在看的子会话的正文里才有这一步。流式的块已经收到过的（请求的 `seen` 不晚于听到过的 `heard`）照平常
/// 读，只配调用的编号：那几块照流式画过了，再开一遍就画重了。别的事件照平常读：补发的读法会把人说的话再画一遍（看着发的那句
/// 先画上了）。补发垫的钟不交。
fn rejoined(events: Vec<Event>, heard: u64) -> Vec<Push> {
    use lm_kernel::kernel::event::Body;
    let mut pushes = Vec::new();
    for event in events {
        match &event.body {
            Body::MessageAssistant(reply) if reply.seen.get() > heard => {
                let mut replay = push::replay::Replay::default();
                pushes.extend(replay.read(event));
                pushes.extend(replay.finish());
            }
            _ => pushes.extend(push::read(&event)),
        }
    }
    pushes.retain(|push| !matches!(push, Push::Clock(_)));
    pushes
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
        let pushes = match followed.next().await {
            Ok(pushes) => pushes,
            Err(Ended::Stopped) => return,
            Err(Ended::Lagged) => {
                deliver(Update::ChildBehind);
                return;
            }
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
mod landed {
    use lm_kernel::kernel::event::Event;

    use crate::human::Human;
    use crate::jobs::{Activity, Doing};

    fn event(json: &str) -> Event {
        serde_json::from_str(json).expect("事件的写法")
    }

    /// 子会话的第一步：开了一轮，说一句、调 `shell`，这次请求说完了。三条都落了盘，命令还在跑（没有结果）。
    pub(super) fn first_step() -> Vec<Event> {
        vec![
            event(
                r#"{"seq":3,"at":"2026-11-20T16:41:19.934Z","kind":"turn.started","turn":3,"by":{"kind":"kernel"},"body":{"trigger":2,"cwd":"/w"}}"#,
            ),
            event(
                r#"{"seq":8,"at":"2026-11-20T16:41:19.939Z","kind":"message.assistant","turn":3,"by":{"kind":"model","endpoint":"fake","model":"m1"},"body":{"seen":7,"blocks":[{"type":"text","text":"一号说完了。"},{"type":"tool_call","call_id":"call_8_1","name":"shell","args":"{\"command\":\"sleep 9\",\"description\":\"Wait for go\"}"}]}}"#,
            ),
            event(
                r#"{"seq":9,"at":"2026-11-20T16:41:19.939Z","kind":"model.called","turn":3,"by":{"kind":"kernel"},"body":{"seen":7,"messages":1,"duration_ms":0,"blocks":[{"start_ms":0,"end_ms":0},{"start_ms":0,"end_ms":0}],"result":"ok"}}"#,
            ),
        ]
    }

    /// LM-24：子会话一派出去就开它的第一轮，界面要等主会话那一步的 `job.started` 推到了才订阅它。订阅晚了一步（机器忙、模型回得
    /// 快），它这一步的回答已经整个落了盘、流式的增量早推完了，补上的只有落了盘的那一截。原来这一截照平常读，回答只读出调用的
    /// 编号，没有「开一块、写字、收一块」：子代理状态行不知道它在做什么，跑着命令也只写名字和计时，看着像挂着不动（伪终端测试
    /// `switching` 加压时 60 遍红 4～10 遍，红的每一遍都是补上的那一截里带着回答的那个子代理）。补上的照补发的读法读
    /// （`push::replay`），和看着流出来时一样。
    #[test]
    fn a_step_that_landed_before_the_subscription_still_says_what_the_child_is_doing() {
        let human = Human::default();
        let mut activity = Activity::default();
        for push in super::landed(first_step()) {
            activity.push(&push, &human);
        }
        assert!(activity.busy, "一轮开着");
        assert_eq!(
            activity.doing,
            Doing::Tool {
                name: "shell".into(),
                subject: Some("Wait for go".into()),
            },
            "补上的那一截里它最后在调 shell"
        );
    }
}

/// LM-36（同 Miyu LM-25 710203654）：子会话的推送掉了队、重新订阅补上的那一截（[`super::rejoined`]）。
#[cfg(test)]
mod rejoined {
    use super::landed::first_step;
    use crate::core::{Push, push};
    use crate::human::Human;
    use crate::jobs::{Activity, Doing};

    /// 照 `pushes` 记的在做什么。
    fn doing(pushes: &[Push]) -> Doing {
        let human = Human::default();
        let mut activity = Activity::default();
        for push in pushes {
            activity.push(push, &human);
        }
        activity.doing
    }

    /// 带 `shell` 调用的那一步在掉队期间整个落了盘，一个流式的块都没收到：补上的照补发的读法读，状态行知道它在调 `shell`。原来
    /// 这一截照平常读（第一个断言），回答只读出调用的编号，状态行不知道它在做什么。
    #[test]
    fn a_step_that_landed_while_behind_still_says_what_the_child_is_doing() {
        let plain: Vec<Push> = first_step().iter().flat_map(push::read).collect();
        assert_eq!(doing(&plain), Doing::Nothing, "原来的读法");
        assert_eq!(
            doing(&super::rejoined(first_step(), 0)),
            Doing::Tool {
                name: "shell".into(),
                subject: Some("Wait for go".into()),
            }
        );
    }

    /// 那一步的流式的块掉队以前已经收到过（听到过它那一次请求，`seen` 是 7）：补上的只配调用的编号，不再开一遍块，正文不画重。
    #[test]
    fn a_step_already_heard_is_not_drawn_again() {
        let pushes = super::rejoined(first_step(), 7);
        let plain: Vec<Push> = first_step().iter().flat_map(push::read).collect();
        assert_eq!(pushes, plain);
        assert!(
            !pushes
                .iter()
                .any(|push| matches!(push, Push::BlockStart { .. } | Push::Said { .. })),
            "{pushes:?}"
        );
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
            heard: 0,
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
