//! 基础系统（`docs/designs/10-自带软件.md` 第三节，从 Miyu `miyu-basesystem` 搬来，L3 四）：随程序附带的几件工具，软件包的
//! 编号是 `basesystem`。名字、参数、输出照成熟 harness 的规范，以 Claude Code 为主（`10-自带软件.md` 第十节）。
//!
//! 工具的说明、参数格式、输出里给她看的几句在 `resources/software/basesystem/` 下（`26-提示词.md` 第八节），编进程序
//! （[`crate::tool`] 的 `shipped`）。一件一件搬：现在 [`tools`] 里有读的 `read`（施工 4-4 上），和写的 `write`（施工 4-6 上）、
//! `edit`（施工 4-6 中），执行命令的 `shell`（施工 4-8，前台的），问人的 `ask_user`（施工 D-2），
//! 查这个会话的 token 和上下文的 `session_usage`（施工 8-15），
//! 翻这个会话自己的记录的 `history`（施工 6-4），看后台命令、读输出、停掉的 `jobs`（施工 7-4），派子代理的 `subagent`（施工 7-5）、父子之间留言的 `send_message`（施工 7-7）。别的照设计第三节的清单接着搬
//! （执行计划第六节 L3 四）；Miyu 的包管理（`packages`、`package`）、脚本当工具（`shell/script`）、按包分组不搬。

mod ask_user;
mod blocking;
mod common;
mod edit;
mod history;
mod jobs;
mod load;
mod read;
mod send_message;
mod session_usage;
mod sessions;
mod shell;
mod subagent;
mod text;
mod write;

use std::sync::Arc;

use crate::tool::{Catalog, Tool};

pub use load::LoadError;
/// 主程序没了它起的进程跟着没的看门的（`shell/lifeline.rs`）：MCP 服务也起在看门的组里（L5 六，`crate::mcp`）。
pub(crate) use shell::lifeline::{Watcher, watcher};

/// 基础系统的每件工具：照编进程序的字造好。
///
/// # Errors
///
/// 哪一份字读不出来、写法不对，说是哪一份（安装坏了才会有，测试守着出厂的都读得进）。
pub fn tools() -> Result<Vec<Arc<dyn Tool>>, LoadError> {
    let common = common::Common::load()?;
    Ok(vec![
        Arc::new(read::Read::load(common.clone())?),
        Arc::new(write::Write::load(common.clone())?),
        Arc::new(edit::Edit::load(common.clone())?),
        Arc::new(shell::Shell::load(common.clone())?),
        Arc::new(history::History::load(common.clone())?),
        Arc::new(subagent::Subagent::load(common.clone())?),
        Arc::new(jobs::Jobs::load(common.clone())?),
        Arc::new(send_message::SendMessage::load(common.clone())?),
        Arc::new(sessions::Sessions::load(common.clone())?),
        Arc::new(session_usage::SessionUsage::load()?),
        Arc::new(ask_user::AskUser::load(common)?),
    ])
}

/// 会话的工具目录（执行计划第六节 L3 四）：基础系统的每件工具，照 [`tools`] 造。L5 起 MCP 的工具也登记进来。
///
/// # Errors
///
/// 同 [`tools`]。
///
/// # Panics
///
/// 实际不会：基础系统的工具名各不相同、名字和参数格式的写法都对，测试守着（`catalog_takes_every_tool`）。
pub fn catalog() -> Result<Catalog, LoadError> {
    Ok(Catalog::new(tools()?).expect("基础系统的工具名各不相同、写法都对"))
}

#[cfg(test)]
mod tests;
