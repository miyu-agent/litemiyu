//! 应急模式造的几样（设计第五节第 16 条，`crate::rescue`）：配置照进界面以前读好的那一份；工具只带文件和命令那几件；
//! 会话照 `lm_engine::session::create_rescue` 开，什么都不写盘。
//!
//! 全屏界面另有两样（`/model`、`/loop` 要的）：模型目录只读缓存里现成的（`lm_engine::models::read_only`：不联网、不写
//! 缓存，没有的照没有目录办）；工具加上循环的 `schedule_wakeup`（循环和定时触发只是会话里的一个定时器，不写盘）。没人看着的
//! 一次运行（`litemiyu ask --rescue`）没有这两样：没有目录（`lm_engine::models::without_catalog`），工具只有那五件。

use std::path::PathBuf;
use std::sync::Arc;

use lm_engine::tool::Catalog;

use super::Ready;
use crate::rescue::Prepared;

/// 带的工具（基础系统里文件和命令的那几件）：读、写、改、跑命令、看后台命令。没带的：`ask_user`、`session_usage`、
/// `history`、`sessions`、`send_message`、`subagent`，MCP。
pub(super) const TOOLS: &[&str] = &["read", "write", "edit", "shell", "jobs"];

impl Ready {
    /// 照应急模式备好的造；`unattended` 是没人看着的一次运行（`litemiyu ask --rescue`）。
    ///
    /// # Errors
    ///
    /// 出厂的档案和工具的字读不进来：一句给人看的原因。
    pub(super) fn rescued(prepared: Prepared, unattended: bool) -> Result<Self, String> {
        let Prepared { paths, rescued, .. } = prepared;
        let loaded = rescued.loaded;
        // 路由不带写盘的那两处：用出来的、记下的只留在内存里。
        let routes = lm_engine::models::prepare(None)?;
        let dirs = lm_engine::session::ModelDirs {
            state: paths.model_state(),
            cache: paths.model_cache(),
        };
        let mut tools = lm_engine::basesystem::tools().map_err(|error| error.to_string())?;
        tools.retain(|tool| TOOLS.contains(&tool.spec().name.as_str()));
        if unattended {
            lm_engine::models::without_catalog(&routes.data);
        } else {
            lm_engine::models::read_only(Arc::clone(&routes.data), dirs, loaded.resolved);
            // 工具面上有 `schedule_wakeup` 会话才认 `/loop`。
            tools.extend(lm_engine::schedule::tools().map_err(|error| error.to_string())?);
        }
        let tools = Catalog::new(tools).map_err(|error| format!("{error:?}"))?;
        Ok(Self {
            paths,
            configs: loaded.configs,
            updates: loaded.updates,
            routes,
            tools: Arc::new(tools),
            home: std::env::var_os("HOME").map(PathBuf::from),
            rescue: true,
            // 应急模式不提示被忽略的几样（`app/checkup.rs`）。
            ignored: None,
            catalog: None,
        })
    }
}
