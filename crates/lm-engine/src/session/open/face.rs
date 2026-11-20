//! 造会话时定的工具面（施工 7-5 起，Miyu 的 `Agents::face`）：目录里每件工具的规格换成快照里的写法，一个会话里不变。
//!
//! 给哪种会话照工具自己声明的范围（施工 XG-2 上，`crate::tool::Scope`）：`local` 的都给；`local_main` 的只给主会话；
//! `local_main_attended`（`ask_user` 这一类）给有人能确认的主会话；`spawn`（派子代理）给有会话表、没到深度上限的
//! （`Agents::allowed`）。（原来 `subagent` 的 `pool` 照这时的配置填上能选的池，LM-8 砍了模型池跟着没了。）
//! Miyu 这里还照场所、群、记忆、知识库、预设、脚本调工具、人格、覆盖的说明筛和改（L1、L3 二删）。

use lm_kernel::policy::ToolEntry;

use crate::tool::{Catalog, Scope, Spec};

/// 这个会话是哪一种：工具面照它给。
pub(super) struct Kind {
    /// 有没有人能确认。
    pub(super) attended: bool,
    /// 是主会话（不是子会话）。
    pub(super) main: bool,
    /// 能派子代理：有会话表、没到深度上限。
    pub(super) spawns: bool,
}

/// 工具目录 `tools` 给这种会话 `kind` 的那一面。
pub(super) fn face(tools: &Catalog, kind: &Kind) -> Vec<ToolEntry> {
    let in_scope = |scope: Scope| match scope {
        Scope::Any | Scope::Local => true,
        Scope::LocalMain => kind.main,
        Scope::LocalMainAttended => kind.main && kind.attended,
        Scope::Spawn => kind.spawns,
    };
    tools
        .specs()
        .filter(|spec| in_scope(spec.scope))
        .map(entry)
        .collect()
}

/// 一件工具的规格在快照里的写法（MCP 的工具回合开始换工具面时也照它写，L5 六）。
pub(crate) fn entry(spec: &Spec) -> ToolEntry {
    ToolEntry {
        name: spec.name.clone(),
        description: spec.description.clone(),
        parameters: spec.parameters.clone(),
        access: spec.access.clone(),
        role: spec.role,
    }
}
