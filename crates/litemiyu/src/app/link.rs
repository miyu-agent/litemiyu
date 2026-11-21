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
    /// 还没开的下一个会话定好了用哪个预设（`--preset`、`/new <名字>`、`/preset` 里选的，LM-29）：预设的名字；没定的是
    /// `None`，照配置里的默认预设。
    pub next_preset: Option<String>,
    /// 默认预设不是全部功能时它的名字（LM-29）：启动时照已经读进来的 `config.toml` 记下（`settings/startup.rs`），配置界面
    /// 存了的、`/preset` 里改了的跟着换；首页照它写新会话会用哪个预设。全部功能、不知道的是 `None`。
    pub default_preset: Option<String>,
}
