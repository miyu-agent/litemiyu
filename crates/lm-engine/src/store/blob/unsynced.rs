//! 存了、还没同步的 blob（速度线三）：会话的工具存改前改后时不当场同步（[`super::Blobs::put_unsynced`]），
//! 登记在这里；后台同步每一次先同步登记了的、再同步日志，要写日志的那一刻也先把它引用到的同步掉。

use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

use crate::store::durable::sync_dir;

/// 存了、还没同步的 blob（速度线三，[`Blobs::put_unsynced`]）：一个会话的工具存进来，后台同步（或者要写日志的那一刻）照
/// 先后同步掉。引用它们的事件要等它们同步了才写进日志：断电以后日志里有的引用，blob 一定在。
#[derive(Debug, Default)]
pub(crate) struct Unsynced {
    /// 登记的编号和文件，照先后。
    waiting: Vec<(u64, PathBuf)>,
    /// 下一个编号。
    next: u64,
}

impl Unsynced {
    /// 登记一个存了没同步的文件。
    pub(crate) fn add(&mut self, path: PathBuf) {
        self.waiting.push((self.next, path));
        self.next += 1;
    }

    /// 最后登记的那一个的编号：这一刻要写的事件引用的 blob 都在它以前（含）。一个都没登记过的是 `None`。
    pub(crate) fn latest(&self) -> Option<u64> {
        self.next.checked_sub(1)
    }

    /// 编号到 `upto`（含）的还有没同步的吗。
    pub(crate) fn waiting(&self, upto: u64) -> bool {
        self.waiting.first().is_some_and(|(id, _)| *id <= upto)
    }

    /// 编号到 `upto`（含）、还没同步的：其中最后一个的编号和它们的文件。没有的是 `None`。
    pub(crate) fn upto(&self, upto: u64) -> Option<(u64, Vec<PathBuf>)> {
        let taken: Vec<&(u64, PathBuf)> = self
            .waiting
            .iter()
            .take_while(|(id, _)| *id <= upto)
            .collect();
        let (last, _) = taken.last()?;
        Some((*last, taken.iter().map(|(_, path)| path.clone()).collect()))
    }

    /// 编号到 `upto`（含）的都同步了，拿掉。
    pub(crate) fn synced(&mut self, upto: u64) {
        self.waiting.retain(|(id, _)| *id > upto);
    }
}

/// 同步这几个 blob（速度线三）：每个文件的数据，再每个目录一次（改名落盘）。
///
/// # Errors
///
/// 打不开、同步不了。
pub(crate) fn sync_files(paths: &[PathBuf]) -> io::Result<()> {
    let mut dirs: Vec<&Path> = Vec::new();
    for path in paths {
        File::open(path)?.sync_data()?;
        if let Some(dir) = path.parent()
            && !dirs.contains(&dir)
        {
            dirs.push(dir);
        }
    }
    for dir in dirs {
        sync_dir(dir)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
