//! 这时的策略快照（Miyu 的 `actor/persona.rs` 里 `Refresh` 那一份：快照和存它的 blob）：回合开始换 MCP 工具面（`mcp.rs`）、
//! 换压缩触发线、给头拆上下文明细（`parts.rs`）都照它。造会话、载入时交；换了的照新的改。

use lm_kernel::policy::Snapshot;

use crate::session::actor::Actor;
use crate::store::blob::Blobs;

/// 这时的策略快照和存新快照的地方。
pub(crate) struct PolicyKit {
    /// 这时的快照：换的时候照它改。
    pub(crate) snapshot: Snapshot,
    /// 新快照存在这里。
    pub(crate) blobs: Blobs,
}

impl Actor {
    /// 交上这时的策略快照：造会话、载入时。
    pub(crate) fn set_kit(&mut self, kit: PolicyKit) {
        self.kit = Some(kit);
    }
}
