//! 供应商的增删改、试通不通、从 Miyu 导入：不带界面的那一层。命令行
//! 和配置界面（`crates/litemiyu/src/settings/`）的「从 Miyu 导入」「试一家」都调这里，只有这一套读写的逻辑。
//!
//! - [`configured`]：配置里配了的几家；[`forget_secret`]：删一个没人再引用的密钥；
//! - [`miyu`]：读 Miyu 的配置和密钥（只读）；[`import`]：照读到的算会导哪几家（[`import::plan`]），再写进 LiteMiyu 自己的文件
//!   （[`import::apply`]）；
//! - [`probe`]：试一家：先列模型，列不出、又不是认证错的发一句最小的话。
//!
//! 读写配置都走 [`crate::settings::Settings`]（只动那几行、记一条改动），key 的明文只经 `store_secret` 进密钥文件。

pub mod import;
pub mod lists;
pub mod miyu;
pub mod probe;

use std::collections::{BTreeMap, BTreeSet};

use lm_kernel::config::Value;
use lm_kernel::config::secret::{self, Reference};

use crate::settings::{SetError, Settings};
use crate::store::config_file::WriteError;
use crate::store::secrets;

/// 替换前发现手改了，最多重来几次（同配置服务）。
const TRIES: usize = 3;

/// 配置里写着 `providers.<id>.…` 的几家的编号，照编号排（同配置界面的供应商页）。
pub fn configured(written: &BTreeMap<String, Value>) -> Vec<String> {
    let ids: BTreeSet<String> = written
        .keys()
        .filter_map(|key| {
            let segments = lm_kernel::config::key::split(key)?;
            (segments.len() >= 3 && segments[0] == "providers").then(|| segments[1].clone())
        })
        .collect();
    ids.into_iter().collect()
}

/// 配置里 `prefix` 开头写着的键（一家供应商、一个池），删一家时照它一项项删。
pub fn written_under<'a>(
    written: &'a BTreeMap<String, Value>,
    prefix: &'a str,
) -> impl Iterator<Item = &'a String> + 'a {
    written.keys().filter(move |key| key.starts_with(prefix))
}

/// 配置里还有没有哪一项引用名字是 `name` 的密钥。
pub fn secret_used(written: &BTreeMap<String, Value>, name: &str) -> bool {
    fn refers(value: &Value, name: &str) -> bool {
        match value {
            Value::Secret(Reference::Secret(used)) => used == name,
            Value::List(items) => items.iter().any(|item| refers(item, name)),
            _ => false,
        }
    }
    written.values().any(|value| refers(value, name))
}

/// 从密钥文件里删掉名字是 `name` 的那一个（换了 key、删了 key 以后原来那个没人引用了）。没有这个名字的交回 `false`。
/// 只动那一行，文件照旧 0600；明文不进日志。
///
/// # Errors
///
/// 密钥文件读不了、读不懂、写不进；一直有人在手改。
pub fn forget_secret(settings: &Settings, name: &str) -> Result<bool, SetError> {
    let path = settings.paths().secrets_file();
    for _ in 0..TRIES {
        let Some(stored) = secrets::read(&path)? else {
            return Ok(false);
        };
        let text = stored.text.text.as_str();
        let file =
            secret::parse_file(text).map_err(|_| SetError::Broken(secrets::FILE.to_string()))?;
        if !file.entries.contains_key(name) {
            return Ok(false);
        }
        let edited = secret::unset_in(text, name)
            .map_err(|_| SetError::Broken(secrets::FILE.to_string()))?;
        match secrets::write(&path, edited.as_bytes(), Some(&stored.text.version)) {
            Ok(()) => {
                return Ok(true);
            }
            Err(WriteError::Changed) => continue,
            Err(WriteError::Io(error)) => return Err(SetError::Io(error.to_string())),
        }
    }
    Err(SetError::Conflict)
}

#[cfg(test)]
mod tests;
