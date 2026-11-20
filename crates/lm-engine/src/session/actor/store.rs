//! 落盘和读回（施工 3-7 中；施工 6-9 从 `actor.rs` 挪出来，那边放不下了）：在阻塞线程里写一批，提交点的交给后台同步
//! （施工 V-19）；撤掉压缩时读回更早的日志（`docs/blueprint/session/actor.md` 第 4 条）。写不进去、读不回来，会话都停下：内存里的会话和磁盘对不上了，
//! 从磁盘重新载入最清楚（`07-存储.md` 第四节「写不进去」）。

use lm_kernel::kernel::block::Block;
use lm_kernel::kernel::event::{Body, CompactableChanged, Event, Transient, TransientBody};
use lm_kernel::kernel::id::{CommandId, Seq};
use lm_kernel::kernel::origin::By;
use lm_kernel::kernel::session::{Input, Outcome};

use std::io;
use std::sync::{Arc, Mutex};

use tokio::sync::mpsc;

use super::Synced;
use super::durable::Syncing;
use super::{Actor, Stop, answer};
use crate::session::current::{Current, Tally};
use crate::session::effects;
use crate::session::store::Flush;
use crate::store::blob::Unsynced;

impl Actor {
    /// 这一刻「当前的」几样（施工 9-6 上）：订阅时和订阅在同一步里交回。权限、还在跑的任务照内核这一刻的，累计的照给头看的
    /// 那一份。
    pub(super) fn current(&self) -> Current {
        // 只拿一次锁：同一条语句里拿两次，第一把要到语句结束才放，第二次就等死在它上面（施工 X-6 写测试时撞上的）。
        let counted = self.counted();
        Current {
            jobs: self.session.running_started(),
            cwd: self.session.cwd().to_string(),
            dirs: self.session.dirs().to_vec(),
            tally: counted.tally.clone(),
            model: self.session.reference().map(str::to_string),
            compactable: self.session.compactable(),
            least_context: self.session.least_context(),
            compaction_line: self.session.pinned_line(),
        }
    }

    /// 送完一批以后推给头的（施工 W-12 补）：有没有能压的变了推一条 `compactable.changed`。头一回
    /// 算只记下：订阅的回应带着这一刻的。`moved` 是假的（这一批全是流式的碎片，施工 V-22）不看有没有能压的：变不了。
    pub(super) fn push_now(&mut self, moved: bool) {
        if !moved {
            return;
        }
        let now = self.session.compactable();
        let before = self.counted().compactable.replace(now);
        if before.is_none_or(|before| before == now) {
            return;
        }
        let (turn, cause) = self
            .session
            .turn_cause()
            .map_or((None, None), |(turn, cause)| (Some(turn), cause));
        let at = self.clock.now();
        self.push(crate::session::handle::Pushed::Transient(Transient {
            at,
            turn,
            by: By::Kernel,
            cause,
            body: TransientBody::CompactableChanged(CompactableChanged { compactable: now }),
        }));
    }

    /// 载入时照整份日志算的累计（施工 9-6 上）：放进给头看的那一份，之后每落一批盘加上这一批。
    pub(crate) fn count_from(&self, tally: Tally) {
        self.counted().tally = tally;
    }

    /// 给头看的那一份，拿着锁。
    pub(super) fn counted(&self) -> std::sync::MutexGuard<'_, crate::session::shown::Shown> {
        self.shown
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// 在阻塞线程里写，写完交回「落盘了」；要同步的交给后台，不等它（施工 V-19，`durable.rs`）。写不进去就停下。撤销、
    /// 恢复落了盘，她看过的照日志重算一遍
    /// （施工 4-7 上）：重算不出来的记一条运行日志，照旧用原来的，改的工具照样先核对。
    ///
    /// 不撞自己的同步（速度线二）：后台同步在路上时，不用同步的这一批（模型的纯文字、工具的结果、`model.called`……，
    /// [`must_sync`] 是假的）不写，攒在 `unwritten` 里当场交回「落盘了」；同步做完了一次写进去（[`Actor::write_held`]）。
    /// 写进去的那一下要等同一个文件上正在做的同步（btrfs 的 `fsync` 拿着这个文件的锁），盘忙时一等上百毫秒，挡在每一步上。
    /// 要同步的那几批（人说的话、带工具调用的回复、回合结束……）照旧写进去才交回，连同攒着的一起照先后写：工具派出去之前，
    /// 派它的回复一定在日志里。程序被杀时丢的只有攒着的那几批。
    pub(super) async fn append(&mut self, events: Vec<Event>) -> Result<Option<Input>, Stop> {
        let Some(upto) = events.last().map(|event| event.seq) else {
            return Ok(None);
        };
        let reseen = effects::reverts(&events);
        // 派出去的任务、回报记进名册（施工 7-4）：写不进去的，会话照样停下，记了也不要紧。
        self.jobs.note(&events);
        // 压缩、撤销、恢复了的，交过的子目录项目指令清掉重来（施工 P-14 下）。
        self.tools.note_docs(&events);
        // 累计的也是（施工 9-6 上）：写完、内核记下「落了盘」之前收件箱不动，订阅插不进来，和它拿到的落盘到哪一条对得上。
        self.counted().tally.add(&events);
        // 子会话的也加进会话表里子代理一共用掉的那一份：主会话的概要照它记，重开时不读子会话的日志。
        if let Some(agents) = &self.agents {
            agents
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .add(&events);
        }
        // 换了上下文上限的当场交给端口（施工 CTX-1，`model/context.rs`）。
        self.follow_context(&events);
        // 内核截了模型编的下一轮的，记一行（Miyu 施工 FT-1）。
        // 会话的概要跟着记（L7 一，`crate::session::window`）。
        if let Some(window) = &mut self.window {
            window.saw(&events);
        }
        let sync = must_sync(&events);
        let background = self.store.as_ref().is_some_and(|store| store.background());
        // 这一批引用的 blob 都在这一刻登记过的里面（速度线三）：写进去以前要同步到这一个。
        let (busy, needs, waiting) = {
            let shared = lock(&self.sync);
            let blobs = lock_blobs(&shared.blobs);
            let needs = blobs.latest();
            let waiting = needs.is_some_and(|needs| blobs.waiting(needs));
            (shared.durable.busy(), needs, waiting)
        };
        if background && !sync && (busy || waiting) {
            self.unwritten.extend(events);
            self.unwritten_needs = needs;
            // 只是 blob 没同步、路上没有同步的：派一次，做完了写进去。
            if !busy && let Some(upto) = lock(&self.sync).durable.again() {
                self.dispatch(upto);
            }
            return Ok(Some(Input::Stored {
                at: self.clock.now(),
                upto,
            }));
        }
        self.unwritten_needs = None;
        let events = match self.unwritten.is_empty() {
            true => events,
            false => {
                let mut all = std::mem::take(&mut self.unwritten);
                all.extend(events);
                all
            }
        };
        let seen = self.write_out(events, sync, reseen, needs).await?;
        match seen {
            Some(Ok(seen)) => self.tools.see(seen),
            Some(Err(_)) => {}
            None => {}
        }
        Ok(Some(Input::Stored {
            at: self.clock.now(),
            upto,
        }))
    }

    /// 把攒着的写进去（速度线二，[`Actor::append`]）：同步做完了、要读日志（订阅补发、撤掉压缩读回）、停下以前。没有攒着的
    /// 什么都不做。写不进去就停下。
    pub(super) async fn write_held(&mut self) -> Result<(), Stop> {
        if self.unwritten.is_empty() {
            return Ok(());
        }
        let events = std::mem::take(&mut self.unwritten);
        let needs = self.unwritten_needs.take();
        self.write_out(events, false, false, needs)
            .await
            .map(|_| ())
    }

    /// 一串后台同步做完了：出错的停下；成了的，路上没有别的同步了就把攒着的写进去（速度线二）。攒着的引用的 blob 还有没
    /// 同步的（这一串开始以后才存的，速度线三），再派一次，做完了再写。
    pub(super) async fn after_sync(&mut self, result: io::Result<()>) -> Result<(), Stop> {
        self.synced(result)?;
        let again = {
            let mut shared = lock(&self.sync);
            if shared.durable.busy() {
                return Ok(());
            }
            let waiting = self
                .unwritten_needs
                .is_some_and(|needs| lock_blobs(&shared.blobs).waiting(needs));
            match waiting && !self.unwritten.is_empty() {
                true => shared.durable.again(),
                false => None,
            }
        };
        match again {
            Some(upto) => {
                self.dispatch(upto);
                Ok(())
            }
            None => self.write_held().await,
        }
    }

    /// 在阻塞线程里写一批（不空），记进账里；`reseen` 的写完照日志重算她看过的，交回重算的。写不进去就停下。登记到 `needs`
    /// 的 blob 还有没同步的，先同步（速度线三：引用它们的事件不比它们先进日志）。
    async fn write_out(
        &mut self,
        events: Vec<Event>,
        sync: bool,
        reseen: bool,
        needs: Option<u64>,
    ) -> Result<Option<Result<crate::tool::Seen, String>>, Stop> {
        let Some(upto) = events.last().map(|event| event.seq) else {
            return Ok(None);
        };
        let mut store = self.store.take().ok_or(Stop)?;
        // 同步挪到后台的（施工 V-19）：只写、不同步，写完拿最新的句柄；不挪的照旧连同同步写。
        let background = store.background();
        let blobs = Arc::clone(&lock(&self.sync).blobs);
        let written = tokio::task::spawn_blocking(move || {
            if let Some(needs) = needs {
                sync_blobs(&blobs, needs);
            }
            let result = match background {
                true => store
                    .write(&events, false)
                    .and_then(|()| store.sync_handle().map(Some)),
                false => store.write(&events, sync).map(|()| None),
            };
            // 边读边记，不把整份日志攒在内存里（施工 V-41 中）。
            let seen = (reseen && result.is_ok()).then(|| store.seen());
            (store, result, seen)
        })
        .await;
        match written {
            Ok((store, Ok(handle), seen)) => {
                self.store = Some(store);
                self.written(upto, sync, handle);
                Ok(seen)
            }
            Ok((_, Err(_), _)) => Err(Stop),
            Err(_) => Err(Stop),
        }
    }

    /// 读回日志（施工 6-9）：在阻塞线程里只读地一段一段读，交回第 `from` 条起的。读不了的（日志坏了、磁盘出错），记一行
    /// `read back failed, stopped`，会话停下；读的线程 panic 了，记一行 `panicked, stopped`（只在测试的配置下走得到：
    /// 发行配置 panic = abort（`docs/00-设计.md` 第九节），发出去的程序崩了整个进程退出）。
    pub(super) async fn read_back(&mut self, from: Seq) -> Result<Input, Stop> {
        match self.events_from(from).await? {
            Ok(events) => Ok(Input::ReadBack {
                at: self.clock.now(),
                from,
                events,
            }),
            Err(_) => Err(Stop),
        }
    }

    /// 在阻塞线程里只读地读第 `from` 条起到末尾的日志（撤掉压缩时读回、收成槽的放回来，`shelf.rs`）：攒着的先写进去，读回的那一截
    /// 要齐（速度线二）。读不了的交回为什么；写不进去、读的线程 panic 了的（记一行 `panicked, stopped`）停下。
    pub(super) async fn events_from(
        &mut self,
        from: Seq,
    ) -> Result<Result<Vec<Event>, String>, Stop> {
        self.write_held().await?;
        let store = self.store.take().ok_or(Stop)?;
        let read = tokio::task::spawn_blocking(move || {
            let events = store.events_from(from);
            (store, events)
        })
        .await;
        match read {
            Ok((store, events)) => {
                self.store = Some(store);
                Ok(events)
            }
            Err(_) => Err(Stop),
        }
    }
}

impl Actor {
    /// 写进去了一批（施工 V-19，`session/actor.md` 第 5 条第 9 款）：有句柄的记下，要同步的交给后台；没有句柄的（不做后台
    /// 同步的）写的时候已经同步过，当场算同步到了这一条。
    fn written(&mut self, upto: Seq, sync: bool, handle: Option<Arc<dyn Flush>>) {
        let mut shared = lock(&self.sync);
        match handle {
            Some(handle) => {
                shared.flush = Some(handle);
                let start = shared.durable.written(upto, sync);
                drop(shared);
                if let Some(upto) = start {
                    self.dispatch(upto);
                }
            }
            None => {
                shared.durable.written(upto, false);
                shared.durable.done(upto);
            }
        }
    }

    /// 回应命令 `id`：交给等着它的最早的那一头。
    pub(super) fn reply(&mut self, id: CommandId, outcome: Outcome) {
        let Some(waiting) = self.replies.get_mut(&id) else {
            return;
        };
        let first = waiting.pop_front();
        if waiting.is_empty() {
            self.replies.remove(&id);
        }
        // 当场交：它的事件写进去了才走到这里，程序被杀也不丢；不扣着等同步（速度线一，`durable.rs` 模块文档）。
        if let Some(reply) = first {
            answer(reply, outcome);
        }
    }

    /// 派一次后台同步，同步到 `upto`（`dispatch`）。
    pub(super) fn dispatch(&self, upto: Seq) {
        dispatch(Arc::clone(&self.sync), self.syncs.clone(), upto);
    }

    /// 一串后台同步做完了报回来的：出错的和写不进去一样，记一行，会话停下。
    pub(super) fn synced(&mut self, result: io::Result<()>) -> Result<(), Stop> {
        match result {
            Ok(()) => Ok(()),
            Err(_) => Err(Stop),
        }
    }

    /// 停下以前（第 11 款）：等路上的同步做完，攒着的写进去（速度线二），把写了还没同步的同步完，放下句柄（Windows 上开着的句柄挡着挪目录）。同步
    /// 不了的记一行。
    pub(super) async fn settle_sync(&mut self) {
        while lock(&self.sync).durable.busy() {
            match self.synced.recv().await {
                Some((_, Ok(()))) => {}
                Some((_, Err(_))) => {
                    return;
                }
                None => break,
            }
        }
        // 攒着的写进去，下面一起同步（速度线二）。
        if self.write_held().await.is_err() {
            return;
        }
        let (pending, flush) = {
            let shared = lock(&self.sync);
            (shared.durable.pending(), shared.flush.clone())
        };
        if let (Some(upto), Some(flush)) = (pending, flush) {
            match tokio::task::spawn_blocking(move || flush.flush()).await {
                Ok(Ok(())) => {
                    lock(&self.sync).durable.done(upto);
                }
                Ok(Err(_)) => {
                    return;
                }
                // 同步的线程 panic 了：只在测试的配置下走得到（发行配置 panic = abort（`docs/00-设计.md` 第九节））。
                Err(_) => {
                    return;
                }
            }
        }
        lock(&self.sync).flush = None;
    }
}

/// 派一串后台同步，从同步到 `upto` 起（施工 V-19）：在阻塞线程里对最新的句柄做，账上还要再做的接着做，都做完了、或者
/// 出了错，报回 actor（`synced`）。
fn dispatch(shared: Arc<Mutex<Syncing>>, done: mpsc::UnboundedSender<Synced>, mut upto: Seq) {
    tokio::task::spawn_blocking(move || {
        loop {
            // 先同步存了的 blob，再同步日志（速度线三）：日志里引用它们的那几条落盘以前，它们先落盘。
            let (flush, blobs) = {
                let syncing = lock(&shared);
                (syncing.flush.clone(), Arc::clone(&syncing.blobs))
            };
            sync_blobs(&blobs, u64::MAX);
            let result = flush.map_or(Ok(()), |flush| flush.flush());
            if let Err(error) = result {
                // actor 已经退出了：没人收，丢掉。
                drop(done.send((upto, Err(error))));
                return;
            }
            let next = lock(&shared).durable.done(upto);
            match next {
                Some(next) => upto = next,
                None => {
                    drop(done.send((upto, Ok(()))));
                    return;
                }
            }
        }
    });
}

/// 同步登记到 `upto`（含）的 blob（速度线三）。同步不了的记一行、照同步了算：和以前存不下来一样，只是看差异、撤销时取
/// 不出来，不为它停下会话。
fn sync_blobs(blobs: &Mutex<Unsynced>, upto: u64) {
    let Some((last, paths)) = lock_blobs(blobs).upto(upto) else {
        return;
    };
    drop(crate::store::blob::sync_files(&paths));
    lock_blobs(blobs).synced(last);
}

/// 拿着存了没同步的 blob 的锁，同 [`lock`]。
fn lock_blobs(blobs: &Mutex<Unsynced>) -> std::sync::MutexGuard<'_, Unsynced> {
    blobs
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// 拿着账的锁：拿锁的那一方 panic 过的照样用（账上的都是完整的一步一步记的）。
fn lock(shared: &Mutex<Syncing>) -> std::sync::MutexGuard<'_, Syncing> {
    shared
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// 这一批要不要同步到磁盘（施工 V-18，`session/actor.md` 第 5 条第 1 款）。要的是「之后会碰外面」的几种：
///
/// - 人、别的会话交来的（`by` 不是模型、工具、内核）：对方当它收下了（回应在写进去以后就交，不等这次同步，速度线一），尽快同步。
/// - 带工具调用的回复：落了盘才派工具，工具会改文件、跑命令；断电以后日志里要看得到它派过。
/// - 回合结束、会话造好、撤销和恢复：一轮收尾的地方。
///
/// 剩下的（模型的纯文字回复、工具的结果、`model.called`、注入的上下文……）只交给系统：程序崩了照样在，断电丢了的
/// 那一截载入时照「崩在回合里」收尾（`aborted`），和只差一两条的崩溃是同一条路。工具结果里引用的 blob 是先落盘的，
/// 丢了结果只是少了引用。
fn must_sync(events: &[Event]) -> bool {
    events.iter().any(|event| {
        let outside = !matches!(event.by, By::Model(_) | By::Tool(_) | By::Kernel);
        outside
            || match &event.body {
                Body::MessageAssistant(reply) => reply
                    .blocks
                    .iter()
                    .any(|block| matches!(block, Block::ToolCall(_))),
                Body::SessionCreated(_)
                | Body::TurnEnded(_)
                | Body::TurnReverted(_)
                | Body::TurnUnreverted(_) => true,
                _ => false,
            }
    })
}

#[cfg(test)]
mod tests;
