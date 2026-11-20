//! 核心给的统一格式差异（撤销的回应里 `files` 的 `diff`，`protocol/undo.md`）读成 [`Diff`](crate::diff::Diff)：`@@ -a,b +c,d @@` 定起始
//! 行号，` ` 不变、`-` 删掉、`+` 加上；两段之间是一行略过的。读不懂的行不要。

#[cfg(test)]
mod tests {}
