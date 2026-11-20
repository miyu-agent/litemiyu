//! 文件记忆（L6，`docs/00-设计.md` 第五节第 7 条）：数据那一处的 `memory/` 文件夹，一条记忆一个小文件，加一份目录 `MEMORY.md`。
//! 目录在会话开头作为一块事实交给她（`context.injected`，模块 `memory`），不进 system、不动缓存前缀；她用现成的 `read`、
//! `write`、`edit` 读写、`shell` 删，不另设工具（守卫放行这个文件夹：`guard.rs`）。
//!
//! - 什么时候交：这一段上下文里还没交过的那一轮（会话的第一轮；压缩、撤销拿走了的下一轮再交）。会话中途她改了目录不重交：
//!   下一个会话看得到（设计要的就到这）。无功能的一档不交（`Actor::bare`）。
//! - 目录封顶：读的时候一行超过 [`LINE_CHARS`] 个字符的截掉后面，只给前 [`MOST_LINES`] 行。说明里写着满了先合并、删旧的。
//! - 几个窗口同时写：各写各的文件不冲突；改目录那一行这一趟不加锁（设计说要拿文件锁，留给窗口之间那一步）。

use std::collections::BTreeMap;
use std::path::Path;

use lm_kernel::kernel::event::ContextInjected;
use lm_kernel::kernel::facts::Present;
use lm_kernel::kernel::id::{FactKind, ModuleId};
use lm_kernel::kernel::session::Injection;
use lm_kernel::kernel::template::Template;

use crate::resources::MemoryTexts;

/// 模块编号、类别。
pub(crate) const MODULE: &str = "memory";

/// 目录的文件名。
pub(crate) const INDEX: &str = "MEMORY.md";

/// 目录一行最多几个字符（2026-11-11 项目主人定）。
pub(crate) const LINE_CHARS: usize = 150;

/// 目录最多给几行（2026-11-11 项目主人定）。
pub(crate) const MOST_LINES: usize = 300;

/// 外壳里目录的位置。
const INDEX_FIELD: &str = "{index}";

/// 外壳的结束标签的开头（大小写都算）。
const CLOSE: &str = "</memory";

/// 目录里外壳的结束标签（大小写都算）：那一处的 `<` 写成 `\u003c`，伪造不出标签外的字。
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

/// 目录的原文 `text` 照封顶截：每行最多 [`LINE_CHARS`] 个字符，最多 [`MOST_LINES`] 行；末尾的空行不算。
pub(crate) fn capped(text: &str) -> String {
    text.trim_end()
        .lines()
        .take(MOST_LINES)
        .map(|line| line.chars().take(LINE_CHARS).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

/// 这一轮要不要交：这一段上下文里交过的（`present` 里有这个模块的）不交；要交的读文件夹 `folder` 里的目录拼好。读不了的
/// （不在、不是 UTF-8）照还没有一行。碰磁盘，在阻塞线程里调。
///
/// # Errors
///
/// 出厂的字的模板坏了（编进程序的，不会）。
pub(crate) fn tail(
    texts: &MemoryTexts,
    folder: &Path,
    present: &[Present],
) -> Result<Option<Injection>, String> {
    if present.iter().any(|one| one.module.as_str() == MODULE) {
        return Ok(None);
    }
    let index = std::fs::read_to_string(folder.join(INDEX)).unwrap_or_default();
    let index = match capped(&index) {
        shown if shown.trim().is_empty() => texts.empty.trim_end().to_string(),
        shown => shown,
    };
    let folder_text = crate::session::path_text::path_text(folder);
    // 目录原样接在中间，不经模板换（模板换进去的字段一律照 JSON 转成一行）：外壳照 `{index}` 切成前后两段，路径照模板换进前一段，
    // 目录里外壳的结束标签写成 `\u003c`（同 MCP 服务的说明，`crate::mcp` 的 `instructions.rs`）。
    let (head, tail) = texts
        .index
        .split_once(INDEX_FIELD)
        .ok_or_else(|| format!("no {INDEX_FIELD} in the memory template"))?;
    let render = |template: &str, fields: &[(&str, &str)]| {
        Template::parse(template)
            .and_then(|template| {
                template.render(&fields.iter().copied().collect::<BTreeMap<_, _>>())
            })
            .map_err(|error| error.to_string())
    };
    let text = format!(
        "{}{}{}",
        render(head, &[("folder", &folder_text)])?,
        guarded(&index),
        render(tail, &[])?
    );
    Ok(Some(Injection {
        module: ModuleId::parse(MODULE).map_err(|error| error.to_string())?,
        fact: ContextInjected {
            kind: FactKind::parse(MODULE).map_err(|error| error.to_string())?,
            text,
            refs: vec![folder_text],
            after: false,
        },
    }))
}

#[cfg(test)]
mod tests;
