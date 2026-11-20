//! 应急模式读配置（设计第五节第 16 条，`litemiyu --rescue`）：不依赖配置文件。配置、密钥读得到就用，读不到、读坏了照全空办，
//! 交回为什么（主程序在界面上说一句）；环境变量、命令行给的供应商（[`Given`]）盖在上面：配置成一家编号 [`PROVIDER`] 的供应商、
//! `models.chat` 指到它的模型。只读，什么都不写。
//!
//! 给的这一家照配置的规矩查一遍（同写在 `config.toml` 里）：地址写法不对、接口种类不认识的交回是哪一样。key 不进那一份字，
//! 直接放进密钥那一份（`{ secret = "rescue" }` 引用它），报错、`Debug` 里都没有它。

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use lm_kernel::config::Layer;
use lm_kernel::config::merge::{Layers, merge};
use lm_kernel::config::parse::{Parsed, parse};
use lm_kernel::config::secret::{Secret, parse_file};
use tokio::sync::watch;

use super::{Files, Loaded, items};
use crate::session::ConfigSource;
use crate::store::config_file;
use crate::store::paths::Paths;
use crate::store::secrets;

/// 给的供应商在配置里的编号：模型写成 `rescue/<模型>`。
pub const PROVIDER: &str = "rescue";

/// 环境变量、命令行给的一家供应商。
pub struct Given {
    /// 地址，路径由驱动接在后面（同配置的 `base_url`）。
    pub base_url: String,
    /// 模型名。
    pub model: String,
    /// 接口种类（同配置的 `driver`）：`openai-chat`、`anthropic`、`openai-responses`。
    pub api: String,
    /// key：没给的不带认证头。
    pub key: Option<Secret>,
}

/// 里面有 key：`Debug` 只说有没有。
impl fmt::Debug for Given {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Given")
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field("api", &self.api)
            .field("key", &self.key.is_some())
            .finish()
    }
}

/// 给的那一家哪一样写得不对：配置项的键（`base_url`、`driver`、`chat`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refused(pub String);

/// 读好的：配置，和配置、密钥读不了的为什么（读得了、没有文件的没有）。
#[derive(Debug)]
pub struct Rescued {
    /// 同 [`super::load`] 交回的。
    pub loaded: Loaded,
    /// 配置、密钥读不了、写坏了：哪一份、为什么（[`super::LoadError`] 的说法）。
    pub unreadable: Option<String>,
}

/// 读 `paths` 那一处（找不到家目录的没有）的配置和密钥，把 `given` 盖在上面；环境变量照 `env` 取。
///
/// # Errors
///
/// 给的那一家哪一样照配置的规矩写得不对。
pub fn load(
    paths: Option<&Paths>,
    given: Option<&Given>,
    env: impl Fn(&str) -> Option<String> + Send + Sync + 'static,
) -> Result<Rescued, Refused> {
    let items = items();
    let mut unreadable = None;
    let text = match paths.map(|paths| config_file::read(&paths.config_file())) {
        Some(Ok(Some(read))) => read.text,
        Some(Err(error)) => {
            unreadable = Some(super::LoadError::Unreadable(super::CONFIG, error).to_string());
            String::new()
        }
        Some(Ok(None)) | None => String::new(),
    };
    let mut parsed = parse(&items, Layer::System, &text).unwrap_or_else(|problem| {
        unreadable = Some(super::LoadError::Syntax(super::CONFIG, super::at(&problem)).to_string());
        Parsed::default()
    });
    let mut entries = BTreeMap::new();
    match paths.map(|paths| secrets::read(&paths.secrets_file())) {
        Some(Ok(Some(stored))) => match parse_file(&stored.text.text) {
            Ok(file) => entries = file.entries,
            Err(problem) => {
                let error = super::LoadError::Syntax(secrets::FILE, super::at(&problem));
                unreadable.get_or_insert(error.to_string());
            }
        },
        Some(Err(error)) => {
            let error = super::LoadError::Unreadable(secrets::FILE, error);
            unreadable.get_or_insert(error.to_string());
        }
        Some(Ok(None)) | None => {}
    }
    if let Some(given) = given {
        let over = overlay(&items, given)?;
        let ours = format!("providers.{PROVIDER}.");
        parsed.entries.retain(|key, _| !key.starts_with(&ours));
        parsed.entries.extend(over.entries);
        match &given.key {
            Some(key) => entries.insert(PROVIDER.to_string(), key.clone()),
            None => entries.remove(PROVIDER),
        };
    }
    let layers = Layers {
        system: Some(&parsed),
        ..Layers::default()
    };
    let resolved = merge(&items, &layers, &|name| env(name));
    let source: Arc<dyn ConfigSource> = Arc::new(Files {
        resolved: resolved.clone(),
        secrets: entries,
        env: Box::new(env),
    });
    let (updates, configs) = watch::channel(source);
    Ok(Rescued {
        loaded: Loaded {
            configs,
            updates,
            resolved,
            // 应急模式不提示被忽略的几样（设计第五节第 16 条：只说这一次要说的话）。
            ignored: Vec::new(),
        },
        unreadable,
    })
}

/// 给的那一家写成一份配置的字，照规矩读：哪一项不对交回它的键。
fn overlay(items: &[lm_kernel::config::Item], given: &Given) -> Result<Parsed, Refused> {
    let quoted = |text: &str| toml_edit::Value::from(text).to_string();
    let key = match given.key {
        Some(_) => format!("key = {{ secret = \"{PROVIDER}\" }}\n"),
        None => String::new(),
    };
    let text = format!(
        "[providers.{PROVIDER}]\ndriver = {}\nbase_url = {}\n{key}[models]\nchat = {}\n",
        quoted(&given.api),
        quoted(&given.base_url),
        quoted(&format!("{PROVIDER}/{}", given.model)),
    );
    let parsed = parse(items, Layer::System, &text).map_err(|_| Refused("toml".to_string()))?;
    match parsed.problems.first() {
        Some(problem) => {
            let key = problem.key.as_deref().unwrap_or_default();
            Err(Refused(key.rsplit('.').next().unwrap_or(key).to_string()))
        }
        None => Ok(parsed),
    }
}

#[cfg(test)]
mod tests;
