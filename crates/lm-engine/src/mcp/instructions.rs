//! MCP 服务交的说明放进 system 尾部（L5 六，Miyu `miyu-endpoint/src/mcp/instructions.rs`，施工 X-39，`mcp.md`「怎么走」第三条
//! 第 5 条）：一个服务一段，键是 [`key`]，外壳照 `core/mcp/instructions.txt`。原文去掉首尾的空白再截到 [`APPEND_MAX`] 个字符；
//! 原文里出现外壳的结束标签的，那一处的 `<` 写成 `<`，伪造不出标签外的字，别的照原样（它就是说明）：不经模板换，外壳照
//! `{text}` 切成前后两段，服务名照模板换进前一段，原文接在中间。会话照工具面挑哪几段带、什么时候换（同 Miyu `miyu-session` 的
//! `appended`：工具面上有这个服务的工具才带，工具面换了的那一轮照这一刻的说明换）。

use lm_kernel::policy::APPEND_MAX;

use crate::mcp::tool::render;

/// 外壳的结束标签的开头（大小写都算）。
const CLOSE: &str = "</mcp-instructions";

/// 外壳里原文的位置：照它切成前后两段。
const TEXT: &str = "{text}";

/// 外壳（`core/mcp/instructions.txt`）。
const SHELL: &str = include_str!("../../../../resources/core/mcp/instructions.txt");

/// 服务那一段的键的开头（Miyu `miyu-session` 的 `mcp_key`）。
const MCP: &str = "mcp:";

/// 服务 `service` 的说明那一段的键：`mcp:<服务名>`。
pub(crate) fn key(service: &str) -> String {
    format!("{MCP}{service}")
}

/// 服务 `service` 交的说明 `text` 拼好的那一段。外壳的写法坏了的（编进程序的，不会）没有。
pub(crate) fn block(service: &str, text: &str) -> Option<String> {
    let (head, tail) = SHELL.split_once(TEXT)?;
    let opened = render(head, &[("name", service)]);
    let block = format!(
        "{opened}{}{}",
        guarded(&cut(text.trim())),
        render(tail, &[])
    );
    Some(block.trim_end().to_string())
}

/// 截到 [`APPEND_MAX`] 个字符。
fn cut(text: &str) -> String {
    text.chars().take(APPEND_MAX).collect()
}

/// 原文里外壳的结束标签（大小写都算）：那一处的 `<` 写成 `<`。只动 ASCII，字节的位置照旧对得上。
fn guarded(text: &str) -> String {
    let lower = text.to_ascii_lowercase();
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    while let Some(found) = lower[at..].find(CLOSE) {
        let start = at + found;
        out.push_str(&text[at..start]);
        out.push_str("\\u003c");
        at = start + 1;
    }
    out.push_str(&text[at..]);
    out
}

#[cfg(test)]
mod tests;
