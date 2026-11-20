//! 一条后台命令的输出文件（`docs/blueprint/store.md` 的 `jobs/<编号>.out`）：读的线程一段段写进去，不截；报了结束就关上，
//! 之后读到的不再写（还拿着管道的孙进程）。结束时整份存成 blob，数出有多少个字。每写进一段、关上时叫一声（施工 V-6 上，
//! 会话的任务那一层的信号）：头带 `wait` 读 `job.output` 的等着它。
//!
//! 应急模式（设计第五节第 16 条，blob 只在内存里的会话）不建输出文件：写进来的留在内存里（[`Output::in_memory`]），上限照
//! `limits.rs` 的 [`super::limits::MEMORY_OUTPUT_LIMIT`]，过了照 `too_large` 停掉，同落盘的过了 5 GB。

use std::collections::VecDeque;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

use crate::store::blob::Blobs;
use lm_kernel::kernel::id::ContentHash;

/// 一条后台命令的输出文件。
pub(super) struct Output {
    path: PathBuf,
    /// 写的一头；关上了、写不进去了的是空的。
    file: Mutex<Option<File>>,
    /// 写进去了多少字节（施工 7-12 下）：过了上限就停掉这条命令。
    written: AtomicU64,
    /// 只在内存里的（应急模式）：写进来的字节，和还开没开着。落盘的没有。
    kept: Option<Mutex<(Vec<u8>, bool)>>,
}

impl Output {
    /// 在 `path` 上开好的 `file`。
    pub(super) fn new(path: PathBuf, file: File) -> Output {
        Output {
            path,
            file: Mutex::new(Some(file)),
            written: AtomicU64::new(0),
            kept: None,
        }
    }

    /// 只在内存里的一份（应急模式）：不建文件，写进来的留在内存里。
    pub(super) fn in_memory() -> Output {
        Output {
            path: PathBuf::new(),
            file: Mutex::new(None),
            written: AtomicU64::new(0),
            kept: Some(Mutex::new((Vec::new(), true))),
        }
    }

    /// 只在内存里的、到这时写进来的一份（跑着时读输出用）；落盘的没有。
    pub(super) fn so_far(&self) -> Option<Vec<u8>> {
        self.kept.as_ref().map(|kept| held(kept).0.clone())
    }

    /// 写一段。关上了的不写；写不进去的（磁盘满了之类）记一行，关上，之后的都不写：命令照跑，读的线程照读，不让它
    /// 卡在写满的管道上。
    pub(super) fn write(&self, text: &str) {
        if let Some(kept) = &self.kept {
            let mut kept = held(kept);
            if kept.1 {
                kept.0.extend_from_slice(text.as_bytes());
                self.written.fetch_add(text.len() as u64, Ordering::Relaxed);
            }
            return;
        }
        let mut file = self.lock();
        let Some(open) = file.as_mut() else {
            return;
        };
        match open.write_all(text.as_bytes()) {
            Ok(()) => {
                self.written.fetch_add(text.len() as u64, Ordering::Relaxed);
            }
            Err(_) => {
                *file = None;
            }
        }
    }

    /// 写进去了多少字节。
    pub(super) fn written(&self) -> u64 {
        self.written.load(Ordering::Relaxed)
    }

    /// 关上：之后读到的不再写。
    pub(super) fn close(&self) {
        *self.lock() = None;
        if let Some(kept) = &self.kept {
            held(kept).1 = false;
        }
    }

    /// 整份存成 blob，交回它的哈希、有多少个字（Unicode 字符）、最后一段（施工 7-13，[`TAIL`]）。读不出来、存不下来的
    /// 记一行，三样都没有：回报里不写输出，她也就不会去读一份不在的。碰磁盘，在阻塞线程里调。
    pub(super) fn stored(&self, blobs: &Blobs) -> Stored {
        if let Some(kept) = &self.kept {
            let content = std::mem::take(&mut held(kept).0);
            return kept_stored(blobs, content);
        }
        // 边读边存（施工 7-12 下）：输出最多 5 GB，不整份读进内存；最后一段只留末尾够用的字节。
        let mut chars = 0;
        let mut end: VecDeque<u8> = VecDeque::new();
        let stored = blobs.put_file(&self.path, |bytes| {
            chars += count(bytes);
            end.extend(bytes);
            let over = end.len().saturating_sub(TAIL_BYTES);
            end.drain(..over);
        });
        match stored {
            Ok(hash) => Stored {
                output: Some(hash),
                chars: Some(chars),
                tail: (chars > 0).then(|| tail_of(end.make_contiguous(), TAIL)),
            },
            Err(_) => Stored::default(),
        }
    }

    fn lock(&self) -> MutexGuard<'_, Option<File>> {
        self.file.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// 拿着内存里的那一份：拿锁的一方 panic 过的照样用。
fn held(kept: &Mutex<(Vec<u8>, bool)>) -> MutexGuard<'_, (Vec<u8>, bool)> {
    kept.lock().unwrap_or_else(PoisonError::into_inner)
}

/// 内存里的输出整份存成（内存里的）blob，交回同 [`Output::stored`] 的三样：不再拷一份。
fn kept_stored(blobs: &Blobs, content: Vec<u8>) -> Stored {
    let chars = count(&content);
    let tail = (chars > 0).then(|| {
        let from = content.len().saturating_sub(TAIL_BYTES);
        tail_of(&content[from..], TAIL)
    });
    match blobs.put_owned(content) {
        Ok(hash) => Stored {
            output: Some(hash),
            chars: Some(chars),
            tail,
        },
        Err(_) => Stored::default(),
    }
}

/// 叫一声：计数加一，等着的醒来再看一眼。
/// 回报里带的最后一段最多几个字（施工 7-13，2026-10-20 项目主人提的：输出不长就直接带上）。
pub(crate) const TAIL: usize = 2000;

/// 留末尾多少字节就够 [`TAIL`] 个字：一个字最多 4 个字节，再多留一点好切在行首。
const TAIL_BYTES: usize = TAIL * 4 + 4;

/// 存好的输出：blob、字数、最后一段。
#[derive(Debug, Default)]
pub(super) struct Stored {
    pub(super) output: Option<ContentHash>,
    pub(super) chars: Option<u64>,
    pub(super) tail: Option<String>,
}

/// 末尾的 `end` 字节里最后 `most` 个字：开头切在一个字中间的照 `\u{FFFD}` 解，再去掉；不是从一行开头起的，从下一行起
/// （下一行在后一半里才这样切，不然照字切）。
pub(crate) fn tail_of(end: &[u8], most: usize) -> String {
    let text = String::from_utf8_lossy(end);
    let total = text.chars().count();
    if total <= most {
        return text.into_owned();
    }
    let start = text
        .char_indices()
        .nth(total - most)
        .map_or(text.len(), |(at, _)| at);
    let part = &text[start..];
    if text[..start].ends_with('\n') {
        return part.to_string();
    }
    match part.find('\n') {
        Some(at) if at < part.len() / 2 => part[at + 1..].to_string(),
        _ => part.to_string(),
    }
}

/// 有多少个字：写进去的都是解好的 UTF-8，不是接续字节的每个字节算一个字，和前台的数法一样。
fn count(bytes: &[u8]) -> u64 {
    bytes.iter().filter(|&&byte| byte & 0xC0 != 0x80).count() as u64
}
