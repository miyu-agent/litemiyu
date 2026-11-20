//! 只在内存里的日志（应急模式，设计第五节第 16 条）：`litemiyu --rescue` 的会话不写会话日志，追加的事件留在这一份里，撤掉压缩
//! 时读回、撤销后重算看过的文件、订阅时补发都照它读；会话关了就没了。写的那一头（[`Store`]）和读的那一头（[`ReadLog`]）共用
//! 一份。
//!
//! 留的是整份日志，压缩替代掉的也在：撤销能撤到任何一轮（撤到还算数的压缩时从那以前最近一次压缩替代到的下一条读回，一次都
//! 没有的从第 1 条读，`lm_kernel` 的 `Ledger::read_back_from`），哪一截都可能要读回，放不掉；同落盘的日志一样只进不出，会话关了
//! 才没。歇着时内核的有效历史照样收成槽（`actor/shelf.rs`），读回从这一份拷。

use std::io;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use lm_kernel::kernel::event::Event;
use lm_kernel::kernel::id::Seq;

use super::Store;
use crate::tool::ReadLog;

/// 追加过的事件，照先后。
#[derive(Debug, Clone, Default)]
pub(crate) struct MemoryLog(Arc<Mutex<Vec<Event>>>);

impl MemoryLog {
    fn lock(&self) -> MutexGuard<'_, Vec<Event>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Store for MemoryLog {
    /// 留下这一批：内存里的没有「落盘」，交回时就算落了。
    fn append(&mut self, events: &[Event]) -> io::Result<()> {
        self.lock().extend_from_slice(events);
        Ok(())
    }

    fn events(&self) -> Result<Vec<Event>, String> {
        Ok(self.lock().clone())
    }

    fn events_from(&self, from: Seq) -> Result<Vec<Event>, String> {
        let events = self.lock();
        let start = events.partition_point(|event| event.seq < from);
        Ok(events[start..].to_vec())
    }
}

impl ReadLog for MemoryLog {
    /// 整份当一段交出去。
    fn read(&self, each: &mut dyn FnMut(Vec<Event>) -> bool) -> Result<(), String> {
        let events = self.lock().clone();
        if !events.is_empty() {
            each(events);
        }
        Ok(())
    }
}
