//! 窗口之间（设计第五节第 2、8 条，L7）：LiteMiyu 一个窗口一个进程、没有常驻的核心，Miyu 里核心的会话表替会话们做的那几样
//! 换成数据目录里的文件。
//!
//! - 占有（`lock.rs`）：开着主会话的窗口拿着会话目录里的文件锁，同一个会话不能被两个窗口同时开；回合在进行时另拿一把，别的
//!   窗口照它写忙不忙。进程没了锁跟着没。
//! - 概要（`summary.rs`）：会话目录里一份小文件，列会话时读；还没有的第一轮结束时写，标题变了、会话结束时写。
//! - 列会话（[`list`]）：扫会话目录，不用先开会话。
//!
//! - 收件目录（L7 二，`crate::session::peers`）：拿到锁时建好收件目录、绑上门铃（LM-4），会话造好、载入做完以后
//!   （[`attach`]）起等它的任务，读没开着时攒下的、之后到的。放下锁以前删掉门铃的套接字文件。
//!
//! 一个主会话一个 [`Window`]：造会话、载入时先拿锁（拿不到的造不成、载入不了），交给 actor 拿着；actor 退出时放下，锁跟着放，
//! 概要写最后一次。子会话没有：它们跟着主会话在同一个进程里。

mod list;
mod local;
mod lock;
mod port;
mod remove;
mod summary;

use std::path::PathBuf;

use lm_kernel::kernel::event::{Body, Event};

pub use list::{Listed, list};
use lock::Claim;
pub use lock::Claimed;
pub(crate) use port::port;
pub use remove::{NotRemoved, remove};
use summary::{Summary, Tracker};

use crate::session::table::AgentsSpent;
use crate::store::paths::Bell;

/// 一个窗口对一个主会话的占有：锁、跟着日志记的概要。
#[derive(Debug)]
pub(crate) struct Window {
    dir: PathBuf,
    claim: Claim,
    tracker: Option<Tracker>,
    /// 要等的收件目录（门铃的套接字）：拿到锁时备好，交给 actor 时起等它的任务（[`attach`]）。收件目录建不了的没有，这个
    /// 窗口收不到，下次打开再读。
    inbox: Option<crate::session::peers::Inbox>,
    /// 门铃绑在哪：放下时（锁放开以前）删掉套接字文件。没绑上的没有。
    bell: Option<crate::session::peers::Placed>,
    /// 门铃放在哪：交给「空了告诉我」敲别的窗口（[`attach`]）。
    places: Bell,
    /// 等收件目录的任务：actor 退出（`Window` 放下）时停，套接字跟着关。
    listening: Option<tokio::task::AbortHandle>,
    /// 「空了告诉我」的两边（L7 三）：交给 actor 时才有；放下时名单上在等的各回一封「关了」。
    peers: Option<crate::session::peers::Peers>,
    /// 子代理（连孙代理）一共用掉的（会话表里那一份，`table.rs` 的 [`AgentsSpent`]）：交给 actor 时接上，写概要时照它记
    /// （`summary.rs` 的 `agents`）。没有会话表的没有。
    agents: Option<AgentsSpent>,
    /// 放在锁后面：先放锁（`claim`），再让这个进程里等着开它的醒（`local.rs`）。
    entry: local::Entry,
}

impl Window {
    /// 在会话目录 `dir` 里拿锁，接着概要记（有概要照概要，没有的照日志算），照 `places` 绑门铃。别的窗口开着的交回
    /// [`Claimed::Elsewhere`]。碰磁盘，在阻塞线程里调；载入的会话在读日志之前调，两个窗口不会同时读、同时写一份日志。
    pub(crate) fn claim(dir: PathBuf, places: Bell) -> Result<Window, Claimed> {
        let claim = Claim::take(&dir)?;
        // 子会话（界面直接开的）照日志算出来没有概要：锁照拿，概要不写，列会话不列它。还没有概要文件的，第一轮结束时写一次。
        let tracker = match summary::read(&dir) {
            // 引擎 十以前写的概要没有预览：照日志补上一次（只有这种旧的多读一遍日志），会话结束时连它一起写回，之后不再补。
            Some(mut summary) => {
                if summary.preview.is_none() {
                    summary.preview = summary::from_log(&dir)
                        .and_then(|logged| logged.preview)
                        .or(Some(String::new()));
                }
                Some(Tracker::new(summary, true))
            }
            None => summary::from_log(&dir).map(|summary| Tracker::new(summary, false)),
        };
        let (inbox, bell) = match crate::session::peers::watch(&dir, &places) {
            Ok((inbox, bell)) => (Some(inbox), bell),
            Err(_) => (None, None),
        };
        let entry = local::enter(dir.clone());
        Ok(Window {
            dir,
            claim,
            tracker,
            inbox,
            bell,
            places,
            listening: None,
            peers: None,
            agents: None,
            entry,
        })
    }

    /// 「空了告诉我」的两边（L7 三）。
    pub(crate) fn peers(&mut self) -> Option<&mut crate::session::peers::Peers> {
        self.peers.as_mut()
    }

    /// 回合在不在进行（每批送完，`actor/listing.rs`）。
    pub(crate) fn set_busy(&mut self, busy: bool) {
        self.claim.set_busy(busy);
    }

    /// 一批事件写进了日志：概要跟着记；标题变了、还没有概要文件的一轮结束了，在阻塞线程里写一次（不等它）。一轮结束时子代理
    /// 用掉的比概要里记的多了的也写一次（只有派过子代理的那几轮）：程序被杀时丢的只有最后那一段。
    pub(crate) fn saw(&mut self, events: &[Event]) {
        let Some(tracker) = &mut self.tracker else {
            return;
        };
        let mut write = tracker.saw(events);
        let ended = events
            .iter()
            .any(|event| matches!(event.body, Body::TurnEnded(_)));
        if let Some(agents) = &self.agents
            && (write || ended)
        {
            write |= tracker.set_agents(spent(agents)) && ended;
        }
        if write {
            let (dir, summary) = (self.dir.clone(), tracker.summary().clone());
            drop(tokio::task::spawn_blocking(move || store(&dir, &summary)));
        }
    }
}

/// 子代理这时一共用掉的。
fn spent(agents: &AgentsSpent) -> Option<crate::session::current::Carried> {
    agents
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .carried()
}

/// 会话目录 `dir` 这一刻有没有窗口开着（L7 三，等它空下来时认「没开着」）。碰磁盘，在阻塞线程里调。
pub(crate) fn open_elsewhere(dir: &std::path::Path) -> bool {
    lock::held_by_other(dir, lock::LOCK)
}

/// 造会话、载入的入口先拿锁（在阻塞线程里），门铃照 `places` 绑：别的窗口开着的交回 `Err`；锁文件开不了的（数据目录只读这类）
/// 不挡着开会话，记一行，没有占有。
pub(crate) async fn claim(dir: PathBuf, places: Bell) -> Result<Option<Window>, Claimed> {
    // 这个进程里刚放下、actor 还在收尾的：等它放下锁（`local.rs`）。
    loop {
        match local::closing(&dir) {
            Err(()) => return Err(Claimed::Elsewhere),
            Ok(Some(mut released)) => {
                // 放下的那一头一断开就醒：通道里从不发东西。
                drop(released.changed().await);
            }
            Ok(None) => break,
        }
    }
    match crate::session::blocking::blocking(move || Window::claim(dir, places)).await {
        Ok(window) => Ok(Some(window)),
        Err(Claimed::Elsewhere) => Err(Claimed::Elsewhere),
        Err(Claimed::Disk(_)) => Ok(None),
    }
}

/// 造好、载入好了：把占有交给 `handle` 的 actor 拿着，起等收件目录的任务（L7 二）。任务交的在占有之后进 actor 的收件箱。
pub(crate) fn attach(handle: &crate::session::Handle, window: Option<Window>) {
    if let Some(mut window) = window {
        window.entry.opened(handle.downgrade());
        if let Some(inbox) = window.inbox.take() {
            window.listening = Some(crate::session::peers::listen(
                inbox,
                window.dir.clone(),
                handle.id().clone(),
                handle.downgrade(),
            ));
        }
        window.peers = Some(crate::session::peers::Peers::new(
            &window.dir,
            handle.id().clone(),
            window.places.clone(),
        ));
        // 子代理一共用掉的：照概要里记的接着算（重开以前的那一份，`Handle::earlier_agents`）。
        if let Some(table) = handle.table() {
            let earlier = window
                .tracker
                .as_ref()
                .and_then(|tracker| tracker.summary().agents.as_ref());
            window.agents = Some(table.resume_agents(earlier));
        }
        handle.keep(window);
    }
}

/// actor 退出：写最后一次概要（一份小文件，当场写：程序可能正在退出，排进阻塞线程的未必轮得到），删掉门铃的套接字文件，锁
/// 跟着放下（字段在这之后才放：删套接字在放锁以前，下一个拿到锁的绑的不会被这里删掉）。会话被删了的写不进去，不记。
impl Drop for Window {
    fn drop(&mut self) {
        if let Some(listening) = &self.listening {
            listening.abort();
        }
        if let Some(tracker) = &mut self.tracker
            && self.dir.is_dir()
        {
            if let Some(agents) = &self.agents {
                tracker.set_agents(spent(agents));
            }
            store(&self.dir, tracker.summary());
        }
        if let Some(bell) = &self.bell {
            bell.release();
        }
    }
}

/// 写概要，写不进的记一行：丢了照日志现算。
fn store(dir: &std::path::Path, summary: &Summary) {
    drop(summary::write(dir, summary));
}

#[cfg(test)]
mod tests;
