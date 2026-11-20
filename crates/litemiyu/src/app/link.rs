//! 连接（蓝图 `app.md` 的 `link` 块）：交给引擎的一头，和给人看的字（工具的显示名、说法）。

use crate::core::Core;
use crate::human::Human;

/// 连接。
pub struct Link {
    /// 交给引擎线程的一头。
    pub core: Core,
    /// 给人看的字：第一次开会话时照界面语言读（`human.rs`）；还没读的是空的。
    pub human: Human,
    /// 新会话默认的聊天模型（`models.chat`）：启动时照已经读进来的 `config.toml` 记下（`settings/startup.rs`），`/model`
    /// 选了的、配置界面存了的跟着换；还没开会话时底栏照它写（Miyu 终端 `link.default_chat`）。没配的是 `None`。
    pub default_chat: Option<String>,
}
