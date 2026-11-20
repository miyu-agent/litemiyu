//! 订阅时要补发的那一截（施工 3-8 六补，`docs/blueprint/session/actor.md` 第 6 条，`04-核心协议.md` 第七节）：日志里序号
//! 大于 `after`、到订阅那一刻落了盘的最后一条为止的事件。
//!
//! 补到哪一条（`upto`）和订阅在 actor 的同一步里拿：那一步之前落了盘的都推过了，之后的都还没推，所以补的和之后推的
//! 接得上，不重不漏。读日志不在 actor 里：交回一个 [`Backlog`]，拿着它的一方（协议端点）另起阻塞线程读，会话照常跑。
//! `upto` 以前的都落了盘，日志只往后追加，什么时候读都一样；读的时候多出来的不要，它们从订阅推过去。
//!
//! 照序号读（施工 V-37 下）：从第 `after + 1` 条定位着读起，读到 `upto` 那一块就停。原来一段段从头读到最后一段的末尾，
//! `after` 以前的整个解完再扔，`upto` 以后订阅以来新写的也读、也解。

use crate::tool::Log;
use lm_kernel::kernel::event::Event;
use lm_kernel::kernel::id::Seq;

use crate::session::blocking::{blocking, in_own_thread};

/// 补发的那一截：序号在 `after` 之后、`upto` 为止（含）的事件，还没读。
#[derive(Debug)]
pub struct Backlog {
    log: Log,
    after: u64,
    upto: u64,
}

impl Backlog {
    pub(crate) fn new(log: Log, after: u64, upto: u64) -> Backlog {
        Backlog { log, after, upto }
    }

    /// 只读地看一个载入不了的会话（LiteMiyu 界面线第十八趟）：日志中间坏了一行（[`super::LoadError::broken_at`]），会话不载入、
    /// 不拿锁，坏行以前的照样翻得到。`dir` 是会话的目录，从第 1 条到第 `upto` 条（坏行的前一条）；读的同 [`Backlog::slice`]
    /// 那一截一样只读，一个字节都不写。
    pub fn broken(dir: std::path::PathBuf, upto: u64) -> Backlog {
        Backlog::new(Log::new(super::store::LogDir(dir)), 0, upto)
    }

    /// 补到哪一条：订阅那一刻落了盘的最后一条，也就是订阅推过来的第一条的前一条。它比 `after` 小的，什么都不补。
    pub fn upto(&self) -> u64 {
        self.upto
    }

    /// 在阻塞线程里只读地读出这一截，照先后。一次读完再交回（施工 3-8 六补定：载入会话本来就整份读进内存，不分批）。
    /// 没有要补的（`after` 不比 `upto` 小），不读日志。
    ///
    /// 从第 `after + 1` 条定位着读，读到 `upto` 就停（施工 V-37 下，日志的只读入口的 `read_range`）：`after` 以前的只数
    /// 换行、不解、不自检，`upto` 以后的不读（2026-11-03 项目主人定）。
    ///
    /// # Errors
    ///
    /// 日志读不了、坏了：原因。会话在这时被删了（目录挪走了）也在这里。
    pub async fn read(self) -> Result<Vec<Event>, String> {
        self.gather(|events, kept: &mut Vec<Event>| {
            if kept.is_empty() {
                *kept = events;
            } else {
                kept.extend(events);
            }
        })
        .await
    }

    /// 同一份日志的另一截：序号在 `after` 之后、`upto` 为止（含）。界面读回放掉的一段正文用（红线 5，LiteMiyu 界面线
    /// 第十一趟）：日志只往后追加，落了盘的什么时候读都一样。
    pub fn slice(&self, after: u64, upto: u64) -> Backlog {
        Backlog {
            log: self.log.clone(),
            after,
            upto,
        }
    }

    /// 读出这一截交给 `then`，交回它的结果：读和 `then` 都在单起、做完就退的线程里（同载入的 `blocking_once`），解日志的
    /// 线程缓存跟着线程退回去，常驻的阻塞线程不留它们（内核线第三趟乙）。没有要补的照空的交给 `then`。
    ///
    /// # Errors
    ///
    /// 同 [`Backlog::read`]。
    pub async fn read_then<T: Send + 'static>(
        self,
        then: impl FnOnce(Vec<Event>) -> T + Send + 'static,
    ) -> Result<T, String> {
        let Some((log, from, upto)) = self.range() else {
            return Ok(then(Vec::new()));
        };
        blocking(move || {
            in_own_thread("lm-readback", move || {
                let mut kept = Vec::new();
                log.read_range(from, upto, |events| {
                    kept.extend(events);
                    true
                })?;
                Ok(then(kept))
            })
        })
        .await
    }

    /// 读出这一截，一块块当场交给 `each` 收进 `state`，读完交回 `state`：读和 `each` 都在单起、做完就退的线程里（同
    /// [`Backlog::read_then`]），不攒整份事件。界面列整份你说的话、打开旧会话时边读边交用（LiteMiyu 界面线第十二趟）。
    /// 没有要补的不读，照原样交回 `state`。
    ///
    /// # Errors
    ///
    /// 同 [`Backlog::read`]。读坏了以前交给 `each` 的照样交过了。
    pub async fn fold<T: Send + 'static>(
        self,
        mut state: T,
        mut each: impl FnMut(&mut T, Vec<Event>) + Send + 'static,
    ) -> Result<T, String> {
        let Some((log, from, upto)) = self.range() else {
            return Ok(state);
        };
        blocking(move || {
            in_own_thread("lm-readback", move || {
                log.read_range(from, upto, |events| {
                    each(&mut state, events);
                    true
                })?;
                Ok(state)
            })
        })
        .await
    }

    /// 在阻塞线程里读这一截，一块块交给 `take` 收进交回的那一份。没有要补的不读。
    async fn gather<T: Send + 'static>(
        self,
        mut take: impl FnMut(Vec<Event>, &mut Vec<T>) + Send + 'static,
    ) -> Result<Vec<T>, String> {
        let Some((log, from, upto)) = self.range() else {
            return Ok(Vec::new());
        };
        blocking(move || {
            let mut kept = Vec::new();
            log.read_range(from, upto, |events| {
                take(events, &mut kept);
                true
            })?;
            Ok(kept)
        })
        .await
    }

    /// 日志的只读入口、从第几条读到第几条：没有要补的（`after` 不比 `upto` 小）没有。
    fn range(self) -> Option<(Log, Seq, Seq)> {
        let Backlog { log, after, upto } = self;
        if after >= upto {
            return None;
        }
        Some((log, Seq::new(after + 1)?, Seq::new(upto)?))
    }
}

#[cfg(test)]
mod tests;
