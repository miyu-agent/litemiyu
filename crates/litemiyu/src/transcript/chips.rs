//! 你说的话里的块（蓝图 `tui.md`「正文」第 2 条、「输入框」第 11 条）：粘贴块点开看全文。Miyu 这里还有附件（图片、PDF、
//! 音频、视频）和文件块：LiteMiyu 还没有贴附件的路，跟着粘贴截图那一步搬回来。

/// 你说的话里的一块。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chip {
    /// 块上写的：`[已粘贴 14 行]`。
    pub label: String,
    /// 原文：粘贴块点开换成它。
    pub full: String,
}
