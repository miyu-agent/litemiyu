//! 会话钉着哪个预设，交给界面看的样子（LM-29，`docs/00-设计.md` 第六节第 14 条）：侧边栏会话那一段写名字；预设钉在会话上
//! 以后配置里的那一张改了、删了，会话照造它时记下的走，名字后面写「已修改」「已删除」。

/// 正在说话的会话钉着的预设。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pinned {
    /// 预设的名字：自带的是 `full`、`bare`（界面写成「全部功能」「无功能」）。
    pub name: String,
    /// 和这一刻配置里同名的那一张比。
    pub state: PinState,
}

/// 钉着的那一张和配置里现在的比。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PinState {
    /// 一样（自带的两个一直是它）。
    Same,
    /// 配置里的改过了：会话照旧用造它时的那一张。
    Changed,
    /// 配置里没有这个名字了：会话照旧用造它时的那一张。
    Gone,
}
