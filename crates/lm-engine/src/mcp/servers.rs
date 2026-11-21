//! 一个主会话（连同它派的子会话）的 MCP 服务（L5 六，设计第五节第 11 条）：第一次造会话、载入会话时照配置里的 `[mcp.<名字>]`
//! 读（`lm_kernel::config::mcp`），开关关掉的（`features.mcp-<名字>`）不要；每个照缓存先有一份工具，没有缓存的在后台起一次列
//! 工具（[`Servers::warm`]），不挡造会话、不挡第一句话，列完了下一个回合换上。会话照 [`Servers::catalog`] 拼自己的工具目录，
//! 照 [`Servers::appended`] 拼 system 尾部的说明。
//!
//! 这一份跟着会话走：主会话和子会话的 actor 拿着；都停了它就没了，起着的服务跟着停（`server.rs`）。Miyu 是核心一份，管配置里的、
//! 软件包带的、扩展带的全部服务，还有给头看的列表、开关、状态、改配置的协议方法、密钥变了重起（`home.rs`、`methods.rs`、
//! `editing.rs`、`written.rs`、`secrets.rs`），这里都没搬。

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use lm_kernel::policy::ToolEntry;
use tokio::sync::watch;

use crate::mcp::server::{Hub, Places, Server};
use crate::mcp::tool::Texts;
use crate::mcp::{cache, instructions, spec};
use crate::session::Configs;
use crate::store::paths::Paths;
use crate::tool::{Catalog, mcp_service};

/// 目录里 MCP 服务的工具记在哪个包下（`Catalog::replacing_service`）。
const PACKAGE: &str = "mcp";

/// 一个主会话的 MCP 服务。
pub struct Servers {
    servers: Vec<Arc<Server>>,
    hub: Arc<Hub>,
    texts: Arc<Texts>,
    /// 配置里写错了、整个不收的服务的名字，照写的先后（LM-3补，`crate::checkup`）。
    refused: Vec<String>,
}

impl Servers {
    /// 照 `paths` 那一处的配置文件读：开关 `switches` 关掉的不要，缓存对得上的照缓存先有一份工具，密钥照 `configs` 取。读
    /// 磁盘，在阻塞线程里调。配置读不了的照没写办；写错了的那几张表不收，名字记下（[`Servers::refused`]）。
    pub(crate) fn read(
        paths: &Paths,
        switches: &BTreeMap<String, bool>,
        configs: Configs,
    ) -> Servers {
        let text = match crate::store::config_file::read(&paths.config_file()) {
            Ok(text) => text.map(|read| read.text).unwrap_or_default(),
            Err(_) => String::new(),
        };
        let (configured, problems) = lm_kernel::config::mcp::configured(&text);
        let refused = problems
            .into_iter()
            .filter_map(|problem| problem.server)
            .collect();
        let hub = Arc::new(Hub::new());
        let servers = configured
            .into_iter()
            .filter(|server| crate::skills::switches::on(switches, &server.feature_id()))
            .map(|server| {
                let name = server.name.clone();
                let cache = cache::path(&paths.cache, &name);
                let remembered = cache::read_at(&cache, &server);
                let places = Places {
                    cache,
                    dir: paths.state.join("mcp").join(&name),
                    log: paths.logs().join(format!("mcp-{name}.stderr")),
                };
                Arc::new(Server::new(
                    server,
                    places,
                    configs.clone(),
                    Arc::clone(&hub),
                    remembered,
                ))
            })
            .collect();
        Servers {
            servers,
            hub,
            texts: Arc::new(Texts::shipped()),
            refused,
        }
    }

    /// 配置里写错了、整个不收的服务的名字（`mcp` 整个没写成表的是 `mcp`），照写的先后。
    pub fn refused(&self) -> &[String] {
        &self.refused
    }

    /// 没有缓存的服务在后台起一次、列工具（不挡调的一方）；列完了闲够了照常停。起不来的记进运行日志，模型调到时再起一次。
    pub(crate) fn warm(&self) {
        for server in &self.servers {
            if server.listing().known {
                continue;
            }
            let server = Arc::clone(server);
            tokio::spawn(async move { drop(server.client().await) });
        }
    }

    /// 目录 `shipped` 加上各服务这时的工具：每个服务一组；目录查不过的那一件不登记。这一轮的开关 `switches` 关着的服务不加
    /// （LM-36，同 Miyu FX-8「开了的照现在的开关」）：读服务时开着、会话中途在 MCP 页关掉的，从下一个回合起工具面上没有它的
    /// 工具，system 尾部也没有它的说明（`appended` 只拼工具面上有工具的服务）；再打开的接着用。读服务时就关着的本来就不在
    /// 这一份里，会话中途打开也没有（同 Miyu：开会话时没开的照旧不开），新开的会话才有。
    pub(crate) fn catalog(&self, shipped: &Catalog, switches: &BTreeMap<String, bool>) -> Catalog {
        let mut catalog = shipped.clone();
        for server in &self.servers {
            if !crate::skills::switches::on(switches, &server.feature_id()) {
                continue;
            }
            let tools = spec::tools(server, &server.listing().tools, &self.texts);
            catalog = catalog.replacing_service(PACKAGE, server.name(), tools).0;
        }
        catalog
    }

    /// 工具面 `face` 上有工具的服务这时交的说明：键到拼好的那一段（`lm_kernel::policy::Snapshot::with_appended`）。
    pub(crate) fn appended(&self, face: &[ToolEntry]) -> BTreeMap<String, String> {
        let on_face: BTreeSet<&str> = face
            .iter()
            .filter_map(|tool| mcp_service(&tool.name))
            .collect();
        self.servers
            .iter()
            .filter(|server| on_face.contains(server.name()))
            .filter_map(|server| {
                let text = server.listing().instructions?;
                Some((
                    instructions::key(server.name()),
                    instructions::block(server.name(), &text)?,
                ))
            })
            .collect()
    }

    /// 这时连着的服务名（有进程、有连接），变了就有新的一份：头照它画、测试照它等起了停了。
    pub fn live(&self) -> watch::Receiver<BTreeSet<String>> {
        self.hub.live()
    }
}
