//! 配置服务（施工 L8，`docs/00-设计.md` 第五节第 13 条）：配置界面直接调的那一层，Miyu 核心协议上 `config.schema`、
//! `config.get`、`config.set`、`secret.set`、`provider.catalog` 那几个方法的里子（`miyu-endpoint/src/config/`、`providers.rs`）。
//! LiteMiyu 没有协议，界面在自己的线程里调这里。
//!
//! - [`Settings::read`]：读一次 `config.toml`、`secrets.toml`，交回每一项的最终值和来源、配置里写着的、密钥的名字。
//! - [`Settings::set`]：改几项、删几项：先重读一遍（手改过的在新的字上改），照 `lm_kernel::config::edit` 只动那几行，注释、
//!   先后、别的行一个字节不动；替换前发现有人这一瞬间手改了，重来，最多三次。写成了往 `journal.jsonl` 记一条。
//! - [`Settings::store_secret`]：存一个密钥，起一个不撞已有的名字，文件一律 0600；配置里写的是引用。
//! - [`schema`]：界面画哪几页、哪几组、哪几项、给人看的字（只读选中的那种语言）。
//! - [`catalog`]：目录里的几家、一家的模型：只在配置界面要时读整份目录，用完放掉。
//!
//! 用到才读：这一层只在进配置界面时用，平常的对话不碰它（`docs/00-设计.md` 第七节「用到才初始化」）。Miyu 这里另有的没搬：
//! 项目配置和信任、个人设置那一层、整份换掉（`miyu config edit`）、`expect`、看着文件的改动当场推、软件包的配置项。

pub mod catalog;
pub mod declared;
pub mod schema;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use lm_kernel::config::edit::{self, Change};
use lm_kernel::config::merge::{Layers, Resolved, merge};
use lm_kernel::config::parse::{Parsed, parse};
use lm_kernel::config::secret::{self, Secret};
use lm_kernel::config::{Item, Layer, Value};
use lm_kernel::kernel::id::EventKind;
use lm_kernel::kernel::origin::{By, Person};
use lm_kernel::kernel::raw::RawJson;
use lm_kernel::kernel::time::Timestamp;
use serde_json::json;

use crate::store::config_file::{self, ConfigText, ReadError, WriteError};
use crate::store::journal;
use crate::store::paths::Paths;
use crate::store::secrets;

/// 替换前发现手改了，最多重来几次（Miyu `config/set.rs` 的 `TRIES`）。
const TRIES: usize = 3;

/// 真的改了的几项：键、原来的值、改成的值（删掉的没有）。
type Changed = Vec<(String, Option<serde_json::Value>, Option<serde_json::Value>)>;

/// 取环境变量的办法：`{ env }` 的 key、带 `env` 的配置项照它。
type Lookup = Box<dyn Fn(&str) -> Option<String> + Send + Sync>;

/// 配置服务：数据放哪、怎么取环境变量、登记的全部配置项。
pub struct Settings {
    paths: Paths,
    env: Lookup,
    items: Vec<Item>,
}

impl fmt::Debug for Settings {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Settings")
            .field("paths", &self.paths)
            .finish_non_exhaustive()
    }
}

/// 读到的一份。
#[derive(Debug, Clone)]
pub struct Snapshot {
    /// 有没有 `config.toml`。
    pub exists: bool,
    /// 配置里写着、算数的：真的键到值。
    pub written: BTreeMap<String, Value>,
    /// 每一项的最终值和来源（默认、配置文件第几行、环境变量）。
    pub resolved: Resolved,
    /// 密钥文件里写对了的名字（不带值）。
    pub secrets: BTreeSet<String>,
    /// 整份读不懂（TOML 写法不对）：第几行第几列；这时只能看、不能存。
    pub broken: Option<(usize, usize)>,
    /// `config.toml` 的字（没有的是空的）：[`Settings::preview`] 照它在内存里先算改完的样子。只有引用，没有 key 的明文。
    pub text: String,
}

/// 改一项：改成什么，或者删掉（恢复默认）。
#[derive(Debug, Clone, PartialEq)]
pub enum Edit {
    /// 键改成这个值；没写的加上。
    Set(String, Value),
    /// 删掉这个键。
    Unset(String),
}

impl Edit {
    /// 改的是哪个键。
    pub fn key(&self) -> &str {
        match self {
            Edit::Set(key, _) | Edit::Unset(key) => key,
        }
    }
}

/// 改不成。什么都没写。
#[derive(Debug)]
pub enum SetError {
    /// 清单里没有这个键。
    UnknownKey(String),
    /// 值不合这一项的类型（不在选项里、超出范围、写法不对）：键和原因的编号。
    Invalid(String, &'static str),
    /// 文件读不懂，或者这一项所在的那一组写成了别的东西：带着键。
    Broken(String),
    /// 改的时候一直有人在手改。
    Conflict,
    /// 读不了、写不进：系统的原话。
    Io(String),
}

impl fmt::Display for SetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SetError::UnknownKey(key) => write!(f, "unknown config key {key}"),
            SetError::Invalid(key, code) => write!(f, "{key}: {code}"),
            SetError::Broken(key) => write!(f, "config.toml cannot be edited at {key}"),
            SetError::Conflict => write!(f, "config.toml kept changing while saving"),
            SetError::Io(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for SetError {}

impl From<ReadError> for SetError {
    fn from(error: ReadError) -> SetError {
        SetError::Io(error.to_string())
    }
}

impl Settings {
    /// 数据在 `paths` 那一处，环境变量照 `env` 取。不碰磁盘。
    pub fn new(paths: Paths, env: impl Fn(&str) -> Option<String> + Send + Sync + 'static) -> Self {
        Self {
            paths,
            env: Box::new(env),
            items: crate::configured::items(),
        }
    }

    /// 登记的全部配置项。
    pub fn items(&self) -> &[Item] {
        &self.items
    }

    /// 数据放哪。
    pub fn paths(&self) -> &Paths {
        &self.paths
    }

    /// 读一次配置和密钥的名字。没有的文件照空的办；写错的那几项照没写（读配置的一方另记 `WARN`）。
    ///
    /// # Errors
    ///
    /// 哪一份读不了、太大、不是 UTF-8，密钥文件的 TOML 写法不对。
    pub fn read(&self) -> Result<Snapshot, SetError> {
        let file = config_file::read(&self.paths.config_file())?;
        let exists = file.is_some();
        let text = file.map(|read| read.text).unwrap_or_default();
        let secrets = self.secret_names()?;
        Ok(self.snapshot_of(text, exists, secrets))
    }

    /// 照配置的字 `text` 算一份（不碰磁盘）。
    fn snapshot_of(&self, text: String, exists: bool, secrets: BTreeSet<String>) -> Snapshot {
        let (parsed, broken) = match parse(&self.items, Layer::System, &text) {
            Ok(parsed) => (parsed, None),
            Err(problem) => (
                Parsed::default(),
                Some(problem.at.map_or((1, 1), |at| (at.line, at.column))),
            ),
        };
        let written = parsed
            .entries
            .iter()
            .filter(|(_, entry)| entry.counts)
            .map(|(key, entry)| (key.clone(), entry.value.clone()))
            .collect();
        let layers = Layers {
            system: Some(&parsed),
            ..Layers::default()
        };
        let resolved = merge(&self.items, &layers, &|name| (self.env)(name));
        Snapshot {
            exists,
            written,
            resolved,
            secrets,
            broken,
            text,
        }
    }

    /// 改几项以后会是什么样（同 [`Settings::set`] 的查法、改法），只在内存里算，不读不写磁盘：界面先照它画，真的写盘在别的线程里
    /// 做（`crates/litemiyu/src/settings/writer.rs`），写完照读回的换上。`names` 是这一下会新存进密钥文件的名字。
    ///
    /// # Errors
    ///
    /// 同 [`Settings::set`] 里不碰磁盘的那几种：键不认得、值不对、文件读不懂。
    pub fn preview(
        &self,
        now: &Snapshot,
        edits: &[Edit],
        names: &[String],
    ) -> Result<Snapshot, SetError> {
        self.check(edits)?;
        if now.broken.is_some() {
            return Err(SetError::Broken(config_file_name()));
        }
        let (edited, changed) = self.edit_text(&now.text, edits)?;
        let mut secrets = now.secrets.clone();
        secrets.extend(names.iter().cloned());
        Ok(self.snapshot_of(edited, now.exists || !changed.is_empty(), secrets))
    }

    /// 每一项认得、值合这一项的类型。
    fn check(&self, edits: &[Edit]) -> Result<(), SetError> {
        for one in edits {
            let item = lm_kernel::config::key::item_of(&self.items, one.key())
                .filter(|item| item.layers.contains(&Layer::System))
                .ok_or_else(|| SetError::UnknownKey(one.key().to_string()))?;
            if let Edit::Set(key, value) = one {
                item.kind
                    .check(value)
                    .map_err(|code| SetError::Invalid(key.clone(), code.as_str()))?;
            }
        }
        Ok(())
    }

    /// 在配置的字 `text` 上改：和文件里一样的不动。交回改好的字、真的改了的（键、原来、改成）。
    fn edit_text(&self, text: &str, edits: &[Edit]) -> Result<(String, Changed), SetError> {
        let parsed = parse(&self.items, Layer::System, text)
            .map_err(|_| SetError::Broken(config_file_name()))?;
        let mut edited = text.to_string();
        let mut changed = Vec::new();
        for one in edits {
            let now = parsed
                .entries
                .get(one.key())
                .filter(|entry| entry.counts)
                .map(|entry| &entry.value);
            let change = match one {
                Edit::Set(_, value) if now == Some(value) => continue,
                Edit::Set(key, value) => Change::Set(key, value),
                Edit::Unset(_) if now.is_none() && !parsed.entries.contains_key(one.key()) => {
                    continue;
                }
                Edit::Unset(key) => Change::Unset(key),
            };
            edited = edit::apply(&edited, change).map_err(|blocked| SetError::Broken(blocked.0))?;
            changed.push((one.key().to_string(), now.map(Value::json), new_json(one)));
        }
        Ok((edited, changed))
    }

    /// 密钥文件里写对了的名字。
    fn secret_names(&self) -> Result<BTreeSet<String>, SetError> {
        let Some(stored) = secrets::read(&self.paths.secrets_file())? else {
            return Ok(BTreeSet::new());
        };
        let file = secret::parse_file(&stored.text.text)
            .map_err(|_| SetError::Broken(secrets::FILE.to_string()))?;
        Ok(file.entries.into_keys().collect())
    }

    /// 改几项（[`Edit`]）：一起查、一起写，有一项不对整条不收。和文件里一样的不动；全都一样的不写盘。交回真的改了的键。
    ///
    /// # Errors
    ///
    /// 见 [`SetError`]。
    pub fn set(&self, edits: &[Edit]) -> Result<Vec<String>, SetError> {
        self.check(edits)?;
        let path = self.paths.config_file();
        for _ in 0..TRIES {
            let read = config_file::read(&path)?;
            let (text, version, bom) = match &read {
                Some(ConfigText { text, version, bom }) => {
                    (text.as_str(), Some(version.as_str()), *bom)
                }
                None => ("", None, false),
            };
            let (edited, changed) = self.edit_text(text, edits)?;
            if changed.is_empty() {
                return Ok(Vec::new());
            }
            match config_file::write(&path, &config_file::bytes(&edited, bom), version) {
                Ok(()) => {
                    self.record(&changed);
                    return Ok(changed.into_iter().map(|(key, _, _)| key).collect());
                }
                Err(WriteError::Changed) => continue,
                Err(WriteError::Io(error)) => {
                    return Err(SetError::Io(error.to_string()));
                }
            }
        }
        Err(SetError::Conflict)
    }

    /// 存一个密钥：名字照 `wanted`（合写法的，Miyu `secret.set` 的规矩），已经有这个名字的接 `-2`、`-3`……错开。交回存成的
    /// 名字。明文不进日志、不进报错。
    ///
    /// # Errors
    ///
    /// 名字不合写法；密钥文件读不了、读不懂、写不进；一直有人在手改。
    pub fn store_secret(&self, wanted: &str, value: &Secret) -> Result<String, SetError> {
        if !secret::valid_name(wanted) {
            return Err(SetError::Invalid(wanted.to_string(), "bad_format"));
        }
        let path = self.paths.secrets_file();
        for _ in 0..TRIES {
            let stored = secrets::read(&path)?;
            let (text, version) = match &stored {
                Some(stored) => (
                    stored.text.text.as_str(),
                    Some(stored.text.version.as_str()),
                ),
                None => ("", None),
            };
            let file = secret::parse_file(text)
                .map_err(|_| SetError::Broken(secrets::FILE.to_string()))?;
            let name = free_name(wanted, |name| file.entries.contains_key(name))
                .ok_or_else(|| SetError::Invalid(wanted.to_string(), "bad_format"))?;
            let edited = secret::set_in(text, &name, value)
                .map_err(|_| SetError::Broken(secrets::FILE.to_string()))?;
            match secrets::write(&path, edited.as_bytes(), version) {
                Ok(()) => {
                    self.record(&[(format!("secrets.{name}"), None, None)]);
                    return Ok(name);
                }
                Err(WriteError::Changed) => continue,
                Err(WriteError::Io(error)) => {
                    return Err(SetError::Io(error.to_string()));
                }
            }
        }
        Err(SetError::Conflict)
    }

    /// 往 `journal.jsonl` 记一条 `config.changed`：哪几个键、原来和改成的值（密钥只记名字）。记不下的记一条 `WARN`，不拦改动。
    fn record(&self, changes: &[(String, Option<serde_json::Value>, Option<serde_json::Value>)]) {
        let Some(at) = Timestamp::from_unix_millis(jiff::Timestamp::now().as_millisecond()) else {
            return;
        };
        let changes: Vec<serde_json::Value> = changes
            .iter()
            .map(|(key, old, new)| {
                let mut one = json!({ "key": key });
                if let Some(old) = old {
                    one["old"] = old.clone();
                }
                if let Some(new) = new {
                    one["new"] = new.clone();
                }
                one
            })
            .collect();
        let body = json!({ "layer": "system", "file": config_file_name(), "via": "settings", "changes": changes });
        let (Ok(kind), Some(body)) = (EventKind::parse("config.changed"), RawJson::of(&body))
        else {
            return;
        };
        let entry = journal::Entry {
            at,
            by: By::Person(Person::default()),
            cause: None,
            kind,
            body,
        };
        drop(journal::append(&self.paths.journal(), entry));
    }
}

/// 配置文件的文件名（改不成时说是哪一份）。
fn config_file_name() -> String {
    "config.toml".to_string()
}

/// 改成的值写进日志的样子：删掉的没有。
fn new_json(edit: &Edit) -> Option<serde_json::Value> {
    match edit {
        Edit::Set(_, value) => Some(value.json()),
        Edit::Unset(_) => None,
    }
}

/// 不撞 `taken` 的名字：`wanted`，撞了接 `-2`、`-3`……接上以后不合写法（太长）的没有。
pub fn free_name(wanted: &str, taken: impl Fn(&str) -> bool) -> Option<String> {
    if !taken(wanted) {
        return Some(wanted.to_string());
    }
    (2..)
        .map(|n| format!("{wanted}-{n}"))
        .take_while(|name| secret::valid_name(name))
        .find(|name| !taken(name))
}

#[cfg(test)]
mod tests;
