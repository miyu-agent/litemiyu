//! 后台同步（施工 V-19，`docs/blueprint/session/actor.md` 第 5 条第 9、11 款）：写进去就往下走，提交点的那几批交给后台同步。
//! 这里只管账：写到哪、同步到哪、正在做的同步到哪、还要做到哪；不碰盘，碰盘的在 `actor/store.rs`。
//!
//! 同步一次只做一个：做的时候又来了要同步的，记下最远的那一条，做完接着派一次，几批合成一次（盘忙时一次同步要几秒，
//! 攒在一起的就不用各等各的）。
//!
//! 命令的回应不扣着等同步（速度线一）：Miyu 给外面来的命令（人、桥、别的 harness）扣着回应，等它的事件同步了才交，交到
//! 对方手里的是断电也在的承诺。LiteMiyu 的命令都来自同一个进程（界面、别的会话、定时、工具），断电时和引擎一起没了，
//! 承诺给不出去也没人收；扣着只会让界面的命令循环等一到两次同步（盘忙时几百毫秒，那时按 Esc、再发一句都排在后面）。
//! 所以回应在它的事件写进去以后就交：程序被杀不丢（写进去了，载入时还在），断电丢的和别的事件一样，是最后一次同步以后的
//! 那一截。

use std::sync::{Arc, Mutex};

use lm_kernel::kernel::id::Seq;

use crate::session::store::Flush;
use crate::store::blob::Unsynced;

/// 后台同步的账。
#[derive(Debug, Default)]
pub(crate) struct Durable {
    /// 写进去的最后一条。
    written: Option<Seq>,
    /// 同步到了哪一条（含）。
    synced: Option<Seq>,
    /// 正在做的同步到哪一条。
    flying: Option<Seq>,
    /// 正在做的时候又要同步的，最远到哪一条：做完接着派。
    wanted: Option<Seq>,
}

impl Durable {
    /// 写进去了一批，最后一条是 `upto`；`must` 的这一批要同步（V-18 的提交点）。交回该当场派的同步：同步到哪。
    pub(crate) fn written(&mut self, upto: Seq, must: bool) -> Option<Seq> {
        self.written = Some(upto);
        match must {
            true => self.ask(upto),
            false => None,
        }
    }

    /// 一次同步做完了，同步到 `upto`：交回该接着派的同步。
    pub(crate) fn done(&mut self, upto: Seq) -> Option<Seq> {
        self.synced = self.synced.max(Some(upto));
        let synced = self.synced;
        let next = self.wanted.take().filter(|wanted| Some(*wanted) > synced);
        self.flying = next;
        next
    }

    /// 再同步一次，到写进去的最后一条（速度线三：只为同步存了的 blob，日志可能没有新写的）：没有在做的交回它（派出去），
    /// 在做的记下。还没写过的什么都不做。
    pub(crate) fn again(&mut self) -> Option<Seq> {
        let written = self.written?;
        self.ask(written)
    }

    /// 有没有同步在路上（停下以前要等它做完：它拿着日志的句柄，Windows 上挡着挪目录）。
    pub(crate) fn busy(&self) -> bool {
        self.flying.is_some()
    }

    /// 写了还没同步的最后一条：停下以前要同步到这里。都同步了的没有。
    pub(crate) fn pending(&self) -> Option<Seq> {
        self.written.filter(|written| Some(*written) > self.synced)
    }

    /// 要同步到 `upto`：没有在做的交回它（派出去），在做的记下最远的那一条。
    fn ask(&mut self, upto: Seq) -> Option<Seq> {
        match self.flying {
            None => {
                self.flying = Some(upto);
                Some(upto)
            }
            Some(_) => {
                self.wanted = self.wanted.max(Some(upto));
                None
            }
        }
    }
}

/// actor 拿着的那一份（施工 V-19）：账、最新的句柄、存了没同步的 blob。和后台同步的阻塞线程共用。
pub(crate) struct Syncing {
    /// 后台同步的账。
    pub(crate) durable: Durable,
    /// 后台同步用的句柄：每写一批换成最新的（`Store::sync_handle`）；不做后台同步的没有。
    pub(crate) flush: Option<Arc<dyn Flush>>,
    /// 工具存了还没同步的改前改后（速度线三，`Blobs::deferring`）：后台同步每一次先同步它们、再同步日志；写日志以前
    /// 引用到的也先同步（`actor/store.rs`）。
    pub(crate) blobs: Arc<Mutex<Unsynced>>,
}

impl Syncing {
    /// 还没写过的。
    pub(crate) fn new() -> Syncing {
        Syncing {
            durable: Durable::default(),
            flush: None,
            blobs: Arc::default(),
        }
    }

    /// 存了没同步的 blob 登记在 `blobs`（会话的工具那一份）。
    pub(crate) fn with_blobs(mut self, blobs: Arc<Mutex<Unsynced>>) -> Syncing {
        self.blobs = blobs;
        self
    }
}

#[cfg(test)]
mod tests;
