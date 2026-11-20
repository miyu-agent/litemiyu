//! 开不了链接、文件时的提示（`text/zh.json` 的 `open`），蓝图 `tui.md`「她的回答：Markdown」第 10 条。

use crate::config::texts;

texts! {
    /// 开不了时说什么。
    #[derive(Debug, Clone)]
    pub struct OpenTexts {
        /// 不开这种协议：`{url}` 地址。
        pub refused: String,
        /// 本机的文件不在了：`{path}` 路径。
        pub missing: String,
        /// 起不来打开它的程序：`{reason}` 原因。
        pub failed: String,
        /// 本机的文件、目录没有能打开它的程序（默认程序、浏览器都没有）：`{path}` 路径（Miyu `shown.no_program`）。
        pub no_program: String,
    }
}
