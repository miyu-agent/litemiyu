//! 回合开始换 MCP 服务的工具（L5 六，设计第五节第 11 条；Miyu 是 `miyu-session` 的 `actor/persona.rs` 回合开始照目录重拼工具面、
//! `appended.rs` 跟着换说明）。工具面只在回合开始时换：
//!
//! - 第一次（载入的会话：造会话时已经读好了）照配置读这个主会话的服务（`crate::mcp::Servers`，在阻塞线程里），没缓存的在后台
//!   起一次列工具。
//! - 每一轮照各服务这时的工具拼执行器的目录（调得到这时列着的）；工具面上 MCP 的那几件和这时的不一样（服务说变了、这次列的和
//!   缓存不一样、头一次列完了），换一份快照：别的工具、system 原文不动，只换 MCP 的那几件和尾部的说明
//!   （`Snapshot::reappended`），存成 blob，交给内核换上，记一条 `session.policy_changed`。工具变了要断工具那一段以后的缓存，
//!   这是换工具面本来的代价；不变的什么都不记。
//! - 拼目录照这一轮的开关（[`Actor::switches`]，LM-36）：会话中途在 MCP 页关掉的服务，从下一个回合起工具面上没有它的工具和
//!   说明；读服务时就关着的不在这一份里，中途打开也要新开的会话才有（同 Miyu FX-8：开会话时没开的照旧不开）。

use std::sync::Arc;

use lm_kernel::kernel::id::ContentHash;
use lm_kernel::policy::ToolEntry;

use crate::mcp::Servers;
use crate::session::Configs;
use crate::session::actor::Actor;
use crate::session::blocking::blocking;
use crate::store::paths::Paths;
use crate::tool::{Catalog, MCP_PREFIX};

/// 一个会话换 MCP 工具要的。
pub(crate) struct McpKit {
    /// 这个主会话的服务（主会话、子会话共用一份，第一次要时读），连同第一次读时被忽略的几样（LM-3补）。
    pub(crate) extras: crate::session::table::Mcp,
    /// 读配置文件、缓存在哪。
    pub(crate) paths: Paths,
    /// 起服务时照它取密钥。
    pub(crate) configs: Configs,
    /// 主程序交的目录（没有 MCP 的）。
    pub(crate) shipped: Arc<Catalog>,
}

impl Actor {
    /// 交上换 MCP 工具要的：造会话、载入时，无功能的一档不交。
    pub(crate) fn set_mcp(&mut self, kit: McpKit) {
        self.mcp = Some(kit);
    }

    /// 回合开始：拼执行器的目录；工具面上 MCP 的那几件变了的，放好新的策略、交回新快照的哈希。没变的、没有 MCP 的没有。
    pub(super) async fn mcp_turn(&mut self) -> Option<ContentHash> {
        let kit = self.mcp.as_ref()?;
        let held = self.kit.as_ref()?;
        let switches = self.switches();
        let servers = match kit.extras.servers.get() {
            Some(servers) => Arc::clone(servers),
            None => {
                let switches = switches.clone();
                let (paths, configs) = (kit.paths.clone(), kit.configs.clone());
                let extras = Arc::clone(&kit.extras);
                let read = blocking(move || {
                    Arc::new(Servers::read(&paths, &switches, configs, extras.handed()))
                })
                .await;
                let servers = Arc::clone(kit.extras.servers.get_or_init(|| read));
                servers.warm();
                servers
            }
        };
        let catalog = servers.catalog(&kit.shipped, &switches);
        let now = entries(&catalog);
        let was: Vec<&ToolEntry> = held
            .snapshot
            .tools
            .iter()
            .filter(|tool| tool.name.starts_with(MCP_PREFIX))
            .collect();
        self.tools.set_catalog(Arc::new(catalog));
        if held.frozen || now.iter().eq(was.iter().copied()) {
            return None;
        }
        let face: Vec<ToolEntry> = held
            .snapshot
            .tools
            .iter()
            .filter(|tool| !tool.name.starts_with(MCP_PREFIX))
            .cloned()
            .chain(now)
            .collect();
        let appended = servers.appended(&face);
        let new = held.snapshot.clone().reappended(face, appended)?;
        let blobs = held.blobs.clone();
        let built = new.clone();
        let made = blocking(move || {
            let policy = built.policy().map_err(|error| error.to_string())?;
            let hash = blobs
                .put(&built.to_bytes())
                .map_err(|error| error.to_string())?;
            Ok::<_, String>((policy, hash))
        })
        .await;
        match made {
            Ok((policy, hash)) => {
                self.session.stage_policy(policy);
                if let Some(held) = self.kit.as_mut() {
                    held.snapshot = new;
                }
                Some(hash)
            }
            Err(_) => None,
        }
    }
}

impl Actor {
    /// 这一轮的技能、MCP 服务开关：这一刻配置里的全局开关，会话钉着预设的再照预设收窄（LM-29，`crate::presets::narrowed`）。
    pub(super) fn switches(&self) -> std::collections::BTreeMap<String, bool> {
        let switches = crate::skills::switches::of(&self.config.current().resolved.values());
        match self
            .kit
            .as_ref()
            .and_then(|kit| kit.snapshot.preset.as_ref())
        {
            Some(pin) => crate::presets::narrowed(switches, pin),
            None => switches,
        }
    }

    /// 载入的会话第一次开回合：这一轮再找技能时跳过的 `skipped`、读服务时不收的，记成被忽略的那两样（LM-3补，
    /// `crate::checkup`）。只记一次；造会话时已经记过的、这一轮没再找技能的（指纹没变、读不了文件）什么都不做。
    pub(super) fn note_ignored(&self, skipped: Option<Vec<String>>) {
        let (Some(kit), Some(skills)) = (self.mcp.as_ref(), skipped) else {
            return;
        };
        let mcp = kit
            .extras
            .servers
            .get()
            .map_or_else(Vec::new, |servers| servers.refused().to_vec());
        kit.extras.note(crate::checkup::Ignored {
            settings: Vec::new(),
            mcp,
            skills,
        });
    }
}

/// 目录 `catalog` 里 MCP 服务的工具在快照里的写法，照名字排（同快照的工具面）。
fn entries(catalog: &Catalog) -> Vec<ToolEntry> {
    catalog
        .specs()
        .filter(|spec| spec.name.starts_with(MCP_PREFIX))
        .map(crate::session::open::entry)
        .collect()
}
