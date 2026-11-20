//! 拼策略快照（施工 3-6 上起在 `open.rs`，施工 P-1 再补挪出来）：提示词接上随核心附带的字、工具面（`lm_engine::resources`
//! 编进程序的那几份，`prompt.md` 照配置目录读），再接核心的几行（`docs/designs/26-提示词.md` 第四节）。核心的
//! 几行里本机是什么系统照 [`crate::store::os::os_name`] 写（施工 2-7 再补）。子会话在核心的几行前面接上子代理的说明（L5 二）。
//! Miyu 这里还接人格、场所、群、「关于你」、项目、
//! 风格锁、技能、知识库、扩展追加的几段，并量 system 一块块在哪给 `/context` 看（L1、L3 二删；技能在 L5 搬回来）。

use std::path::Path;

use lm_kernel::policy::steps::StepLimits;
use lm_kernel::policy::{Snapshot, ToolEntry, compose};

/// 拼一份快照的料。
pub(crate) struct Parts {
    /// 有没有人能确认。
    pub(crate) attended: bool,
    /// 工具面。
    pub(crate) face: Vec<ToolEntry>,
    /// 无功能的一档（设计第五节第 14 条，`lm_kernel::policy::Snapshot::bare`）：拼好以后去掉 system、工具和几样开关。
    pub(crate) bare: bool,
    /// 步数上限（施工 AE-1，`lm_kernel::policy::steps`）。
    pub(crate) steps: StepLimits,
    /// 子会话（L5 二）：system 末尾接上子代理的那一段说明（`agents.md` 第九条第 3 条）。
    pub(crate) child: bool,
    /// 她用得上的技能那一块（L5 五，`crate::skills::listing`）：没有的不写。
    pub(crate) skills: Option<String>,
    /// 接在 system 尾部的 MCP 服务的说明（L5 六，`crate::mcp::Servers::appended`）：没有的不写。
    pub(crate) appended: std::collections::BTreeMap<String, String>,
}

/// 照 `parts` 拼一份快照，提示词照配置目录 `config` 里的 `prompt.md`。
pub(crate) fn build(config: &Path, parts: Parts) -> Snapshot {
    let sources = crate::resources::sources(config);
    let mut snapshot = compose(sources, parts.attended)
        .with_skills(parts.skills)
        .with_tools(parts.face)
        .with_steps(parts.steps);
    if parts.child {
        snapshot = snapshot.with_venue(crate::resources::subagent_venue());
    }
    let lines = crate::resources::core_lines();
    let snapshot = snapshot
        .with_core_lines(&lines, crate::store::os::os_name())
        .with_appended(parts.appended);
    match parts.bare {
        true => snapshot.bare(),
        false => snapshot,
    }
}
