//! 会话的限额：窗口、压缩线。会话照它的模型算好交过来（`lm_kernel::kernel::session::ContextLimits`），界面照它画上下文
//! 的窗口，不自己照模型名查。

use lm_kernel::kernel::session::ContextLimits;

/// 会话的限额。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Limits {
    /// 上下文窗口，token；不知道的是 `None`。
    pub window: Option<u64>,
    /// 到多少 token 自动压缩；不知道的是 `None`。
    pub compaction_line: Option<u64>,
}

impl Limits {
    /// 照会话交来的读。
    pub fn of(limits: &ContextLimits) -> Self {
        Self {
            window: limits.window,
            compaction_line: limits.compaction_line,
        }
    }
}
