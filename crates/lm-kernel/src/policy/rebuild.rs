//! 压后重建装进快照的字和数（`docs/blueprint/compaction.md` 第八、九条，施工 6-5）：检查点里代码写的几段的模板、
//! 重读的文件那一块的头尾，和重读几个、多大的数。以前造的快照里没有，读成没有：那些会话不写那几段、不重读。

use crate::assemble::RestoredWrap;
use crate::kernel::session::{Notes, Rebuild, SkillRebuild};
use crate::kernel::template::Template;
use crate::policy::read::{Fields, Read, read_by_name};
use serde::Serialize;

use crate::policy::snapshot::BuildError;

/// 压后重建的字（`compaction/` 下）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RebuildTexts {
    /// 读过、改过的文件清单（`notes-files.txt`）。
    pub notes_files: String,
    /// 清单放不下的还有几个（`notes-files-more.txt`）。
    pub notes_files_more: String,
    /// 取回指路（`notes-retrieve.txt`）。
    pub notes_retrieve: String,
    /// 太大没重读的（`notes-too-large.txt`）。
    pub notes_too_large: String,
    /// 重读的文件那一块的头（`restored-open.txt`）。
    pub restored_open: String,
    /// 重读的文件那一块的尾（`restored-close.txt`）。
    pub restored_close: String,
}

read_by_name!(RebuildTexts, RebuildTexts::read);

impl RebuildTexts {
    /// 照名字一格格读（`policy/read.rs`，引擎线第七趟瘦身）：读出来的同原来派生的读法。
    pub(crate) fn read(value: &serde_json::Value) -> Read<RebuildTexts> {
        let fields = Fields::of(value, "RebuildTexts")?;
        Ok(RebuildTexts {
            notes_files: fields.text("notes_files")?,
            notes_files_more: fields.text("notes_files_more")?,
            notes_retrieve: fields.text("notes_retrieve")?,
            notes_too_large: fields.text("notes_too_large")?,
            restored_open: fields.text("restored_open")?,
            restored_close: fields.text("restored_close")?,
        })
    }
}

/// 压后重建的数（`compaction.md`「对外的样子」的策略数据）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct RebuildNumbers {
    /// 最多重读几个。
    pub files: usize,
    /// 单个最多多少 token。
    pub file_tokens: u64,
    /// 合计最多多少 token。
    pub total: u64,
    /// 窗口不到这么多的不重读。
    pub min_window: u64,
    /// 交给执行器的候选最多几个。
    pub candidates: usize,
    /// 用过的技能另算一组（施工 X-10 下）。以前造的快照里没有，读成没有：技能的正文照普通文件算；没有的不写。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skills: Option<SkillNumbers>,
}

/// 用过的技能那一组的数（`09-压缩.md` 第四节）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct SkillNumbers {
    /// 单个最多多少 token。
    pub tokens: u64,
    /// 合计最多多少 token。
    pub total: u64,
}

/// 出厂的数（`compaction.md` 第九条）：最多 5 个，单个 5000，合计 50000，窗口 32000 以下不重读；候选多给一倍留余地。
/// 用过的技能另算，单个 5000、合计 25000（施工 X-10 下）。
pub const REBUILD: RebuildNumbers = RebuildNumbers {
    files: 5,
    file_tokens: 5_000,
    total: 50_000,
    min_window: 32_000,
    candidates: 10,
    skills: Some(SkillNumbers {
        tokens: 5_000,
        total: 25_000,
    }),
};

read_by_name!(RebuildNumbers, RebuildNumbers::read);
read_by_name!(SkillNumbers, SkillNumbers::read);

impl SkillNumbers {
    /// 照名字一格格读（`policy/read.rs`，引擎线第八趟瘦身）：读出来的同原来派生的读法。
    pub(crate) fn read(value: &serde_json::Value) -> Read<SkillNumbers> {
        let fields = Fields::of_struct(value, "SkillNumbers", &["tokens", "total"])?;
        Ok(SkillNumbers {
            tokens: fields.number("tokens")?,
            total: fields.number("total")?,
        })
    }
}

impl RebuildNumbers {
    /// 照名字一格格读（`policy/read.rs`，引擎线第八趟瘦身）：读出来的同原来派生的读法。
    pub(crate) fn read(value: &serde_json::Value) -> Read<RebuildNumbers> {
        let fields = Fields::of_struct(
            value,
            "RebuildNumbers",
            &[
                "files",
                "file_tokens",
                "total",
                "min_window",
                "candidates",
                "skills",
            ],
        )?;
        Ok(RebuildNumbers {
            files: fields.number("files")?,
            file_tokens: fields.number("file_tokens")?,
            total: fields.number("total")?,
            min_window: fields.number("min_window")?,
            candidates: fields.number("candidates")?,
            skills: fields.opt("skills", SkillNumbers::read)?,
        })
    }

    /// 交给内核的样子。
    pub(crate) fn kernel(self) -> Rebuild {
        Rebuild {
            files: self.files,
            file_tokens: self.file_tokens,
            total: self.total,
            min_window: self.min_window,
            candidates: self.candidates,
            skills: self.skills.map(|skills| SkillRebuild {
                tokens: skills.tokens,
                total: skills.total,
            }),
        }
    }
}

impl RebuildTexts {
    /// 代码写的几段的模板，每一份试换过字段。
    ///
    /// # Errors
    ///
    /// 哪一份模板坏了。
    pub(crate) fn notes(&self) -> Result<Notes, BuildError> {
        Ok(Notes {
            files: template(&self.notes_files, &[])?,
            files_more: template(&self.notes_files_more, &["count"])?,
            retrieve: template(&self.notes_retrieve, &["upto"])?,
            too_large: template(&self.notes_too_large, &["files"])?,
            uncovered: None,
        })
    }

    /// 重读的文件那一块的头尾。
    ///
    /// # Errors
    ///
    /// 头的模板坏了。
    pub(crate) fn wrap(&self) -> Result<RestoredWrap, BuildError> {
        Ok(RestoredWrap {
            open: template(&self.restored_open, &["path"])?,
            close: self.restored_close.clone(),
        })
    }
}

/// 读一份模板，拿 `fields` 里的每个字段试换一次。
pub(crate) fn template(source: &str, fields: &[&str]) -> Result<Template, BuildError> {
    let bad = |error| BuildError::Texts {
        which: "compaction rebuild texts",
        error,
    };
    let template = Template::parse(source).map_err(bad)?;
    let trial: std::collections::BTreeMap<&str, &str> =
        fields.iter().map(|field| (*field, "")).collect();
    template.render(&trial).map_err(bad)?;
    Ok(template)
}
