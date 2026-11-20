//! 读回放掉的段（红线 5，`app/window.rs`）、列日志里你说的话（`/jump`，`transcript/said.rs`）在引擎线程这一头：引擎拿着
//! 这个会话日志的只读入口（打开旧会话时补发剩下的那一份；新开的会话头一次要时订阅一次拿；看着的子会话照切进去时留的
//! 那一份）在单起、读完就退的线程里读那一截，照补发读成推送交回（[`Update::ReadBack`]），或者一块块办进草稿、只交回你说的话（[`Update::Spoken`]）。读的线程退了，
//! 解日志的缓存跟着还回去，常驻的线程不留。

use lm_engine::session::{Backlog, Handle};

use super::super::{Push, Update, push};
use super::{Engine, QUIET};
use crate::config::Texts;
use crate::transcript::Gather;

impl Engine {
    /// 读回会话 `session` 从第 `piece` 段起的 `count` 段：日志里序号在 `after` 之后、`upto` 为止的事件。不是主会话、也不是
    /// 看着的子会话的不读。
    pub(super) async fn read_back(
        &mut self,
        session: &str,
        (piece, count): (u32, u32),
        (after, upto): (u64, u64),
    ) {
        let Some((session, slice)) = self.slice(session, after, upto).await else {
            return;
        };
        let deliver = std::sync::Arc::clone(&self.deliver);
        let before = self.handed.take();
        self.handed = Some(tokio::spawn(async move {
            let pushes = slice
                .read_then(|events| {
                    let mut replay = push::replay::Replay::default();
                    let mut pushes: Vec<Push> =
                        events.into_iter().flat_map(|e| replay.read(e)).collect();
                    pushes.extend(replay.finish());
                    pushes
                })
                .await;
            in_turn(before).await;
            if !deliver(Update::ReadBack {
                session,
                piece,
                count,
                pushes,
            }) {}
        }));
        self.settle_later();
    }

    /// 列会话 `session` 日志里序号到 `upto` 为止你说的话：一块块读、当场补发、办进草稿（`transcript/said.rs`），不攒整份
    /// 事件、推送。
    pub(super) async fn spoken(&mut self, session: &str, upto: u64, texts: Box<Texts>) {
        let Some((session, slice)) = self.slice(session, 0, upto).await else {
            return;
        };
        let deliver = std::sync::Arc::clone(&self.deliver);
        let before = self.handed.take();
        self.handed = Some(tokio::spawn(async move {
            let state = (push::replay::Replay::default(), Gather::new(texts));
            let said = slice
                .fold(state, |(replay, gather), events| {
                    for push in events.into_iter().flat_map(|e| replay.read(e)) {
                        gather.push(push);
                    }
                })
                .await
                .map(|(mut replay, mut gather)| {
                    for push in replay.finish() {
                        gather.push(push);
                    }
                    gather.finish()
                });
            in_turn(before).await;
            if !deliver(Update::Spoken { session, said }) {}
        }));
        self.settle_later();
    }

    /// 读回、列你说的话以后安静 [`QUIET`] 还一次堆：读的线程放掉的、界面放回又放掉的段留下的空页（翻一万轮翻一趟，原来要等
    /// 下一条会话的推送来过才还）。又读回了就重新等。
    pub(super) fn settle_later(&mut self) {
        if let Some(waiting) = self.settling.take() {
            waiting.abort();
        }
        let task = tokio::spawn(async {
            tokio::time::sleep(QUIET).await;
            lm_heap::trim();
        });
        self.settling = Some(task.abort_handle());
    }

    /// 会话 `id`（主会话、切进去看着的子会话）日志里序号在 `after` 之后、`upto` 为止的那一截，连同会话编号；别的会话
    /// （换过会话、退出子会话以后才到的）没有。
    async fn slice(&mut self, id: &str, after: u64, upto: u64) -> Option<(String, Backlog)> {
        if let Some(viewing) = &self.viewing
            && viewing.handle.id().as_str() == id
        {
            return Some((id.to_string(), viewing.reader.slice(after, upto)));
        }
        // 只读打开的坏了一行的会话（`sessions.rs` 的 `view_broken`）：没有会话，照坏行以前的那一截读。
        if self.read_only.as_deref() == Some(id) {
            let reader = self.reader.as_ref()?;
            return Some((id.to_string(), reader.slice(after, upto)));
        }
        let session: Handle = self.session.clone()?;
        if session.id().as_str() != id {
            return None;
        }
        if self.reader.is_none() {
            let (subscription, backlog) = session.subscribe_after(0).await.ok()?;
            drop(subscription);
            self.reader = Some(backlog);
        }
        let reader = self.reader.as_ref()?;
        Some((session.id().as_str().to_string(), reader.slice(after, upto)))
    }
}

/// 前一次要的读回、列你说的话交了再交（读照旧各读各的、并着读）：机器忙时后要的那一截（视口附近、靠上）可能先读完，先放回去
/// 的段和视口之间隔着一块还没读回的空档，那一块后到时视口照看着的那一条不动，和全留的就差了那一块估错的行数（第十三趟，
/// 伪终端 `window` 那条在机器负载上千时撞上）。照要的先后交，放回的段总挨着已经是真的那一边。
async fn in_turn(before: Option<tokio::task::JoinHandle<()>>) {
    if let Some(before) = before
        && before.await.is_err()
    {}
}
