//! 看不了的附件带上原来的路径的两句（施工 3-9 五补，`docs/blueprint/drivers/openai-chat.md` 第 9 条）：随核心附带的字，存进
//! 策略快照的 `core.drivers.attached_path`；以前造的快照里没有，读成没有，照旧不带路径。

use crate::drivers::AttachedPathSources;
use crate::policy::read::{Fields, Read, read_by_name};
use serde::Serialize;

/// 两句，每一格是 `resources/core/drivers/` 下同名（下划线换成 `-`）文件的原文。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AttachedPathTexts {
    /// 看不了的图片，带名字和路径（`image-omitted-path.txt`）。
    pub image_omitted_path: String,
    /// 读不了的文件，带名字、类型、大小和路径（`file-omitted-path.txt`）。
    pub file_omitted_path: String,
}

read_by_name!(AttachedPathTexts, AttachedPathTexts::read);

impl AttachedPathTexts {
    /// 照名字一格格读（`policy/read.rs`，引擎线第七趟瘦身）：读出来的同原来派生的读法。
    pub(crate) fn read(value: &serde_json::Value) -> Read<AttachedPathTexts> {
        let fields = Fields::of(value, "AttachedPathTexts")?;
        Ok(AttachedPathTexts {
            image_omitted_path: fields.text("image_omitted_path")?,
            file_omitted_path: fields.text("file_omitted_path")?,
        })
    }
}

impl AttachedPathTexts {
    /// 交给驱动的原文。
    pub(crate) fn sources(&self) -> AttachedPathSources<'_> {
        AttachedPathSources {
            image_omitted_path: &self.image_omitted_path,
            file_omitted_path: &self.file_omitted_path,
        }
    }
}
