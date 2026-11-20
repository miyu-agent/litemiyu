//! 应急模式造的几样（设计第五节第 16 条，`crate::rescue`）：配置照进界面以前读好的那一份，没有模型目录（LM-9：程序不带目录，
//! 应急模式不读不写缓存、不联网，`lm_engine::models::without_catalog`），工具只带文件和命令那几件；会话照
//! `lm_engine::session::create_rescue` 开，什么都不写盘。

use std::path::PathBuf;
use std::sync::Arc;

use lm_engine::tool::Catalog;

use super::Ready;
use crate::rescue::Prepared;

/// 带的工具（基础系统里文件和命令的那几件）：读、写、改、跑命令、看后台命令。没带的：`ask_user`、`session_usage`、`history`、`sessions`、`send_message`、`subagent`，循环，MCP。
pub(super) const TOOLS: &[&str] = &["read", "write", "edit", "shell", "jobs"];

impl Ready {
    /// 照应急模式备好的造。
    ///
    /// # Errors
    ///
    /// 出厂的档案和工具的字读不进来：一句给人看的原因。
    pub(super) fn rescued(prepared: Prepared) -> Result<Self, String> {
        let Prepared { paths, rescued, .. } = prepared;
        let loaded = rescued.loaded;
        let routes = lm_engine::models::prepare(None)?;
        lm_engine::models::without_catalog(&routes.data);
        let tools = lm_engine::basesystem::tools()
            .map_err(|error| error.to_string())?
            .into_iter()
            .filter(|tool| TOOLS.contains(&tool.spec().name.as_str()))
            .collect::<Vec<_>>();
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
