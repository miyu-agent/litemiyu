//! 替身也照会话 actor 的样子歇着就收成槽、要看历史以前先从「磁盘」读回来放回（红线 5，`session/shelve.rs`）：内核的场景测试
//! 因此都走一遍收起、放回，照字断言的请求守着放回来的和没收过的一样。读回的每一条照日志的一行过一遍（`to_line`、`from_line`），
//! 和执行器从真日志读回的一样。
//!
//! 只为对照不收的（[`Stage::hold_history`]）：两个替身照同一份剧本跑，比交出来的请求。

use super::stage::Stage;
use crate::kernel::event::Event;
use crate::kernel::session::{Input, Session};

impl Stage {
    /// 以后不收成槽：收着的先放回来，有效历史从此一直在内存里，同红线 5 以前的样子。对照收成槽的用。
    pub fn hold_history(&mut self) {
        self.unshelve();
        self.shelving = false;
    }

    /// 收起来了几次（测试看：真的收了）。
    pub fn shelvings(&self) -> usize {
        self.shelvings
    }

    /// 收着呢（测试看：歇着以后收了没有）。
    pub fn shelved(&self) -> bool {
        self.session.shelved().is_some()
    }

    /// 要送的这一条要看历史的：收着就先放回来（[`Stage::unshelve`]）。
    pub(super) fn put_back(&mut self, input: &Input) {
        if Session::needs_history(input) {
            self.unshelve();
        }
    }

    /// 收着的：从「磁盘」读回来放回，和收起来以前的比一遍，一样才算（收着时又追加过的不比）。读回的少了要放回的那一条、放回的和
    /// 收起来以前的不一样，都是内核的 bug。
    pub(super) fn unshelve(&mut self) {
        let Some(from) = self.session.shelved() else {
            return;
        };
        let read: Vec<Event> = self
            .log
            .iter()
            .filter(|event| event.seq >= from)
            .map(|event| {
                Event::from_line(&event.to_line())
                    .unwrap_or_else(|e| panic!("日志的一行读不回来：{e}"))
            })
            .collect();
        self.session
            .unshelve(&read)
            .unwrap_or_else(|e| panic!("放不回来：{e}"));
        if let Some(before) = self.shelved_from.take() {
            assert!(
                *self.session.history() == before,
                "放回来的有效历史和收起来以前的不一样"
            );
        }
    }

    /// 跑到没事可做了：歇着的收成槽（不收的替身不收）。
    pub(super) fn rest(&mut self) {
        if !self.shelving || self.session.shelved().is_some() {
            return;
        }
        let before = self.session.history().clone();
        if self.session.shelve().is_some() {
            self.shelved_from = Some(before);
            self.shelvings += 1;
        }
    }
}
