//! 出厂的字（L3 二，执行计划第六节）：Miyu 从资源目录读（`miyu_store::resources::ResourceRoot`），这里编进程序
//! （`include_str!`），读不了的错整个没有。只留 LiteMiyu 用得上的几份：拼 system 的来源、核心的几行、
//! 项目指令的包法。人格、场所、记忆、知识库、向量、用户覆盖的那一层都没有（设计第五节第 15 条）。

use std::path::Path;

use lm_kernel::policy::shipped::{PROMPT, core};
use lm_kernel::policy::{CoreLines, Sources};

/// 拼 system 的来源：随核心附带的字，加提示词。`config` 是配置目录：里面有 `prompt.md` 的照它的原文（空文件就是什么都
/// 不加），没有的用出厂那一段（设计第五节第 15 条）。读不了（不是「没有」）的也用出厂的，记一行 `WARN`。
pub fn sources(config: &Path) -> Sources {
    let path = config.join("prompt.md");
    let prompt = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(_) => PROMPT.to_string(),
    };
    Sources {
        core: core(),
        prompt,
    }
}

/// 子会话的 system 末尾接的那一段（`core/jobs/subagent-venue.txt`，`agents.md` 第九条第 3 条，L5 二）：说它是子代理、交代是
/// 父会话写的、最后的回答就是回报（`Snapshot::with_venue`）。
pub fn subagent_venue() -> &'static str {
    include_str!("../../../resources/core/jobs/subagent-venue.txt")
}

/// 核心的几行，造会话时拼进 system（`Snapshot::with_core_lines`）。跑的是哪一版照这一份编进去的版本。
pub fn core_lines() -> CoreLines {
    CoreLines {
        local_paths: include_str!("../../../resources/core/local-paths-rule.txt").to_string(),
        machine: include_str!("../../../resources/core/machine.txt").to_string(),
        harness: include_str!("../../../resources/core/agent-version.txt")
            .replace("{version}", env!("CARGO_PKG_VERSION")),
    }
}

/// 项目指令（`AGENTS.md` 这一类）的包法（`core/project-docs/`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectDocTexts {
    /// 一份文件的外壳头：字段 `path`。
    pub open: String,
    /// 外壳尾。
    pub close: String,
    /// 文件改了、换一份时头一句：字段 `root`。
    pub replaced: String,
    /// 超了总上限、截断了的那一句：字段 `limit`。
    pub cut: String,
}

/// 项目指令的包法。
pub fn project_doc_texts() -> ProjectDocTexts {
    ProjectDocTexts {
        open: include_str!("../../../resources/core/project-docs/open.txt").to_string(),
        close: include_str!("../../../resources/core/project-docs/close.txt").to_string(),
        replaced: include_str!("../../../resources/core/project-docs/replaced.txt").to_string(),
        cut: include_str!("../../../resources/core/project-docs/cut.txt").to_string(),
    }
}

/// 文件记忆的目录那一块（`core/memory/`，L6，设计第五节第 7 条）：`index` 是外壳和用法的说明，字段 `folder`、`index`；`empty`
/// 是目录里还没有一行时 `index` 换进去的那一句。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemoryTexts {
    /// 外壳和用法的说明。
    pub index: String,
    /// 还没有一行时。
    pub empty: String,
}

/// 文件记忆的目录那一块的字。
pub fn memory_texts() -> MemoryTexts {
    MemoryTexts {
        index: include_str!("../../../resources/core/memory/index.txt").to_string(),
        empty: include_str!("../../../resources/core/memory/empty.txt").to_string(),
    }
}

#[cfg(test)]
mod tests;
