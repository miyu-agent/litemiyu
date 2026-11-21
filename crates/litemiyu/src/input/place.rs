//! 往输入框里放一个本机的文件（Miyu 终端 `input/place.rs`，蓝图「输入框」第 12 条）：`Ctrl+V` 复制进来的走这里，收成文件块：
//! 块上写文件名，发出去换回路径（`label.rs`），点一下打开那个文件。
//! Miyu 认得出种类的（图片、PDF、音频、视频）收成附件；LiteMiyu 只有图片收成图片块（LM-28），别的一律是文件块。

use std::path::PathBuf;

use super::InputBox;
use super::label::{file_label, quoted};

impl InputBox {
    /// 收成文件块：块上写文件名（`file`、`folder` 是文件、目录的写法），发出去换回路径（Miyu 的 `put_path`）。
    pub(super) fn put_path(&mut self, path: PathBuf, words: (&str, &str)) {
        let label = file_label(&path, path.is_dir(), words);
        self.editor.insert_file(label, quoted(&path), path);
    }
}

impl InputBox {
    /// 收一张图片（LM-28，Miyu 的 `attach`）：光标处放一个图片块，块上的字随后照先后编号（[`InputBox::renumber`]）。
    /// `label` 是块上的写法（`[图片 {n}]`）。
    pub fn attach_image(&mut self, path: PathBuf, label: &str) {
        self.goal_col = None;
        self.follow = true;
        self.put_image(path, label);
    }

    /// 放一个图片块，先写第 0 张占着位置，编号等 [`InputBox::renumber`]。
    pub(super) fn put_image(&mut self, path: PathBuf, label: &str) {
        self.editor.insert_image(label.replace("{n}", "0"), path);
    }

    /// 输入框里的图片块照先后接着 `base`（正文里你说的话还留着几张）编号（「输入框」第 12 条：整个会话一直往下排），
    /// 块上照 `label` 写。没有图片块的什么都不做。
    pub fn renumber(&mut self, base: usize, label: &str) {
        if self.editor.has_images() {
            self.editor
                .renumber(|n| label.replace("{n}", &(base + n).to_string()));
        }
    }
}
