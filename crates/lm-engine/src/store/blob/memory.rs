//! 只在内存里的 blob（应急模式，设计第五节第 16 条）：`litemiyu --rescue` 的会话什么都不写盘，图片、改前改后的原文、后台命令的
//! 输出、策略快照这些大内容存在这一张表里，会话关了就没了。按内容哈希存，同一份只留一份；存进来的就是算哈希的那一份，取的时候
//! 不再核对。
//!
//! 只有存、取、量大小、从某处读这几样：落盘的那几样（临时文件、同步、刷修改时间）这里根本不走。

use std::collections::HashMap;
use std::fs::File;
use std::io::{self, Cursor, Read};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use lm_kernel::kernel::id::{ContentHash, Hasher};

use super::{BlobError, Blobs};

impl Blobs {
    /// 只在内存里的一份（应急模式，设计第五节第 16 条）：存、取都在内存里，一个文件、一个目录都不建（`blob/memory.rs`）。
    pub fn in_memory() -> Blobs {
        Blobs {
            memory: Some(Held::default()),
            ..Blobs::new(PathBuf::new())
        }
    }

    /// 是不是只在内存里的（[`Blobs::in_memory`]）。
    pub(crate) fn is_in_memory(&self) -> bool {
        self.memory.is_some()
    }

    /// 存一份已经在手里的内容（后台命令在内存里的输出）：只在内存里的不再拷一份，别的照 [`Blobs::put`]。
    ///
    /// # Errors
    ///
    /// 同 [`Blobs::put`]。
    pub(crate) fn put_owned(&self, content: Vec<u8>) -> io::Result<ContentHash> {
        match &self.memory {
            Some(held) => Ok(held.put_owned(content)),
            None => self.put(&content),
        }
    }

    /// 从第 `offset` 个字节起读一个 blob 的一头（发请求体时照原样发的图片、PDF，`session/route/exchange.rs`）：只在内存里的
    /// 读那张表，别的照 [`Blobs::open_at`] 打开文件。
    ///
    /// # Errors
    ///
    /// 同 [`Blobs::open_at`]。
    pub fn reader_at(
        &self,
        hash: &ContentHash,
        offset: u64,
    ) -> Result<Box<dyn Read + Send>, BlobError> {
        if let Some(held) = &self.memory {
            return held.reader_at(hash, offset);
        }
        Ok(Box::new(self.open_at(hash, offset)?))
    }
}

/// 一张表：哈希到内容。拷出来的 `Blobs` 共用一份。
#[derive(Debug, Default, Clone)]
pub(super) struct Held(Arc<Mutex<HashMap<ContentHash, Arc<[u8]>>>>);

impl Held {
    /// 存一份，交回哈希。已经有了的不再存。
    pub(super) fn put(&self, content: &[u8]) -> ContentHash {
        let hash = ContentHash::of(content);
        self.lock()
            .entry(hash.clone())
            .or_insert_with(|| Arc::from(content));
        hash
    }

    /// 照文件 `source` 存一份，读到的每一段也交给 `each`（同 `Blobs::put_file`）。
    pub(super) fn put_file(
        &self,
        source: &Path,
        mut each: impl FnMut(&[u8]),
    ) -> io::Result<ContentHash> {
        let mut file = File::open(source)?;
        let mut content = Vec::new();
        file.read_to_end(&mut content)?;
        each(&content);
        Ok(self.put_owned(content))
    }

    /// 存一份已经在手里的内容（后台命令在内存里的输出，`session/jobs/output.rs`）：不再拷一份。
    pub(super) fn put_owned(&self, content: Vec<u8>) -> ContentHash {
        let mut hasher = Hasher::default();
        hasher.update(&content);
        let hash = hasher.finish();
        self.lock()
            .entry(hash.clone())
            .or_insert_with(|| Arc::from(content));
        hash
    }

    /// 取一份：没有的报没有。
    pub(super) fn get(&self, hash: &ContentHash) -> Result<Arc<[u8]>, BlobError> {
        self.lock()
            .get(hash)
            .cloned()
            .ok_or_else(|| BlobError::Missing(hash.clone()))
    }

    /// 从 `offset` 起读最多 `length` 个字节，连同整份多大（同 `Blobs::read_range`）。
    pub(super) fn read_range(
        &self,
        hash: &ContentHash,
        offset: u64,
        length: u64,
    ) -> Result<(Vec<u8>, u64), BlobError> {
        let content = self.get(hash)?;
        let size = u64::try_from(content.len()).unwrap_or(u64::MAX);
        let start = usize::try_from(offset.min(size)).unwrap_or(content.len());
        let end = usize::try_from(offset.saturating_add(length).min(size)).unwrap_or(content.len());
        Ok((content[start..end.max(start)].to_vec(), size))
    }

    /// 从 `offset` 起读的一头（发请求时照原样发的图片、PDF，`session/route/exchange.rs`）。
    pub(super) fn reader_at(
        &self,
        hash: &ContentHash,
        offset: u64,
    ) -> Result<Box<dyn Read + Send>, BlobError> {
        let mut cursor = Cursor::new(self.get(hash)?);
        cursor.set_position(offset);
        Ok(Box::new(cursor))
    }

    fn lock(&self) -> MutexGuard<'_, HashMap<ContentHash, Arc<[u8]>>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
