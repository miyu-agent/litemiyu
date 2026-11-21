//! 工具的接口和工具目录（Miyu 的 `docs/designs/05-内核接口.md` 第六节「工具的规格」、第八节「启用、注册与
//! 目录快照」，施工 4-1；从 `miyu-tool` 搬来）。
//!
//! 工具是内核之外的软件（`10-自带软件.md` B1）：每件工具报出自己的规格，核心起来时登记进工具目录，
//! 登记完就冻结。造会话时，会话照目录把工具面存进策略快照（`03-事件模型.md` E5），以后一直照快照发。
//!
//! - [`Spec`]：一件工具的规格，第一批四格；
//! - [`Tool`]：一件工具：报规格，报一次调用要碰的路径（[`Target`]，施工 4-3 下），执行一次调用（[`Call`] 进、
//!   [`Done`] 出，施工 4-2）；工具执行时经 [`Call::real`] 换路径，换出来的要是权限策略判过的（[`Unchecked`]，施工 5-18）；
//! - [`Catalog`]：工具目录，登记时查重名、名字和参数格式的写法；
//! - [`Log`]：这个会话日志的只读入口（施工 6-4），`history` 用；
//! - [`AgentPort`]：派子代理的端口（施工 7-5），`subagent` 用；
//! - [`MessagePort`]：发话的端口（施工 7-7、C-5），`send_message` 用；
//! - [`JobPort`]：任务端口（施工 7-3），`shell` 把起好的后台命令交给它，`jobs` 经它查、停（施工 7-4）；
//! - [`SessionsPort`]：列会话、只读地开别的会话的日志（施工 C-3、C-4），`sessions`、`history` 用；[`find_session`]
//!   认她写的会话编号；
//! - [`UsagePort`]：查这个会话的用量、上下文（施工 8-15），`session_usage` 用；
//! - [`picture`]：什么算一张图（施工 4-13 定，3-9 三补挪来）：`read` 读到的、人附上的，都照它认。
//! - [`attachment`]：一个附件是什么（施工 3-9 三补定，C-9 挪来）：人附上的、她带给别的会话的，都照它认。

mod agents;
pub mod attachment;
mod caller;
mod catalog;
mod checked;
pub mod detail;
mod jobs;
pub mod load;
mod log;
mod messages;
mod person;
pub mod picture;
mod questions;
mod run;
mod schedules;
mod sessions;
mod settings;
pub(crate) mod shipped;
mod stop;
#[cfg(feature = "testkit")]
pub mod testkit;
mod usage;

pub use agents::{AgentPort, NotSpawned, Order, SUBAGENT, Spawned, Spawning, is_subagent};
pub use caller::{Caller, InUse};
pub use catalog::{
    BASESYSTEM, Catalog, CatalogError, MCP_PREFIX, Problem, mcp_prefix, mcp_service,
};
pub use checked::Unchecked;
pub use jobs::{
    Asking, BACKGROUND, Background, Exit, JobError, JobPort, Listed, Output, Process,
    RUN_IN_BACKGROUND, SHELL, Text,
};
pub use log::{Log, ReadLog};
pub use messages::{Delivered, MessagePort, NotSent, Recipient, SEND_MESSAGE, Sending};
pub use person::{Halt, PersonCommand, PersonEnd, PersonRun, PersonRunning};
pub use questions::{ASK_USER, Answering, QuestionPort};
pub use run::{Call, Done, Effect, Hazard, Own, Picture, Progress, Running, Seen, Target};
pub use schedules::{
    SCHEDULE_WAKEUP, ScheduleAnswer, ScheduleAnswering, ScheduleAsk, SchedulePort, ScheduleRefused,
    WAKEUP_MIN,
};
pub use sessions::{
    Found, Listing, MainSession, Opening, SESSIONS, SessionsPort, find_session, reaches,
};
pub use settings::SettingsPort;
pub use stop::Stop;
pub use usage::{ContextUse, SESSION_USAGE, Spent, UsagePort};

use std::collections::BTreeMap;

use lm_kernel::kernel::raw::RawJson;
/// 范围（施工 XG-2 上）：定义在内核里，和职能放在一起，这里照旧导出。
pub use lm_kernel::kernel::tool::Scope;
use lm_kernel::kernel::tool::{Access, Role};

/// 一件工具的规格（05 第六节）：第一批四格（施工 4-1）。显示名、摘要随施工 4-5；默认超时、场所、
/// 组，用得上时再加。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Spec {
    /// 工具名：英文，稳定不变，模型照它调。只用英文字母、数字、`_`、`-`，1 到 64 个字符。
    pub name: String,
    /// 给模型看的说明，英文，原样进 tools 数组。
    pub description: String,
    /// 参数的 JSON Schema，原样进 tools 数组，一个字节不改：必须是 `{"type":"object",…}`。
    pub parameters: RawJson,
    /// 访问类别：权限策略、能不能和别的一起跑，都看它。
    pub access: Access,
    /// 职能（施工 XG-2 上，`docs/designs/64-工具职能.md` 第四节）：核心要找「哪件是干这个的」时照它认，不照名字。没写的没有。
    pub role: Option<Role>,
    /// 范围（施工 XG-2 上，设计 64 第五节）：给哪种会话，组工具面时照它筛。只能收窄、不能放宽：别处原有的限制（子代理的
    /// 层数）照旧都算。不写的是 [`Scope::Any`]。
    pub scope: Scope,
}

/// 一件工具。
pub trait Tool: Send + Sync {
    /// 它的规格。
    fn spec(&self) -> &Spec;

    /// 给人看的名字，照语言（`zh`、`en`……；施工 9-8 六补）：MCP 的工具交，执行器派它那一刻取下、随结果记进日志，界面照
    /// 语言挑。自带的不交，照自带的字（`resources/core/human/`）。默认空。
    fn display_name(&self) -> BTreeMap<String, String> {
        BTreeMap::new()
    }

    /// 这次调用要碰的路径、是读是写：照参数算，不碰磁盘（施工 4-3 下）。换成真实的位置、查边界是执行前的
    /// 链的事。默认一条都没有，例如执行命令。参数不对的，也交回空的：执行时再报错。
    fn targets(&self, _call: &Call) -> Vec<Target> {
        Vec::new()
    }

    /// 这次调用认出来的高危（施工 5-17）：照参数算，不碰磁盘。高危的问人（说明多 `hazard`，不提规则），灭顶的直接拒
    /// （`docs/00-设计.md` 第五节第 6 条）。默认没有。
    fn hazard(&self, _call: &Call) -> Option<Hazard> {
        None
    }

    /// 问人时交给说明（`detail`）的几格，键和值（施工 D-4）：照参数算，例如 `shell` 的 `title`、`command`。权限策略并进
    /// 它自己写的那几格。参数不对的交回空的。默认空。
    fn asking(&self, _call: &Call) -> Vec<(&'static str, String)> {
        Vec::new()
    }

    /// 执行一次调用：执行器在它自己的任务里跑交回的 future，执行中的输出交给 `progress`。叫停有两种：
    /// 「叫它停」举 [`Call::stop`] 的旗，等它交回来（施工 4-9 再补一）；「掐掉」丢掉这个 future（施工 4-2）。
    fn run(&self, call: Call, progress: Progress) -> Running<'_>;

    /// 后台运行关着的会话里用的说明（施工 T-1 下）：在后台跑的工具换一种说法，`subagent` 有（派出去等它报回来）。别的没有，
    /// 照规格的。
    fn foreground_description(&self) -> Option<&str> {
        None
    }

    /// 给人跑一条命令（施工 SH-1 上，设计 `57-人在会话里跑命令.md` 第二节第 1 条，[`crate::tool::PersonCommand`]）：**只给人用**，
    /// 引擎只在人在输入框里用 `!` 跑命令时调，她的调用走 [`Tool::run`] 碰不到。照这件工具跑命令的程序、环境变量跑，不判权限、
    /// 不查高危、不拦干等、到时整组杀；`halt` 举起来时整组杀、交回到这时的输出。`shell` 实现它；默认没有，交回 `None`。
    fn run_for_person(
        &self,
        _command: crate::tool::PersonCommand,
        _halt: crate::tool::Halt,
    ) -> Option<crate::tool::PersonRunning<'_>> {
        None
    }

    /// 人在会话里跑命令（设计 57）交给它（施工 XG-1 下）：`!` 照这一条挑那一件，再调 [`Tool::run_for_person`]。`shell` 是；
    /// 默认不是。
    fn serves_person(&self) -> bool {
        false
    }

    /// 只在会话有后台时才给的那几个参数（施工 BS-1 上，设计 44 第三节第 3 条）：后台运行关着的会话，工具面上去掉它们。`shell`
    /// 的是 `run_in_background` 和 `watch`（施工 MN-1 上）。默认没有。
    fn background_parameters(&self) -> Vec<&str> {
        Vec::new()
    }
}
