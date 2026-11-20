//! 无功能那一档的快照（`docs/00-设计.md` 第五节第 14 条）：一件工具都不给，回合开始什么都不注入（环境那几块、AGENTS.md、
//! 技能目录、记忆、MCP），就是和模型干聊。自己设的提示词（system）照留；压缩、回顾、起标题照旧。做法是 Miyu 整理会话的裸快照
//! （施工 D-1 上）改的：那一份连 system 都去掉、不压缩，是给一次性的整理请求用的。

use crate::policy::Snapshot;

impl Snapshot {
    /// 照这一份造无功能那一档：随核心附带的字、system、压缩、回顾、标题照留，工具和要注入的几段都去掉。
    #[must_use]
    pub fn bare(mut self) -> Snapshot {
        self.no_tools = true;
        self.tools.clear();
        self.jobs = None;
        self.peers = None;
        self.skills = None;
        self.profile = None;
        self
    }
}

#[cfg(test)]
mod tests;
