//! 写盘的端口（施工 3-7 中「我定的」）：actor 追加的事件往哪里写。平时是会话日志；测试里换成写不进去
//! 的，查停下的那条路。不对外。撤销、恢复以后重算她看过的，从这里重读一遍日志（施工 4-7 上）；撤掉压缩时读回更早的
//! 一段，也从这里读（施工 6-9）。
//!
//! 交给 `history` 的日志只读入口也在这里（施工 6-4）：照会话的目录一段一段读。
//!
//! 平时写的是会话日志本身。Miyu 这里每落一批还顺手更新会话列表的索引、用量汇总、回合索引（SQLite，LiteMiyu 不带）。

use std::io;
use std::path::{Path, PathBuf};

use crate::store::log::{
    Flow, OpenError, SessionLog, read_events, read_range, read_range_ending, read_range_only,
    read_segments,
};
use crate::tool::{ReadLog, Seen};
use lm_kernel::kernel::event::Event;
use lm_kernel::kernel::id::Seq;

use crate::session::effects::{SEEN_KINDS, SeenScan};

/// 一次写一批。在阻塞线程里用，所以要能挪到别的线程上。
/// 能在别的线程里同步的句柄（施工 V-19，`session/actor.md` 第 5 条第 9 款）：会话日志正在写的那一段的另一个句柄。
pub(crate) trait Flush: Send + Sync {
    /// 同步：这个句柄所在的那一段，到这一刻写进去的都落了盘。
    ///
    /// # Errors
    ///
    /// 同步不了。
    fn flush(&self) -> io::Result<()>;
}

impl Flush for std::fs::File {
    fn flush(&self) -> io::Result<()> {
        self.sync_data()
    }
}

pub(crate) trait Store: Send + 'static {
    /// 追加一批，返回时这一批都落了盘。
    fn append(&mut self, events: &[Event]) -> io::Result<()>;

    /// 追加一批（施工 V-18，`session/actor.md` 第 5 条第 1 款）：`sync` 是真的同 [`Store::append`]；假的只交给系统、不同步，
    /// 程序崩了照样在，断电要等下一次同步的一批才保得住。不分这两种的（测试里的）一律照 `append`。
    fn write(&mut self, events: &[Event], sync: bool) -> io::Result<()> {
        let _ = sync;
        self.append(events)
    }

    /// 同步挪到后台做不做（施工 V-19）：做的，写的时候一律不同步，写完拿 [`Store::sync_handle`] 交给后台；不做的（测试里
    /// 不在磁盘上的）照旧同步地写，写完就算同步了。
    fn background(&self) -> bool {
        false
    }

    /// 后台同步用的句柄（施工 V-19）：写完一批再拿，换了段也是新的那一段。不做后台同步的没有。
    ///
    /// # Errors
    ///
    /// 给不出句柄；不做后台同步的一律报不支持。
    fn sync_handle(&self) -> io::Result<std::sync::Arc<dyn Flush>> {
        Err(io::ErrorKind::Unsupported.into())
    }

    /// 只读地读回整份日志。
    fn events(&self) -> Result<Vec<Event>, String>;

    /// 只读地读回第 `from` 条起的日志（施工 6-9）：只留要的。
    fn events_from(&self, from: Seq) -> Result<Vec<Event>, String>;

    /// 照日志重算她看过的文件（撤销、恢复落了盘以后，施工 4-7 上）：同 `effects::seen_in` 照整份日志算的。在磁盘上的边读边记
    /// （施工 V-41 中），不把整份日志攒在内存里；别的（测试里的）照 [`Store::events`] 整份读来算。
    fn seen(&self) -> Result<Seen, String> {
        self.events()
            .map(|events| crate::session::effects::seen_in(&events))
    }
}

impl Store for SessionLog {
    fn append(&mut self, events: &[Event]) -> io::Result<()> {
        SessionLog::append(self, events)
    }

    fn write(&mut self, events: &[Event], sync: bool) -> io::Result<()> {
        SessionLog::write(self, events)?;
        if sync {
            SessionLog::sync(self)?;
        }
        Ok(())
    }

    fn background(&self) -> bool {
        true
    }

    fn sync_handle(&self) -> io::Result<std::sync::Arc<dyn Flush>> {
        Ok(std::sync::Arc::new(SessionLog::sync_handle(self)?))
    }

    fn events(&self) -> Result<Vec<Event>, String> {
        read_events(self.dir()).map_err(|error| error.to_string())
    }

    /// 照序号定位着读（施工 V-37 下，`crate::store::log::read_range`）：`from` 以前的只数换行、不解，撤掉压缩时只解要读回的
    /// 那一截。原来一段段整个解完再挑。跳过的那一截不自检（2026-11-03 项目主人定）。写到哪了这里记着：离末尾近的从末尾往回数
    /// （占用线三，`read_range_ending`），收着的历史多半是最后那一小截。
    fn events_from(&self, from: Seq) -> Result<Vec<Event>, String> {
        let mut kept = Vec::new();
        read_range_ending(self.dir(), from, &self.mark(), |events| {
            gather(&mut kept, events);
            Flow::Go
        })
        .map_err(|error| error.to_string())?;
        Ok(kept)
    }

    /// 从第一条一块块读、边读边记（施工 V-41 中），只解看过的文件要的种类（`Event::from_line_only`），整份照旧自检。
    fn seen(&self) -> Result<Seen, String> {
        let mut scan = SeenScan::default();
        scan_own(SessionLog::dir(self), &mut scan).map_err(|error| error.to_string())?;
        Ok(scan.seen())
    }
}

/// 一块块收进一整份：头一块整个搬过来，不再拷一遍（同 `miyu_store::log` 里的，施工 V-14）。
fn gather(events: &mut Vec<Event>, read: Vec<Event>) {
    if events.is_empty() {
        *events = read;
    } else {
        events.extend(read);
    }
}

/// 会话自己的日志（在 `dir`）从第一条一块块读，交给 `scan` 记她看过的文件（施工 V-41 中）：只解看过的文件要的种类，从第一条读起的照
/// `read_events` 整份自检。
fn scan_own(dir: &Path, scan: &mut SeenScan) -> Result<(), OpenError> {
    read_range_only(dir, Seq::FIRST, None, SEEN_KINDS, |events| {
        for event in &events {
            scan.note(event);
        }
        Flow::Go
    })
}

/// 这个会话日志的只读入口（施工 6-4）：会话的目录。
pub(crate) struct LogDir(pub(crate) PathBuf);

impl ReadLog for LogDir {
    fn read(&self, each: &mut dyn FnMut(Vec<Event>) -> bool) -> Result<(), String> {
        read_segments(&self.0, each).map_err(|error| error.to_string())
    }

    /// 照序号定位着读（施工 V-37 下，`crate::store::log::read_range`）：`from` 以前的只数换行、不解、不自检（2026-11-03 项目
    /// 主人定），读过 `upto` 那一块就停，后面的段不打开。
    fn read_range(
        &self,
        from: Seq,
        upto: Seq,
        each: &mut dyn FnMut(Vec<Event>) -> bool,
    ) -> Result<(), String> {
        read_range(&self.0, from, Some(upto), |events| match each(events) {
            true => Flow::Go,
            false => Flow::Stop,
        })
        .map_err(|error| error.to_string())
    }

    /// 同上，只是每一行只解 `only` 里的种类（施工 V-49，`crate::store::log::read_range_only`）：`history` 翻记录用。
    fn read_range_only(
        &self,
        from: Seq,
        upto: Seq,
        only: &[&str],
        each: &mut dyn FnMut(Vec<Event>) -> bool,
    ) -> Result<(), String> {
        read_range_only(&self.0, from, Some(upto), only, |events| {
            match each(events) {
                true => Flow::Go,
                false => Flow::Stop,
            }
        })
        .map_err(|error| error.to_string())
    }
}

mod base;
mod memory;
pub(crate) use base::{Base, Based};
pub(crate) use memory::MemoryLog;

#[cfg(test)]
mod tests;
