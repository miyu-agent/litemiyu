//! 会话的进出（设计第五节第 8、12 条，Miyu 蓝图 `tui.md`「会话列表 `/sessions`」）：列磁盘上的会话、打开一个旧会话。
//!
//! Miyu 的终端向核心要列表（`session.list`）、切过去时核心载入再补发；LiteMiyu 在引擎线程里直接调
//! `lm_engine::session::list_sessions`（阻塞线程里扫）、`load`，补发照 `subscribe_after(0)` 从日志读。

use std::sync::Arc;

use lm_engine::session::Backlog;
use lm_engine::store::paths::{Env, Paths};

use super::super::{Push, Update};
use super::Engine;
use super::handing::{Handing, To};
use super::setup::{self, NotLoaded};

impl Engine {
    /// 列磁盘上的主会话（连这个窗口开着的），最近有动静的在前，交回界面。数据放哪照 `LITEMIYU_HOME`、XDG（不用先造好
    /// 会话要的几样）。
    pub(super) async fn list(&self) {
        let listed = lm_engine::spawn_blocking(move || {
            let paths = Paths::locate(&Env::current()).map_err(|error| error.to_string())?;
            lm_engine::session::list_sessions(&paths).map_err(|error| error.to_string())
        })
        .await
        .unwrap_or_else(|error| Err(error.to_string()));
        let parked = self.parked.keys().cloned().collect();
        self.report(Update::Sessions { listed, parked });
    }

    /// 导出会话 `id` 到 `path`（`/export`）：在阻塞线程里照 [`crate::cli::export_session`] 的写法直接写成 Markdown（不再起
    /// 自己跑子命令，审计 #068），交回写好的文件多大，或者为什么没写成（`litemiyu: ` 打头的那一句）。
    pub(super) async fn export(&self, id: String, path: std::path::PathBuf) {
        let done = lm_engine::spawn_blocking(move || {
            crate::cli::export_session(&id, &path)?;
            let size = std::fs::metadata(&path)
                .map_err(|error| error.to_string())?
                .len();
            Ok((path, size))
        })
        .await
        .unwrap_or_else(|error| Err(error.to_string()));
        self.report(Update::Exported(done));
    }

    /// 删掉会话 `id`（`/delete`）：在阻塞线程里拿着锁直接删掉它的目录，交回删没删成。正在说话的
    /// 那个界面先发了 `New` 停下它（引擎线程照先后办，到这里已经放下了锁）。
    pub(super) async fn delete(&mut self, id: String) {
        // 留在后台接着跑的（LM-45）：先停下、放开它的锁，不然删的时候撞上自己拿着的锁。
        if let Some(parked) = self.parked.remove(&id)
            && parked.stop().await.is_err()
        {}
        let done = lm_engine::spawn_blocking(move || {
            let paths = Paths::locate(&Env::current()).map_err(|error| error.to_string())?;
            // 原因照引擎的原话（`no such session`、`the session is open in another window`），界面照 `why` 换成句子（LM-39）。
            lm_engine::session::remove_session(&paths, &id)
                .map(|_| ())
                .map_err(|error| error.to_string())
        })
        .await
        .unwrap_or_else(|error| Err(error.to_string()));
        self.report(Update::Deleted(done));
    }

    /// 打开旧会话 `id`：先载入它（别的窗口开着的、打不开的照实报，正在说话的会话照旧），载入了再停下正在说话的那个，
    /// 换成它；从头补发它日志里的事件（边读边交），接着照常转推送。日志中间坏了一行的，`view_broken` 的只读打开坏行以前的
    /// （[`Engine::view_broken`]），不是的照打不开报。
    pub(super) async fn open_old(&mut self, id: String, view_broken: bool) {
        if self.ready.is_none() {
            match setup::Ready::new(self.rescue.take(), self.unattended) {
                Ok(ready) => self.ready = Some(ready),
                Err(message) => return self.report(Update::Failed(message)),
            }
        }
        self.want_catalog();
        let Some(ready) = &self.ready else {
            return;
        };
        // 留在后台接着跑的（LM-45）：它就活在这个进程里，照它的把手接上，不重新载入（载入会撞上自己拿着的锁）。
        let parked = match self.parked.remove(&id) {
            // 已经停了的不算（别的窗口接走过）：照磁盘上的载入。
            Some(parked) if parked.current().await.is_ok() => Some(parked),
            _ => None,
        };
        let loaded = match parked {
            Some(parked) => Ok(parked),
            None => match ready.load(&id).await {
                // 别的窗口开着它：请那边放手（它在那边留在后台跑着的会放），等锁放开再载入（LM-45）。
                Err(NotLoaded::Taken) => ready.take_over(&id).await,
                loaded => loaded,
            },
        };
        let handle = match loaded {
            Ok(handle) => handle,
            Err(NotLoaded::Broken {
                dir, line, upto, ..
            }) if view_broken => return self.view_broken(id, dir, (line, upto)).await,
            Err(why) => {
                let (taken, unavailable, message) = match why {
                    NotLoaded::Taken => (true, false, String::new()),
                    NotLoaded::Unavailable(message) => (false, true, message),
                    NotLoaded::Broken { message, .. } | NotLoaded::Failed(message) => {
                        (false, false, message)
                    }
                };
                return self.report(Update::OpenFailed {
                    taken,
                    unavailable,
                    message,
                });
            }
        };
        self.leave().await;
        self.unattend(Some(&handle));
        let Ok((subscription, backlog)) = handle.subscribe_after(0).await else {
            return self.report(Update::OpenFailed {
                taken: false,
                unavailable: false,
                message: "session stopped".to_string(),
            });
        };
        self.report(Update::Opened(id));
        self.report(Update::Loops(handle.loops()));
        self.report(Update::LoopNow(handle.looping()));
        self.reader = Some(backlog.slice(0, 0));
        self.replay(backlog).await;
        // 补发时读、解、补发、界面放掉的一大堆：安静下来以后还一次堆（原来要等下一条推送来过才还，打开以后一直留着）。
        self.follow(subscription, handle, true);
        self.tell_preset();
    }

    /// 日志中间坏了一行的会话 `id`（在 `dir`，坏在第 `line` 行，读到第 `upto` 条为止）只读打开（同 Miyu 核心 YC-6：日志一个
    /// 字节都不动、不修）：停下正在说话的那个，补发坏行以前的，交 [`Update::ReadOnly`]。不载入、不拿锁、不订阅；之后读回、
    /// 列你说的话照这一截读（`readback.rs`），要会话办的照 `session_read_only` 拒（`engine.rs` 的 `refuse_read_only`）。
    async fn view_broken(
        &mut self,
        id: String,
        dir: std::path::PathBuf,
        (line, upto): (usize, u64),
    ) {
        self.leave().await;
        let backlog = Backlog::broken(dir, upto);
        self.report(Update::Opened(id.clone()));
        self.reader = Some(backlog.slice(0, 0));
        self.read_only = Some(id);
        self.replay(backlog).await;
        self.report(Update::ReadOnly { line });
        self.settle_later();
    }

    /// 从头补发 `backlog`：日志一块块读、当场补发、一条条交给界面（边读边交，第十二趟），不攒整份事件、整份推送；读和补发
    /// 都在单起、读完就退的线程里（`Backlog::fold`），解日志、补发时的分配跟着那条线程放掉。
    async fn replay(&self, backlog: Backlog) {
        let state = Handing::new(Arc::clone(&self.deliver), To::Opened(self.agents.0.clone()));
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
            // 读坏了：交了的照样交过了，钟拨回现在再报。
            Err(message) => {
                self.report(Update::Push(Push::Clock(None)));
                self.report(Update::Failed(message));
            }
        }
    }
}
