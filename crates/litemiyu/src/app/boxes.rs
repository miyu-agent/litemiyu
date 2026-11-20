//! 开着的框（蓝图 `app.md` 的 `boxes` 块）：贴在输入框上面的面板。Miyu 这里还有会话列表、`/model`、`/usage` 这些框和
//! 配置页、第一次打开的引导（L7、L8 跟着功能搬回）。

/// 输入框上面那个框里开着的。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Panel {
    /// 帮助（`/help`）：滚到第几行。
    Help {
        /// 第一行是第几行。
        scroll: usize,
    },
    /// 后台面板（`jobs.rs`）：选中、展开的在 [`Boxes::jobs`]。
    Jobs,
    /// 会话列表（`/sessions`，`sessions.rs`）：列表在 [`Boxes::sessions`]。
    Sessions,
    /// `/model`（`models.rs`）：列表在 [`Boxes::model_list`]。
    Models,
    /// `/effort`（`effort.rs`）：选中第几行（第 0 行跟随设置），几级在 [`Boxes::efforts`]。
    Effort {
        /// 选中第几行。
        selected: usize,
    },
    /// `/status`（`status.rs`）：滚到第几行。
    Status {
        /// 第一行是第几行。
        scroll: usize,
    },
}

/// 开着的框。
#[derive(Default)]
pub struct Boxes {
    /// 开着的面板。
    pub panel: Option<Panel>,
    /// 后台面板选中、展开的那一条。
    pub jobs: super::jobs::JobsView,
    /// 开着的会话列表（`/sessions`）。
    pub sessions: Option<crate::session_list::SessionList>,
    /// 开着的 `/model` 框。
    pub model_list: Option<super::model_list::ModelList>,
    /// `/effort` 框改哪个模型、有哪几级：引擎交回来以前是 `None`。
    pub efforts: Option<crate::core::Efforts>,
}
