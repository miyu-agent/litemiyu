//! 输入框里文件块上写的字（`text/<语言>.json` 的 `file_block`，Miyu 终端 `attach.file`、`attach.folder`）：`Ctrl+V` 收成文件块时
//! 才读，别的字表不带它（设计第七节「守红线要做的」第 1 条）。原来和 `@` 文件列表的字放在一格（`mention`），LM-2 砍了 `@` 以后
//! 单留这两句。

use super::{Config, localize, read, texts};

texts! {
    /// 文件块上写的。
    #[derive(Debug, Clone)]
    pub struct FileBlockTexts {
        /// 文件收成的块上写的：`{name}` 文件名（Miyu 终端 `attach.file`）。
        pub file: String,
        /// 目录收成的块上写的：`{name}` 目录名（Miyu 终端 `attach.folder`）。
        pub folder: String,
    }
}

texts! {
    @shared
    /// 只读 `file_block` 那一格的外壳：同一份 JSON，别的格不管。
    pub struct Only {
        /// 文件块上的字。
        pub file_block: FileBlockTexts,
    }
}

impl Config {
    /// 文件块上的字：每次调都照语言读一遍（只在 `Ctrl+V` 收文件块时调）。
    ///
    /// # Errors
    ///
    /// 编进程序的那一份读不懂（安装坏了才会有，测试守着）。
    pub fn file_block_texts(&self) -> Result<FileBlockTexts, String> {
        let (name, json) = localize::text(&self.language)?;
        read::parse::<Only>(&name, json).map(|only| only.file_block)
    }
}
