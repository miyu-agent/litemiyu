//! 闲下来不留整份会话（红线 5，内核线第三趟）：一批做完、会话歇着（`Session::resting`），内核把有效历史收成槽
//! （`Session::shelve`），端口放掉驱动的备忘录（[`crate::session::port::ModelPort::rest`]），拿出来的整条交给放完就退的线程放、
//! 空着的堆还给系统（`blocking.rs` 的 `release`）；下一条要看历史的输入送进内核以前，照内核说的从日志读回那一截、放回去
//! （[`Actor::unshelve`]，内核拷一份，读来的同样交出去放）。人说话以后的头一次请求因此整份重编一次；一个回合里照旧接着用编好的。
//!
//! 读回失败（日志坏了、被截短、读盘出错、读的时候被改短）：会话照旧跑，运行日志记一行 `history unreadable`；内核照旧收着，这时它
//! 看到的是空的历史，这里一个请求都不发（主请求、辅助请求、看图），当场照「没发出去」说完，原话写明读不回、为什么
//! （[`Actor::unreadable`]）：这一轮照没发出去收场，不会拼出少一段的请求。下一次闲着时来了要看历史的输入再读一次，读回来了照常。

use lm_kernel::kernel::event::{CallError, ErrorClass};
use lm_kernel::kernel::session::{Input, Session};

use super::{Actor, Stop};
use crate::session::blocking::release;

impl Actor {
    /// 要送 `input` 了：要看历史、内核收着的，先读回来放回去。读不回的照旧送（见模块的说明）；同一轮里读不回过的不再读，等闲着时。
    /// 读日志的线程 panic 了的停下（同读回日志，`store.rs`）。
    pub(super) async fn unshelve_for(&mut self, input: &Input) -> Result<(), Stop> {
        if !Session::needs_history(input) || self.session.shelved().is_none() {
            return Ok(());
        }
        if self.unread.is_some() && !self.session.idle() {
            return Ok(());
        }
        self.unshelve().await
    }

    /// 从日志读回内核要的那一截、放回去。
    pub(super) async fn unshelve(&mut self) -> Result<(), Stop> {
        let Some(from) = self.session.shelved() else {
            return Ok(());
        };
        let read = self.events_from(from).await?;
        let put = read.and_then(|events| {
            let put = self.session.unshelve(&events);
            // 内核拷了要的那几条，读来的整份是解日志的线程分配的：交给放完就退的线程放（`blocking.rs`）。
            release(events);
            put.map_err(|missing| missing.to_string())
        });
        match put {
            Ok(()) => self.unread = None,
            Err(why) => {
                self.unread = Some(why);
            }
        }
        Ok(())
    }

    /// 一批做完了：歇着的收成槽、放掉备忘录，收了的把空着的堆还给系统（一个回合一次，在阻塞线程里还，不挡下一条）。
    ///
    /// 应急模式的会话照样收：它的日志只在内存里（`session/store/memory.rs`），歇着时有效历史、备忘录各是日志里那一截的又一份，
    /// 收了只剩日志一份；下一次读回是从内存里那一份拷（界面线第九趟量过：四百轮带工具的长对话，不收时每轮多涨约 10 kB）。
    pub(super) fn rest(&mut self) {
        if !self.session.resting() {
            return;
        }
        self.model.rest();
        if let Some(taken) = self.session.shelve() {
            release(taken);
        }
    }

    /// 内核收着、历史读不回来的时候：请求一律不发，交回当场说完用的错（见模块的说明）。没收着的没有。
    pub(super) fn unreadable(&self) -> Option<CallError> {
        self.session.shelved()?;
        let why = self.unread.as_deref().unwrap_or("not read back yet");
        Some(CallError {
            class: ErrorClass::Unclassified,
            message: format!("这个会话的历史从日志读不回来，请求没有发出去：{why}"),
            status: None,
            retry_after_ms: None,
        })
    }
}
