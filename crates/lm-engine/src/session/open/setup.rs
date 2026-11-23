//! 造会话、载入要交进来的几样（施工 8-8 从 `open.rs` 挪来：那一页长过了行数的上限）。Miyu 这里还交人格、预设、场所、权限、
//! 沙盒、会话表、软件包、定时、扩展、索引、用量、知识库、向量、技能、记忆（L1、L3 二删；会话表、技能在 L5 搬回来）。

use std::path::Path;
use std::sync::Arc;

use lm_kernel::kernel::facts::Environment;
use lm_kernel::kernel::id::{CommandId, SessionId};
use lm_kernel::kernel::origin::By;
use lm_kernel::kernel::time::UtcOffset;

use crate::session::config::Configs;
use crate::session::port::Models;
use crate::store::paths::Paths;
use crate::tool::Catalog;

/// 造一个会话要的。
pub struct Create<'a> {
    /// 数据放在哪：会话目录、blob，提示词照它的配置目录读（`prompt.md`）。
    pub root: &'a Paths,
    /// 会话编号，照 [`crate::session::new_id`] 造。
    pub id: SessionId,
    /// 有没有人能确认（`02-内核.md` 第六节「确认怎么走」第 2 条）。
    pub attended: bool,
    /// 一次性的（`22-命令行.md` O2，施工 3-9 下）。
    pub oneshot: bool,
    /// 会话所在的环境：时区、工作目录。
    pub environment: Environment,
    /// 造会话的那个命令的编号：`session.created` 的 `cause`。
    pub command: CommandId,
    /// 谁发的造会话。
    pub by: By,
    /// 给会话造请求模型的端口：驱动的占位取自这个会话的策略快照。
    pub models: &'a dyn Models,
    /// 工具目录：造会话时照它定工具面，执行工具时照它找。
    pub tools: Arc<Catalog>,
    /// 系统的家目录：权限策略照它换 `~`（施工 4-3 下）。读不出来的是空的。
    pub home: Option<&'a Path>,
    /// 从哪取配置（施工 8-4）：回合开始时照它冻结这一轮的配置。测试里给 [`crate::session::fixed`] 的一份。
    pub configs: Configs,
    /// 会话用哪个模型（施工 8-8）：模型或 `@池`。没要的照这时的 `models.chat`。记进 `session.created` 的 `model`。
    pub model: Option<String>,
    /// 无功能的一档（设计第五节第 14 条）：一件工具都不给，回合开始什么都不注入；记进快照、`session.created` 的 `no_tools`。
    pub no_tools: bool,
    /// 自定义的预设（设计第六节第 14 条，LM-29，`crate::presets`）：工具面、技能、MCP 服务照这张名单筛，记进快照。没有的是
    /// 自带的两个预设（全部功能，或者 `no_tools` 的无功能）。子会话不看这一格，照主会话的。
    pub preset: Option<lm_kernel::policy::PresetPin>,
    /// 调的一方另交进来的 MCP 服务（`litemiyu acp`：编辑器在开会话时带来的），已经照配置的规矩查过。排在配置里的后面，
    /// 和配置里的重名的不要；开关、预设照样管着；不读、不写目录缓存（[`crate::mcp::Servers`]）。只有主会话看这一格，
    /// 子会话用主会话那一份。
    pub handed: Vec<lm_kernel::config::mcp::McpServer>,
}

/// 照最后一次记下的工作目录（没有的是空的）定实际在哪干活。
pub type Pick<'a> = Box<dyn FnOnce(Option<&str>) -> String + Send + 'a>;

/// 载入的会话在哪干活（施工 V-2 三补）。
pub enum Workplace<'a> {
    /// 给定的：时区、工作目录、加进来的目录。
    Given(Environment),
    /// 照日志里最后一次记下的：载入读日志时顺手认出最后一次记下的工作目录（都没记下的是空的）、加进来的目录，交给 `pick`
    /// 定实际在哪干活。
    Remembered {
        /// 时区。
        offset: UtcOffset,
        /// 照最后一次记下的工作目录定实际在哪干活。
        pick: Pick<'a>,
    },
}

/// 载入一个会话要的。
pub struct Load<'a> {
    /// 数据放在哪。
    pub root: &'a Paths,
    /// 会话编号。
    pub id: SessionId,
    /// 在哪干活。
    pub place: Workplace<'a>,
    /// 给会话造请求模型的端口。
    pub models: &'a dyn Models,
    /// 工具目录：执行工具时照它找（工具面照快照里的）。
    pub tools: Arc<Catalog>,
    /// 系统的家目录。
    pub home: Option<&'a Path>,
    /// 从哪取配置。
    pub configs: Configs,
    /// 调的一方另交进来的 MCP 服务（同 [`Create`] 的那一格）：这一次运行里用的，日志里不记，下次载入照下次交的。
    pub handed: Vec<lm_kernel::config::mcp::McpServer>,
}
