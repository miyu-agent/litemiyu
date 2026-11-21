//! 头附的图片（LM-28，Miyu 协议的 `blob.put` 加 `session.send` 的 `attachments`，`docs/blueprint/protocol.md`）：头把一句话里附的
//! 图片交过来，会话认一遍（[`crate::tool::attachment::kind`]，照内容、不看扩展名）、存成 blob，交回图片块，头再连同字一起
//! `Send`。Miyu 的头走协议传文件，LiteMiyu 的界面和引擎在一个进程里，直接交路径或者字节。
//!
//! - 只收图片（PNG、JPEG、GIF、WebP，量得出宽高，不超过 5 MiB、每边不超过 8000 像素）：别的文件照旧由头写成路径。
//! - 剪贴板贴的图（`keep`）没有长久的本机路径：平常另拷一份进会话目录的 `attachments/`（[`crate::store::attachments::keep`]），
//!   块的 `path` 指着它，她照这个路径读得到、模型看不了时占位那一句带得上。应急模式（blob 只在内存里）什么都不写，不带路径。
//! - 不进内核、不记事件：只存 blob。存了没发出去的（后面那一句被拒了）留在 blob 里，照没人引用的 blob 回收。

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::sync::oneshot;

use lm_kernel::kernel::block::{Block, Image};
use lm_kernel::kernel::id::{FileName, SourcePath};

use super::{Handle, Message, Stopped};
use crate::store::blob::Blobs;
use crate::tool::attachment::{Kind, kind};
use crate::tool::picture;

/// 头附的一张图。
#[derive(Debug, Clone)]
pub struct Attach {
    /// 文件名，只是名字。
    pub name: String,
    /// 内容在哪。
    pub source: Source,
    /// 剪贴板贴的：没有长久的本机路径，另拷一份进会话目录。
    pub keep: bool,
}

/// 一张图的内容在哪。
#[derive(Debug, Clone)]
pub enum Source {
    /// 本机的文件：会话自己读。
    File(PathBuf),
    /// 已经在内存里的（应急模式下贴的图不落盘）。
    Held(Arc<[u8]>),
}

/// 附上了的。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attached {
    /// 图片块，照交来的先后。
    pub blocks: Vec<Block>,
    /// 这个会话这时的模型看不了图（模型资料的输入里没有图片）：图不会原样交给它，配了看图模型的由它转述，没配的只在那个
    /// 位置写一句带文件名、路径的占位。头照它提示一句。
    pub blind: bool,
}

/// 附不上。带着是哪一个的名字。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttachError {
    /// 读不了（文件没了、没权限），或者存不下来。
    Unreadable(String),
    /// 超过上限：5 MiB，或者有一边超过 8000 像素。
    TooBig(String),
    /// 不是能发的图片。
    NotImage(String),
}

impl AttachError {
    /// 原因码：头照它找提示的字。
    pub fn code(&self) -> &'static str {
        match self {
            AttachError::Unreadable(_) => "attachment_unreadable",
            AttachError::TooBig(_) => "attachment_too_big",
            AttachError::NotImage(_) => "attachment_not_image",
        }
    }

    /// 是哪一个。
    pub fn name(&self) -> &str {
        match self {
            AttachError::Unreadable(name)
            | AttachError::TooBig(name)
            | AttachError::NotImage(name) => name,
        }
    }
}

impl Handle {
    /// 把 `items` 存成 blob，照先后交回图片块，连同这时的模型看不看得了图。有一张附不上的整批不算，交回是哪一张、为什么。
    ///
    /// # Errors
    ///
    /// 会话停了。里面那一层：[`AttachError`]。
    pub async fn attach(
        &self,
        items: Vec<Attach>,
    ) -> Result<Result<Attached, AttachError>, Stopped> {
        let (reply, answer) = oneshot::channel();
        self.send(Message::Attach {
            session: self.id.clone(),
            items,
            reply,
        })?;
        answer.await.map_err(|_| Stopped)
    }
}

/// 存每一张，交回图片块。`dir` 是会话的附件目录，应急模式没有。碰磁盘，在阻塞线程里调。
pub(crate) fn store(
    blobs: &Blobs,
    dir: Option<&Path>,
    items: Vec<Attach>,
) -> Result<Vec<Block>, AttachError> {
    items
        .into_iter()
        .map(|item| one(blobs, dir, item).map(Block::Image))
        .collect()
}

fn one(blobs: &Blobs, dir: Option<&Path>, item: Attach) -> Result<Image, AttachError> {
    let Attach { name, source, keep } = item;
    let unreadable = || AttachError::Unreadable(name.clone());
    let read;
    let (bytes, from): (&[u8], Option<PathBuf>) = match &source {
        Source::Held(bytes) => (bytes, None),
        Source::File(path) => {
            read = read_file(path).map_err(|big| match big {
                true => AttachError::TooBig(name.clone()),
                false => unreadable(),
            })?;
            (&read, (!keep).then(|| path.clone()))
        }
    };
    let (media_type, width, height) = match kind(bytes, None) {
        Ok(Kind::Image {
            media_type,
            width,
            height,
        }) => (media_type, width, height),
        Ok(Kind::File { .. }) => return Err(AttachError::NotImage(name)),
        Err(_) => return Err(AttachError::TooBig(name)),
    };
    let blob = blobs.put(bytes).map_err(|_| unreadable())?;
    let kept = match (keep, dir) {
        (true, Some(dir)) => Some(
            crate::store::attachments::keep(dir, &name, media_type.as_str(), bytes)
                .map_err(|_| unreadable())?,
        ),
        _ => None,
    };
    let path = kept
        .or_else(|| from.and_then(|path| std::fs::canonicalize(path).ok()))
        .and_then(|path| SourcePath::parse(path.to_str()?).ok());
    Ok(Image {
        blob,
        name: FileName::parse(&name).ok(),
        media_type,
        width,
        height,
        path,
    })
}

/// 读一个文件，最多读到图片的字节上限多一个：`Err(true)` 是超了，`Err(false)` 是读不了。
fn read_file(path: &Path) -> Result<Vec<u8>, bool> {
    let file = std::fs::File::open(path).map_err(|_| false)?;
    let mut bytes = Vec::new();
    file.take(picture::MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| false)?;
    match bytes.len() as u64 > picture::MAX_BYTES {
        true => Err(true),
        false => Ok(bytes),
    }
}

#[cfg(test)]
mod tests;
