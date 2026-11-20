//! 这个进程里开着的主会话（L7 一）：界面放下一个会话的把手以后，它的 actor 还要把最后几条写完才退出、放下锁。这时同一个进程
//! 再开它（切走又切回来）不该被当成别的窗口开着：照这张表认出是自己这里正在关的，等它放下再拿锁。把手还有人拿着的（同一个
//! 进程里真开着两份）照别的窗口开着答。
//!
//! 表在进程里只有一张，用到才有东西：一个开着的主会话一项（会话目录、它的弱把手、放下时断开的通道）。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use tokio::sync::watch;

use crate::session::handle::WeakHandle;

/// 开着的一项：弱把手（交给 actor 以前没有），和放下时断开的那一头。
struct Open {
    handle: Option<WeakHandle>,
    released: watch::Receiver<()>,
}

static OPEN: Mutex<BTreeMap<PathBuf, Open>> = Mutex::new(BTreeMap::new());

/// 拿着这一项的那一头：放下（窗口放下锁以后）就从表里拿掉，等着的跟着醒。
#[derive(Debug)]
pub(crate) struct Entry {
    dir: PathBuf,
    _released: watch::Sender<()>,
}

/// 这个进程里要不要等：会话目录 `dir` 正在关的，交回等它放下的那一头；真开着的交回 `Err`；没有的交回 `Ok(None)`。
pub(crate) fn closing(dir: &Path) -> Result<Option<watch::Receiver<()>>, ()> {
    let table = OPEN.lock().unwrap_or_else(PoisonError::into_inner);
    match table.get(dir) {
        None => Ok(None),
        Some(open) => match open.handle.as_ref().and_then(WeakHandle::upgrade) {
            None if open.handle.is_some() => Ok(Some(open.released.clone())),
            _ => Err(()),
        },
    }
}

/// 拿到锁了：记进表里。
pub(crate) fn enter(dir: PathBuf) -> Entry {
    let (released, receiver) = watch::channel(());
    OPEN.lock().unwrap_or_else(PoisonError::into_inner).insert(
        dir.clone(),
        Open {
            handle: None,
            released: receiver,
        },
    );
    Entry {
        dir,
        _released: released,
    }
}

impl Entry {
    /// 交给 actor 了：记下它的弱把手，照它认是不是正在关。
    pub(crate) fn opened(&self, handle: WeakHandle) {
        if let Some(open) = OPEN
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get_mut(&self.dir)
        {
            open.handle = Some(handle);
        }
    }
}

impl Drop for Entry {
    fn drop(&mut self) {
        OPEN.lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&self.dir);
    }
}
