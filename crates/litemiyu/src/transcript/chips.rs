//! 你说的话里的块（蓝图 `tui.md`「正文」第 2 条、「输入框」第 11、12 条）：粘贴块点开看全文；图片块（LM-28）只写块上的字、
//! 不展开，点块用系统的程序打开那个文件，编号整个会话一直往下排。Miyu 另有 PDF、音频、视频的附件块：LiteMiyu 只有图片。

use std::path::PathBuf;

use super::Transcript;

/// 你说的话里的一块。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chip {
    /// 块上写的：`[已粘贴 14 行]`、`[图片 1]`。
    pub label: String,
    /// 原文：粘贴块点开换成它；图片块的就是块上的字。
    pub full: String,
    /// 是图片块：编号照它数，不展开。
    pub image: bool,
    /// 图片块是本机的哪个文件：点块用系统的程序打开它。粘贴块、不知道在哪的图片（应急模式里发的）是 `None`。
    pub file: Option<PathBuf>,
}

impl Transcript {
    /// 正文里你说的话还留着几张图片：撤销藏起来的不算，`/new` 清空了从头数（蓝图「输入框」第 12 条）。正文只留几屏、
    /// 放掉了的段里的照旧算（`pieces.rs` 放的时候记下）。
    pub fn image_count(&self) -> usize {
        self.shed_images + super::pieces::images_in(&self.entries)
    }
}

impl Transcript {
    /// 序号是 `seq` 的那一句带着图片（[`crate::core::Push::Images`]，Miyu 终端 `transcript/attached.rs` 的 `attach_to`）：
    /// `files` 是每一张在本机的哪儿。你在这个界面发的那句已经画着图片块，只把块指的文件换成会话记的那一份（暂存的截图过后
    /// 会删，会话目录里那一份留着）；补发来的那句字里照 `template`（`[图片 {n}]`）写着块上的字，照先后对上、画成块，字里
    /// 没写的（别的语言的界面发的）接在字后面，编号照正文里已有的接着数。
    pub(super) fn images(&mut self, seq: u64, files: Vec<Option<String>>, template: &str) {
        let Some(at) = self
            .entries
            .iter()
            .rposition(|e| e.kind == super::Kind::User && e.seq == Some(seq))
        else {
            return;
        };
        let files = files.into_iter().map(|file| file.map(PathBuf::from));
        if self.entries[at].pasted.iter().any(|c| c.image) {
            let drawn = self.entries[at].pasted.iter_mut().filter(|c| c.image);
            for (chip, file) in drawn.zip(files) {
                if file.is_some() {
                    chip.file = file;
                }
            }
            return;
        }
        let next = self.image_count() + 1;
        let entry = &mut self.entries[at];
        let mut written = labels_in(&entry.text, template).into_iter();
        for (count, file) in (next..).zip(files) {
            let label = written.next().unwrap_or_else(|| {
                let label = template.replace("{n}", &count.to_string());
                if !entry.text.is_empty() && !entry.text.ends_with(char::is_whitespace) {
                    entry.text.push(' ');
                }
                entry.text.push_str(&label);
                label
            });
            entry.pasted.push(Chip {
                full: label.clone(),
                label,
                image: true,
                file,
            });
        }
    }
}

/// 字里照 `template`（`[图片 {n}]`）写的块，照先后（Miyu 终端 `transcript/attached.rs` 的 `labels_in`）。
fn labels_in(text: &str, template: &str) -> Vec<String> {
    let Some((head, tail)) = template.split_once("{n}") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find(head) {
        let after = &rest[at + head.len()..];
        let digits = after.chars().take_while(char::is_ascii_digit).count();
        if digits > 0 && after[digits..].starts_with(tail) {
            out.push(format!("{head}{}{tail}", &after[..digits]));
            rest = &after[digits + tail.len()..];
        } else {
            rest = after;
        }
    }
    out
}
