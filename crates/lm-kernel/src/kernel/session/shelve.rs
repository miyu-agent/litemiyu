//! 闲下来收成槽、要用时放回来（红线 5，内核线第三趟）：会话歇着（[`Session::resting`]）的时候执行器叫它收
//! （[`Session::shelve`]），有效历史里的整条事件放掉、只记序号（`history/shelf.rs`）；下一条要看历史的输入送进来以前，执行器照
//! [`Session::shelved`] 从日志读回那一截交进来（[`Session::unshelve`]），放回的和收起来以前一条不差。内核不读盘：读的是执行器。
//!
//! 收着的时候还要答的几样查询：有没有能压的（[`Session::compactable`]）收的时候算好记下，收着时历史、账本、限额都不会变（变它们
//! 的输入都要先放回来）；派出去的任务本来就不收。读不回来的那段时间（执行器照旧送了输入）投影看到的是空的历史，执行器
//! 一律不发请求（内核线第三趟「读回失败」）。

use super::Session;
use super::input::Input;
use crate::kernel::event::Event;
use crate::kernel::history::Missing;
use crate::kernel::id::Seq;

impl Session {
    /// 歇着：空闲（[`Session::idle`]），追加的都落了盘，没有等落盘才回应的命令，回顾、起标题、提前压、替它看图都不在路上。执行器
    /// 照它在一批做完以后收成槽、放掉驱动的备忘录：这时收了，下一条输入多半是人下一句话，不会一收一放来回读盘。
    pub fn resting(&self) -> bool {
        self.idle()
            && self.unstored.is_empty()
            && self.waiting.is_empty()
            && self.titling.is_none()
            && !self.prepare.asking()
            && !self.sight.looking()
    }

    /// 收成槽：歇着的才收，有效历史里的整条事件拿出来交回、只记序号；有没有能压的先算好记下。交回的由执行器放（在哪个线程放
    /// 由它定：`history/shelf.rs` 说了为什么）。不在歇着、已经收着、分叉出来的底子还在有效历史里（在父会话的日志里，读不回）的
    /// 不收，交回没有。
    pub fn shelve(&mut self) -> Option<Vec<Event>> {
        if !self.resting() || self.shelved().is_some() {
            return None;
        }
        let compactable = self.compactable();
        let taken = self.history.shelve(self.ledger.first())?;
        self.rested = Some(compactable);
        Some(taken)
    }

    /// 收着的：要从日志的第几条读起，读到末尾，交给 [`Session::unshelve`]。没收着的没有。
    pub fn shelved(&self) -> Option<Seq> {
        self.history.shelved(self.ledger.next_seq())
    }

    /// 放回来：`events` 是日志里从 [`Session::shelved`] 那一条起、一条接一条读到末尾的事件。放回的是在调的线程里拷的一份，
    /// `events` 照旧归调的一方放（`history/shelf.rs`）。没收着的什么都不做。
    ///
    /// # Errors
    ///
    /// 读回的里面少了要放回的那一条（日志被截短了、读的时候被改短了）：什么都不动，照旧收着。
    pub fn unshelve(&mut self, events: &[Event]) -> Result<(), Missing> {
        self.history.unshelve(events)?;
        self.rested = None;
        Ok(())
    }

    /// 收着的时候能不能直接送这一条，不用先放回来：只改在不在看着、在哪个目录这两样的，不看历史。别的一律先放回来。
    pub fn needs_history(input: &Input) -> bool {
        !matches!(input, Input::Watched { .. } | Input::Environment(_))
    }

    /// 有效历史（替身对照用：收起来以前的和放回来的一样，`testkit/shelf.rs`）。
    #[cfg(any(test, feature = "testkit"))]
    pub(crate) fn history(&self) -> &crate::kernel::history::History {
        &self.history
    }

    /// 收的时候记下的「有没有能压的」；没收着的没有。
    pub(super) fn rested_compactable(&self) -> Option<bool> {
        self.rested
    }
}
