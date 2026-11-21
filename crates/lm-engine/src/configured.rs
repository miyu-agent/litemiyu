//! 照配置文件取会话的配置（Miyu 核心的配置服务里会话要的那一半，`miyu-core/src/settings.rs` 的 `read`、
//! `miyu-endpoint/src/config/hub.rs` 的 `ConfigSource`）：读一次系统配置 `config.toml`、密钥文件 `secrets.toml`，照登记的
//! 配置项合出最终值；会话照它挑供应商，`{ secret }` 照密钥文件、`{ env }` 照进程的环境取 key。
//!
//! Miyu 这里另有的没搬：项目配置和信任的记录、改了配置当场换上（配置服务、`config.changed`）、软件包自己的配置项。
//! LiteMiyu 改配置的界面（L8）搬来以前，配置在第一次发话时读一次，改了的下次启动用上。
//!
//! 读不进来的照 Miyu 不拦着：写错的那几项、密钥文件里写错的那几行照没写办，名字交回（[`Loaded::ignored`]，主程序弹一条
//! 提示）。整份读不了（权限、太大、不是 UTF-8、TOML 写法不对）的交回错误，由主程序说给人听。

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use lm_kernel::config::merge::{Layers, Resolved, merge};
use lm_kernel::config::parse::parse;
use lm_kernel::config::problem::Problem;
use lm_kernel::config::secret::{Reference, Secret, parse_file};
use lm_kernel::config::{Item, Layer};
use lm_kernel::models::settings::{
    CatalogSettings, ModelSettings, PriceSettings, ProviderSettings, UseSettings,
};
use tokio::sync::watch;

use crate::session::settings::{CompactionSettings, ConfirmSettings, JobSettings};
use crate::session::{ConfigSource, Configs};
use crate::settings::declared::{SessionDefaults, TuiSettings, UiSettings};
use crate::store::config_file::{self, ReadError};
use crate::store::paths::Paths;
use crate::store::secrets;

/// 系统配置的文件名（`store::paths::Paths::config_file`）。
const CONFIG: &str = "config.toml";

/// 登记的全部配置项：模型这一块（连同手写的价格，核心线第二趟；取模型目录的 `[models.catalog]`，LM-9）、压缩、后台命令，配置界面加的界面语言、新会话的档（`crate::settings::declared`）。
pub fn items() -> Vec<Item> {
    [
        ProviderSettings::ITEMS,
        ModelSettings::ITEMS,
        PriceSettings::ITEMS,
        UseSettings::ITEMS,
        CatalogSettings::ITEMS,
        CompactionSettings::ITEMS,
        JobSettings::ITEMS,
        ConfirmSettings::ITEMS,
        UiSettings::ITEMS,
        TuiSettings::ITEMS,
        SessionDefaults::ITEMS,
        crate::skills::switches::ITEMS,
        crate::presets::ITEMS,
    ]
    .concat()
}

/// 读好的配置。
#[derive(Debug)]
pub struct Loaded {
    /// 会话从这里取（[`Configs`]）。
    pub configs: Configs,
    /// 往 `configs` 里换上重读的一份（配置界面存过以后，`litemiyu` 的 `core/engine/setup.rs`）。
    pub updates: watch::Sender<Arc<dyn ConfigSource>>,
    /// 合出来的最终值：读模型目录时照它把配置里写过的模型都查一遍（`models::begin`）。
    pub resolved: Resolved,
    /// 读的时候被忽略的配置项（LM-3补，[`crate::checkup`]）：`config.toml` 里不认识的、写错的项的键，照文件里的先后；
    /// 接着是 `secrets.toml` 里写错的那几行的名字。退役的键（`net.search.*` 这些）不算。
    pub ignored: Vec<String>,
}

/// 整份读不了。
#[derive(Debug)]
pub enum LoadError {
    /// `config.toml` 或 `secrets.toml` 读不了：哪一份、为什么。
    Unreadable(&'static str, ReadError),
    /// 写法不对：哪一份、第几行第几列（知道的话）。
    Syntax(&'static str, Option<(usize, usize)>),
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::Unreadable(file, error) => write!(f, "{file}: {error}"),
            LoadError::Syntax(file, Some((line, column))) => {
                write!(f, "{file}: not valid TOML at line {line}, column {column}")
            }
            LoadError::Syntax(file, None) => write!(f, "{file}: not valid TOML"),
        }
    }
}

impl std::error::Error for LoadError {}

/// 读 `paths` 那一处的配置和密钥，环境变量照 `env` 取（`{ env }` 的 key、带 `env` 的配置项）。没有这两份文件的照全空办。
///
/// # Errors
///
/// 哪一份整份读不了，见 [`LoadError`]。
pub fn load(
    paths: &Paths,
    env: impl Fn(&str) -> Option<String> + Send + Sync + 'static,
) -> Result<Loaded, LoadError> {
    let items = items();
    let text = config_file::read(&paths.config_file())
        .map_err(|error| LoadError::Unreadable(CONFIG, error))?
        .map(|read| read.text)
        .unwrap_or_default();
    let parsed = parse(&items, Layer::System, &text)
        .map_err(|problem| LoadError::Syntax(CONFIG, at(&problem)))?;
    let layers = Layers {
        system: Some(&parsed),
        ..Layers::default()
    };
    let resolved = merge(&items, &layers, &|name| env(name));
    let mut ignored: Vec<String> = parsed
        .problems
        .iter()
        .cloned()
        .map(|problem| crate::checkup::setting_name(problem, CONFIG))
        .collect();
    let stored = secrets::read(&paths.secrets_file())
        .map_err(|error| LoadError::Unreadable(secrets::FILE, error))?;
    let mut entries = BTreeMap::new();
    if let Some(stored) = stored {
        let file = parse_file(&stored.text.text)
            .map_err(|problem| LoadError::Syntax(secrets::FILE, at(&problem)))?;
        entries = file.entries;
        ignored.extend(
            file.problems
                .into_iter()
                .map(|problem| crate::checkup::setting_name(problem, secrets::FILE)),
        );
    }
    let source: Arc<dyn ConfigSource> = Arc::new(Files {
        resolved: resolved.clone(),
        secrets: entries,
        env: Box::new(env),
    });
    let (updates, configs) = watch::channel(source);
    Ok(Loaded {
        configs,
        updates,
        resolved,
        ignored,
    })
}

/// 问题出在第几行第几列。
fn at(problem: &Problem) -> Option<(usize, usize)> {
    problem.at.map(|at| (at.line, at.column))
}

/// 取环境变量的办法。
type Lookup = Box<dyn Fn(&str) -> Option<String> + Send + Sync>;

/// 读好的一份：最终值、密钥文件、取环境变量的办法。
struct Files {
    /// 最终值。
    resolved: Resolved,
    /// 密钥文件里写对了的：名字到密钥。
    secrets: BTreeMap<String, Secret>,
    /// 取环境变量。
    env: Lookup,
}

/// 里面有密钥：`Debug` 不印。
impl fmt::Debug for Files {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Files")
            .field("secrets", &self.secrets.len())
            .finish_non_exhaustive()
    }
}

impl ConfigSource for Files {
    /// 没有项目配置：在哪个目录都是同一份。
    fn with_project(&self, _dir: &str) -> Resolved {
        self.resolved.clone()
    }

    /// `{ secret }` 照密钥文件，`{ env }` 照环境：没设的、设成空的、有控制字符的是空的。
    fn secret(&self, reference: &Reference) -> Option<Secret> {
        match reference {
            Reference::Secret(name) => self.secrets.get(name).cloned(),
            Reference::Env(name) => (self.env)(name).and_then(|value| Secret::new(&value).ok()),
        }
    }

    /// 系统那一层是配置目录里的 `config.toml`（Miyu 写数据根里的相对路径）：手写的价格记进 `cost.source` 照它写
    /// （`config:config.toml:<行>`）。LiteMiyu 没有别的层，照层的名字写。
    fn file(&self, layer: Layer) -> String {
        match layer {
            Layer::System => CONFIG.to_string(),
            other => other.as_str().to_string(),
        }
    }
}

pub mod rescue;

#[cfg(test)]
mod tests;
