//! 退役的键（施工 FL-1）：以前登记过、现在拆掉了的配置项。写过它们的配置照旧读得进：不进最终值，也不报 `unknown_key`，
//! 免得每次查配置都多一条警告。新写的（`config.set`）照不认识的拒。
//!
//! - `providers.<id>.compat.strict_tools`、`loading.tools.<name>`、`loading.mcp.<id>`：工具一律全量以后不起作用了（设计
//!   `25-工具加载.md` W10，2026-10-27 项目主人定）。
//! - `memory.merge_sessions`：后台整理不再要凑够几个会话，只看离上次多久（施工 E-9，2026-11-02 项目主人定）。
//! - `net.search.*`：搜索来源、几家的 key、SearXNG 的地址（LM-2 砍了 `web_search`，2026-11-18 项目主人定）。
//! - `pools`（整张：自己建的池、固定的三个池的成员、分法、子代理能选、说明）、`models.cooldown`（整张：四类冷却）、
//!   `providers.<id>.cache`（只用来定池不写分法时怎么分）：LM-8 砍了模型池（2026-11-19 项目主人定）。值写成什么样都整张不往里走。

use crate::config::key::{Fit, fit};

/// 退役的几样，写法同配置项的键：占位照 [`crate::config::key`] 的规矩。
const RETIRED: &[&str] = &[
    "providers.<id>.compat.strict_tools",
    "loading.tools.<name>",
    "loading.mcp.<id>",
    "memory.merge_sessions",
    "net.search.source",
    "net.search.exa.key",
    "net.search.deepseek.key",
    "net.search.tavily.key",
    "net.search.brave.key",
    "net.search.searxng.url",
    "pools",
    "models.cooldown",
    "providers.<id>.cache",
];

/// 照段拆好的键 `segments` 是不是退役了的。
pub(crate) fn retired(segments: &[String]) -> bool {
    RETIRED
        .iter()
        .any(|pattern| matches!(fit(pattern, segments), Fit::Yes(_)))
}

#[cfg(test)]
mod tests;
