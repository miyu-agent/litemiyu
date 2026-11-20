//! 被忽略的三样（LM-2 起）：配置里被忽略的项、被跳过的 MCP 服务、读不了的技能。LM-1 去掉运行日志以后这三样完全无声：
//! 写错的配置项照没写办、写错的 MCP 服务不起、开头写错的技能不列，人看不出哪里不对。主程序汇成一条界面提示（`litemiyu` 的
//! `app/checkup.rs`）。
//!
//! 交回的是名字（LM-3：原来只数个数，人看了提示不知道是哪一个）：配置项的键、MCP 服务的名字、技能的文件夹名，照在文件里、
//! 目录里的先后。别的原来记日志的错（密钥文件权限太松、引用的环境变量没设、模型池里有成员被跳过……）不在这里。
//!
//! 什么时候知道（LM-3补，2026-11-19 拍板）：不另读一遍，引擎本来要读的时候顺手记下。原来主程序起来时另起一条线程整份
//! 读配置、解析 TOML、扫技能目录，那些代码页一直算在空闲的占用里（干净配置也多约 230 kB）。现在：
//!
//! - 配置项：引擎第一次整份读配置时（[`crate::configured::load`] 交回的 `ignored`）；
//! - MCP 服务、技能：造会话时、载入的会话第一次开回合时（`crate::mcp::Servers::read`、`crate::skills::look`），记进会话的
//!   [`crate::session::Handle::ignored`]。

/// 被忽略的几样，各是哪几个。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ignored {
    /// 配置项：`config.toml` 里不认识的、写错的项的键（原样），`secrets.toml` 里写错的那一行的名字。
    pub settings: Vec<String>,
    /// MCP 服务：`config.toml` 里写错了、整个不收的 `[mcp.<名字>]` 的名字；`mcp` 整个没写成表的是 `mcp`。
    pub mcp: Vec<String>,
    /// 技能：开头读不了的、撞了名的、链接指进 LiteMiyu 自己目录的，`skills/` 下那个文件夹的名字。
    pub skills: Vec<String>,
}

impl Ignored {
    /// 三样都没有。
    pub fn is_empty(&self) -> bool {
        self.settings.is_empty() && self.mcp.is_empty() && self.skills.is_empty()
    }
}

/// 一条配置问题说的是哪一项：写错的键原样；整份文件的问题（没有键）记那一份文件的名字 `file`。
pub(crate) fn setting_name(problem: lm_kernel::config::problem::Problem, file: &str) -> String {
    problem.key.unwrap_or_else(|| file.to_string())
}

#[cfg(test)]
mod tests;
