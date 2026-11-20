//! 编进程序的工具的字（L3 四）：Miyu 的工具从资源目录读说明和输出里的几句（`software/<软件包>/…`）；LiteMiyu 只有一个程序
//! 文件，照资源目录里的相对路径编进程序（[`files`]，照资源目录生成），[`super::load`] 照路径找。

mod files;

/// 路径 `path`（照资源目录里的相对位置，如 `software/basesystem/tools/read.json`）的原文；程序里没有的没有。
pub(crate) fn get(path: &str) -> Option<&'static str> {
    files::FILES
        .binary_search_by(|(one, _)| one.cmp(&path))
        .ok()
        .map(|at| files::FILES[at].1)
}

#[cfg(test)]
mod tests;
