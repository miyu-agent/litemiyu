//! 驱动的几句占位（`resources/core/drivers/`，`docs/blueprint/policy.md` 的 `CoreTexts`）：策略快照 `core.drivers` 那一格，
//! 读成驱动的占位（`docs/blueprint/drivers/openai-chat.md`）。施工 3-9 四补从 `snapshot.rs` 挪出来：那一份到了行数上限。

use crate::drivers::{DriverTextSources, DriverTexts};
use crate::kernel::template::TemplateError;
use crate::policy::read::{Fields, Read, read_by_name};
use serde::Serialize;

use crate::policy::attached_path::AttachedPathTexts;
use crate::policy::image_name::ImageNameTexts;
use crate::policy::open_path::OpenPathTexts;
use crate::policy::text_file::TextFileTexts;
use crate::policy::vision::ImageDescriptionTexts;

/// 驱动的几句占位。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DriverPlaceholders {
    /// 图片发不了（`image-omitted.txt`）。
    pub image_omitted: String,
    /// 文件读不了（`file-omitted.txt`）。
    pub file_omitted: String,
    /// 工具一个字都没回（`no-output.txt`）。
    pub no_output: String,
    /// 工具结果里的附件挪到了后面（`tool-attachments.txt`）。
    pub tool_attachments: String,
    /// 工具结果只有附件，挪到了后面（`tool-attachments-only.txt`）。
    pub tool_attachments_only: String,
    /// 文本文件照字放进消息的三句（施工 3-9 三补）。以前造的快照里没有，读成没有：文本文件照别的文件写占位；没有的
    /// 不写。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_file: Option<TextFileTexts>,
    /// 带名字的图片的三句（施工 3-9 四补）。以前造的快照里没有，读成没有：带名字的图片照不带名字的写；没有的不写。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_name: Option<ImageNameTexts>,
    /// 替它看的图的三句标签（施工 8-17）。以前造的快照里没有，读成没有：看不了图的照旧写占位；没有的不写。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub image_description: Option<ImageDescriptionTexts>,
    /// 看不了的附件带上原来的路径的两句（施工 3-9 五补）。以前造的快照里没有，读成没有：照旧不带路径；没有的不写。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attached_path: Option<AttachedPathTexts>,
    /// 看得了的附件开头带上路径的两句（施工 3-9 六补）。以前造的快照里没有，读成没有：照旧只带名字；没有的不写。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open_path: Option<OpenPathTexts>,
}

read_by_name!(DriverPlaceholders, DriverPlaceholders::read);

impl DriverPlaceholders {
    /// 照名字一格格读（`policy/read.rs`，引擎线第七趟瘦身）：读出来的同原来派生的读法。
    pub(crate) fn read(value: &serde_json::Value) -> Read<DriverPlaceholders> {
        let fields = Fields::of(value, "DriverPlaceholders")?;
        Ok(DriverPlaceholders {
            image_omitted: fields.text("image_omitted")?,
            file_omitted: fields.text("file_omitted")?,
            no_output: fields.text("no_output")?,
            tool_attachments: fields.text("tool_attachments")?,
            tool_attachments_only: fields.text("tool_attachments_only")?,
            text_file: fields.opt("text_file", TextFileTexts::read)?,
            image_name: fields.opt("image_name", ImageNameTexts::read)?,
            image_description: fields.opt("image_description", ImageDescriptionTexts::read)?,
            attached_path: fields.opt("attached_path", AttachedPathTexts::read)?,
            open_path: fields.opt("open_path", OpenPathTexts::read)?,
        })
    }
}

impl DriverPlaceholders {
    /// 读成驱动的占位。
    ///
    /// # Errors
    ///
    /// 占位的模板坏了，或者要了不该有的字段。
    pub(crate) fn texts(&self) -> Result<DriverTexts, TemplateError> {
        DriverTexts::new(DriverTextSources {
            image_omitted: &self.image_omitted,
            file_omitted: &self.file_omitted,
            no_output: &self.no_output,
            tool_attachments: &self.tool_attachments,
            tool_attachments_only: &self.tool_attachments_only,
            text_file: self.text_file.as_ref().map(TextFileTexts::sources),
            image_name: self.image_name.as_ref().map(ImageNameTexts::sources),
            image_description: self
                .image_description
                .as_ref()
                .map(ImageDescriptionTexts::sources),
            attached_path: self.attached_path.as_ref().map(AttachedPathTexts::sources),
            open_path: self.open_path.as_ref().map(OpenPathTexts::sources),
        })
    }
}
