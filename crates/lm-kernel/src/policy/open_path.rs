//! 看得了的附件开头带上路径的两句（施工 3-9 六补，`docs/blueprint/drivers/openai-chat.md` 第 9 条）：随核心附带的字，存进
//! 策略快照的 `core.drivers.open_path`；以前造的快照里没有，读成没有，照旧只带名字。

use crate::drivers::OpenPathSources;
use crate::policy::read::{Fields, Read, read_by_name};
use serde::Serialize;

/// 两句，每一格是 `resources/core/drivers/` 下同名（下划线换成 `-`）文件的原文。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct OpenPathTexts {
    /// 能看图时图片前面，带名字和路径（`image-open-path.txt`）。
    pub image_open_path: String,
    /// 照字放进消息的文本文件开头，带名字和路径（`file-open-path.txt`）。
    pub file_open_path: String,
}

read_by_name!(OpenPathTexts, OpenPathTexts::read);

impl OpenPathTexts {
    /// 照名字一格格读（`policy/read.rs`，引擎线第七趟瘦身）：读出来的同原来派生的读法。
    pub(crate) fn read(value: &serde_json::Value) -> Read<OpenPathTexts> {
        let fields = Fields::of(value, "OpenPathTexts")?;
        Ok(OpenPathTexts {
            image_open_path: fields.text("image_open_path")?,
            file_open_path: fields.text("file_open_path")?,
        })
    }
}

impl OpenPathTexts {
    /// 交给驱动的原文。
    pub(crate) fn sources(&self) -> OpenPathSources<'_> {
        OpenPathSources {
            image_open_path: &self.image_open_path,
            file_open_path: &self.file_open_path,
        }
    }
}
