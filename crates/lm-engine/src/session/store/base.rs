//! 分叉出来的会话的底子（LM-18，照 Miyu `miyu-session/src/store.rs` 的 `Base` 和 `Indexed::on_base`）：fork 子代理的日志从
//! 分叉点的下一条起，分叉点以前的历史在父会话的日志里。底子只记在哪（父会话的日志目录、到第几条为止），不常驻底子里的事件：
//! 要用的时候现读、读到分叉点就停。父会话的日志只往后追加，分叉点以前的不会变，什么时候读都一样；父会话后来压缩、撤销记的
//! 都在分叉点以后，读不到。
//!
//! 读回日志（撤掉压缩时、闲下来收成槽以后放回来，`actor/shelf.rs`）、重算她看过的文件，都先接底子再接自己的（[`Based`]）。

use std::io;
use std::path::PathBuf;

use crate::store::log::{Flow, OpenError, SessionLog, read_range, read_range_only};
use crate::tool::Seen;
use lm_kernel::kernel::event::Event;
use lm_kernel::kernel::id::Seq;

use super::{Flush, Store};
use crate::session::effects::{SEEN_KINDS, SeenScan};

/// 底子在哪：父会话的日志目录，到分叉点第几条为止（连同它）。
#[derive(Debug, Clone)]
pub(crate) struct Base {
    /// 父会话的日志目录。
    dir: PathBuf,
    /// 分叉点。
    upto: Seq,
}

impl Base {
    /// 父会话的日志在 `dir`，到第 `upto` 条为止。
    pub(crate) fn new(dir: PathBuf, upto: Seq) -> Base {
        Base { dir, upto }
    }

    /// 只读地读第 `from` 条起、到分叉点为止的（父会话可能正在写）：`from` 已经过了分叉点的一条都不要，不读。
    ///
    /// # Errors
    ///
    /// 父会话的日志打不开、坏了；读到分叉点以前就没了的（父会话被删了、被截短了）报 [`OpenError::Missing`]：底子少一截，接上的
    /// 历史就是错的，不能当没有。
    pub(crate) fn events_from(&self, from: Seq) -> Result<Vec<Event>, OpenError> {
        let mut events = Vec::new();
        if from > self.upto {
            return Ok(events);
        }
        self.each(from, None, |read| {
            if events.is_empty() {
                events = read;
            } else {
                events.extend(read);
            }
        })?;
        Ok(events)
    }

    /// 照先后一块块交给 `each`，从第 `from` 条读到分叉点：`only` 不是空的只解它里面的种类。读到分叉点那一块就停，没读到的
    /// 报没有。
    ///
    /// # Errors
    ///
    /// 同 [`Base::events_from`]。
    pub(crate) fn each(
        &self,
        from: Seq,
        only: Option<&[&str]>,
        mut each: impl FnMut(Vec<Event>),
    ) -> Result<(), OpenError> {
        let mut reached = false;
        let sink = |events: Vec<Event>| {
            reached = events.last().is_some_and(|event| event.seq >= self.upto);
            each(events);
            Flow::Go
        };
        match only {
            Some(only) => read_range_only(&self.dir, from, Some(self.upto), only, sink)?,
            None => read_range(&self.dir, from, Some(self.upto), sink)?,
        }
        if !reached {
            return Err(OpenError::Missing(self.dir.clone()));
        }
        Ok(())
    }
}

/// 分叉出来的会话的日志（同 Miyu 的 `Indexed::on_base`）：写的照它自己的日志写，读回的先接底子。
pub(crate) struct Based {
    /// 它自己的日志。
    pub(crate) log: SessionLog,
    /// 底子在哪。
    pub(crate) base: Base,
}

impl Store for Based {
    fn append(&mut self, events: &[Event]) -> io::Result<()> {
        Store::append(&mut self.log, events)
    }

    fn write(&mut self, events: &[Event], sync: bool) -> io::Result<()> {
        Store::write(&mut self.log, events, sync)
    }

    fn background(&self) -> bool {
        Store::background(&self.log)
    }

    fn sync_handle(&self) -> io::Result<std::sync::Arc<dyn Flush>> {
        Store::sync_handle(&self.log)
    }

    fn events(&self) -> Result<Vec<Event>, String> {
        self.events_from(Seq::FIRST)
    }

    fn events_from(&self, from: Seq) -> Result<Vec<Event>, String> {
        let mut events = self
            .base
            .events_from(from)
            .map_err(|error| error.to_string())?;
        // 自己的日志从分叉点的下一条起：从更早的读起的照它的第一条读（读回定位从末尾往回数，不认它前面的序号）。
        events.extend(Store::events_from(
            &self.log,
            from.max(self.base.upto.next()),
        )?);
        Ok(events)
    }

    /// 先记底子（读到分叉点为止，只解看过的文件要的种类），再记自己的；都边读边记。
    fn seen(&self) -> Result<Seen, String> {
        let mut scan = SeenScan::default();
        self.base
            .each(Seq::FIRST, Some(SEEN_KINDS), |events| {
                for event in &events {
                    scan.note(event);
                }
            })
            .map_err(|error| error.to_string())?;
        super::scan_own(self.log.dir(), &mut scan).map_err(|error| error.to_string())?;
        Ok(scan.seen())
    }
}
