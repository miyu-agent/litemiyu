//! 往输入框里放一个本机的文件（Miyu 终端 `input/place.rs`，蓝图「输入框」第 12 条）：`Ctrl+V` 复制进来的走这里，收成文件块：
//! 块上写文件名，发出去换回路径（`label.rs`），点一下打开那个文件。
//! Miyu 认得出种类的收成附件，LiteMiyu 没有附件，一律是文件块。

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
