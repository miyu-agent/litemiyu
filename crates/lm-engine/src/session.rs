//! 会话 actor（`docs/designs/02-内核.md` 第七节「会话 actor 怎么跑」，施工 3-7 中）：把内核的会话
//! 状态机接上磁盘。
//!
//! 一个会话一个异步任务。人的命令、执行器的回报都进它的收件箱，一条一条送进内核；内核交出的动作
//! 照 02 第四节「执行器怎么回动作」的表回。追加的事件写进会话日志、同步到磁盘了，才回应命令、
//! 才推给订阅的头（`07-存储.md` S4）。
//!
//! - [`create()`]：造一个会话，拼好策略快照存下；
//! - [`load()`]：从磁盘载入一个会话，照快照重建策略；
//! - [`Handle`]：发命令、订阅、有计划地停下；[`Backlog`]：订阅时要补发的那一截（施工 3-8 六补）；[`Shown`]：给头看的限额和
//!   会话接下来请求的模型（[`Next`]，施工 8-10）；
//! - [`Models`]、[`ModelPort`]：给会话造请求模型的端口，和端口本身，测试里照剧本回（`testkit`）；照配置挑供应商的路由在 L3 三搬；
//! - [`new_id`]：新的会话编号；
//! - [`Configs`]、[`ConfigSource`]：会话从哪取配置，回合开始时冻结一份（[`TurnConfig`]），这一轮的请求都照它（施工 8-4），
//!   连同取 key 的办法（[`Turn`]，施工 8-6）。
//!
//! Miyu 这里还有子代理和会话表、跨会话、后台任务、长期目标、记忆、知识库、技能、用量（L3 二删，L5、L7 从 Miyu 搬回；
//! 长期目标 2026-11-14 砍了）。

mod actor;
mod agents;
mod backlog;
pub(crate) mod blocking;
mod clock;
mod config;
mod current;
mod effects;
mod guard;
mod handle;
mod job_ids;
mod jobs;
mod kinds;
mod lettering;
mod lines;
mod looping;
mod memory;
mod messages;
mod open;
mod path_text;
mod peers;
mod pictures;
mod port;
mod project_docs;
mod report;
mod reread;
mod route;
pub mod settings;
mod shown;
mod snapshot;
mod spawn;
mod store;
mod table;
#[cfg(feature = "testkit")]
pub mod testkit;
mod tools;
mod usage;
mod window;

pub use backlog::Backlog;
pub use clock::{new_command_id, new_id};
pub use config::{ConfigSource, Configs, Turn, TurnConfig, fixed, fixed_with};
pub use current::{Current, Tally};
pub use handle::{
    Attach, AttachError, AttachSource, Attached, Charged, Context, ContextRefused, Ended, Handle,
    LineRefused, Meter, Pushed, Rejoin, Retitled, Spent, Stopped, Subscription, SystemPart,
    about_tokens, parse_context,
};
pub use jobs::Unreadable;
pub use looping::{Looping, Pace};
pub use open::{
    Create, CreateError, Load, LoadError, Pick, Placed, Workplace, create, create_rescue, load,
    load_placed,
};
pub use path_text::path_text;
pub use port::{Cancel, ForSession, ModelPort, Models, Reports, Sight};
pub use route::{
    CatalogReader, Holding, IDLE, ModelData, ModelDirs, Observed, RECALL_CAP, RECALL_FILE, Routes,
    read_observed,
};
pub use shown::{Next, Shown};
pub use window::{
    Asked as HandoverAsked, Found, HANDOVER_WAIT, Listed, NotRemoved, ask_handover,
    find as find_session, list as list_sessions, remove as remove_session,
};
