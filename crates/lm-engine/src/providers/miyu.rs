//! 读 Miyu 的配置和密钥（`docs/00-设计.md` 第五节第 13 条「从 Miyu 导入供应商」）：只读，不写、不锁、不建 Miyu 的任何东西。
//! 配置界面的「从 Miyu 导入」照它读。
//! Miyu 的核心没在跑、配置半坏时也尽量读：认得出几家交几家，认不出的照实说为什么。
//!
//! 数据根照 Miyu `miyu-store/src/root.rs` 现在的找法：`MIYU_HOME`（绝对路径，开头的 `~` 照家目录接），不然是家目录的 `.miyu`；
//! 配置 `etc/config.toml`、密钥 `etc/private/secrets.toml`（`root/places.rs`）。两边的配置格式同出一源，照 LiteMiyu 自己的清单
//! 一项项认（[`lm_kernel::config::parse::parse`]，读成树按名字取），Miyu 有、LiteMiyu 没有的项（价格这些）不抄。
//!
//! 密钥的明文只在 [`Key::Found`] 里（[`Secret`] 打印出来是 `Secret(…)`），不进任何一句给人看的话、日志。

use std::collections::BTreeMap;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};

use lm_kernel::config::parse::{Parsed, parse};
use lm_kernel::config::problem::Code;
use lm_kernel::config::secret::{Reference, Secret, parse_file};
use lm_kernel::config::{Item, Layer, Value};

use crate::store::config_file::{self, ReadError};
use crate::store::secrets;

/// 标记文件：Miyu 布局 3 的数据根。
const MARKER: &str = ".miyu-layout";

/// 旧版（布局 1、2）的标记文件：路径不一样，不认。
const OLD_MARKER: &str = ".miyu-root";

/// 一份配置最多从写坏的地方往前退几次（每次退到坏的那一行所在的那一张表前面）。
const SALVAGE: usize = 8;

/// 找 Miyu 的数据根：`var` 取环境变量。
///
/// # Errors
///
/// `MIYU_HOME` 写的是相对路径；要用家目录时找不到：给人看的原因的编号（`relative`、`no_home`）。
pub fn locate(var: &dyn Fn(&str) -> Option<String>) -> Result<PathBuf, &'static str> {
    let home = || {
        var("HOME")
            .filter(|home| !home.is_empty())
            .map(PathBuf::from)
    };
    match var("MIYU_HOME").filter(|set| !set.is_empty()) {
        Some(set) => {
            let path = match set.strip_prefix('~') {
                Some(rest) => home().ok_or("no_home")?.join(rest.trim_start_matches('/')),
                None => PathBuf::from(set),
            };
            if path.is_absolute() {
                Ok(path)
            } else {
                Err("relative")
            }
        }
        None => Ok(home().ok_or("no_home")?.join(".miyu")),
    }
}

/// Miyu 的配置里的一家供应商。
#[derive(Debug, Clone, PartialEq)]
pub struct Provider {
    /// 编号。
    pub id: String,
    /// 显示名（写了的）。
    pub name: Option<String>,
    /// 地址写给人看的样子（写死的照原样，环境变量的引用写成 `{ env = … }`）；没写的没有。
    pub url: Option<String>,
    /// 要抄的项：`providers.<id>.` 后面那一截到值（不含 key）。
    pub entries: Vec<(String, Value)>,
    /// key。
    pub key: Key,
    /// 写得不对、没抄的几项。
    pub dropped: usize,
}

/// 一家的 key。
#[derive(Debug, Clone, PartialEq)]
pub enum Key {
    /// 没写。
    None,
    /// 照 Miyu 的密钥文件取到了。
    Found(Secret),
    /// 引用环境变量：照原样抄引用。
    Env(String),
    /// 引用的密钥文件里没有这个名字。
    Missing(String),
    /// 密钥文件读不了、读不懂：原因的编号（[`Unread`]）。
    Unread(Unread),
}

/// 一份文件读不了的原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unread {
    /// 没有权限。
    Denied,
    /// 超过 1 MiB。
    TooBig,
    /// 不是 UTF-8。
    NotUtf8,
    /// 写法不对。
    Broken,
    /// 别的读不了（是个目录……）。
    Other,
}

/// 一家没法导的原因。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Skip {
    /// 编号不合 LiteMiyu 的写法：带着原样的那一段。
    BadName(String),
    /// 没写地址，也认不出是目录里哪一家。
    NoUrl,
}

/// 读到的。
#[derive(Debug, Clone, PartialEq)]
pub struct Found {
    /// 认得出的几家，照编号排。
    pub providers: Vec<Provider>,
    /// 认不出的几家：编号、为什么。
    pub skipped: Vec<(String, Skip)>,
    /// 配置写坏了：坏在第几行第几列（这时 `providers` 是坏处前面读出来的）。
    pub broken: Option<(usize, usize)>,
    /// 主对话的模型（`models.chat`）。Miyu 的池不读：导入不带池。
    pub chat: Option<String>,
}

/// 一家都读不出来。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failed {
    /// 是旧版 Miyu 的数据根（布局 1、2）。
    OldLayout,
    /// 没有配置文件：带着路径。
    NoConfig(PathBuf),
    /// 配置读不了。
    Unreadable(Unread),
    /// 配置是空的（没有一家供应商，也没有认不出的）。
    Empty,
    /// 写坏了，坏处前面一家都没有：第几行第几列。
    Broken(usize, usize),
}

/// 读 `root` 那一份 Miyu 的配置和密钥，照 `items`（LiteMiyu 的清单）认；`known` 说一个编号（和手写的 `catalog`、地址）是不是
/// 目录里认得出的一家（没写地址的照它判能不能用）。
///
/// # Errors
///
/// 见 [`Failed`]。
pub fn read(
    root: &Path,
    items: &[Item],
    known: &mut dyn FnMut(&str, Option<&str>) -> bool,
) -> Result<Found, Failed> {
    if !root.join(MARKER).exists() && root.join(OLD_MARKER).exists() {
        return Err(Failed::OldLayout);
    }
    let path = root.join("etc").join("config.toml");
    let text = match config_file::read(&path) {
        Ok(Some(read)) => read.text,
        Ok(None) => return Err(Failed::NoConfig(path)),
        Err(error) => return Err(Failed::Unreadable(unread(&error))),
    };
    let (parsed, broken) = salvage(items, &text);
    let secrets = secrets_of(root);
    let mut found = Found {
        providers: Vec::new(),
        skipped: Vec::new(),
        broken,
        chat: match counted(&parsed, "models.chat") {
            Some(Value::Text(chat)) => Some(chat.to_string()),
            _ => None,
        },
    };
    for (id, entries) in grouped(&parsed, "providers") {
        found
            .providers
            .push(provider(id, entries, &parsed, &secrets));
    }
    // 编号那一段写法不对的：清单认不出，问题里带着原样的键。
    for problem in &parsed.problems {
        if let Some(key) = &problem.key
            && let Some(rest) = key.strip_prefix("providers.")
            && problem.code == Code::BadSegment
        {
            let id = rest.split('.').next().unwrap_or(rest).trim_matches('"');
            if !found.skipped.iter().any(|(seen, _)| seen == id) {
                found
                    .skipped
                    .push((id.to_string(), Skip::BadName(id.to_string())));
            }
        }
    }
    let (usable, unusable): (Vec<Provider>, Vec<Provider>) =
        found.providers.into_iter().partition(|one| {
            let catalog = one.entries.iter().find_map(|(leaf, value)| match value {
                Value::Text(text) if leaf == "catalog" => Some(text.to_string()),
                _ => None,
            });
            one.url.is_some() || known(&one.id, catalog.as_deref())
        });
    found.providers = usable;
    found
        .skipped
        .extend(unusable.into_iter().map(|one| (one.id, Skip::NoUrl)));
    if found.providers.is_empty() && found.skipped.is_empty() {
        return Err(match broken {
            Some((line, column)) => Failed::Broken(line, column),
            None => Failed::Empty,
        });
    }
    Ok(found)
}

/// 照清单读；写坏了的退到坏的那一行所在的那一张表前面再读，读出坏处前面的几家（最多退 [`SALVAGE`] 次）。交回读到的、最早
/// 坏在哪。
fn salvage(items: &[Item], text: &str) -> (Parsed, Option<(usize, usize)>) {
    let mut text = text;
    let mut broken = None;
    for _ in 0..SALVAGE {
        match parse(items, Layer::System, text) {
            Ok(parsed) => return (parsed, broken),
            Err(problem) => {
                let (line, column) = problem.at.map_or((1, 1), |at| (at.line, at.column));
                broken.get_or_insert((line, column));
                // 坏的那一行往上最近的一个表头前面截断：那张表整张不要。
                let cut = text
                    .lines()
                    .take(line.saturating_sub(1))
                    .scan(0usize, |at, one| {
                        let start = *at;
                        *at += one.len() + 1;
                        Some((start, one))
                    })
                    .filter(|(_, one)| one.trim_start().starts_with('['))
                    .map(|(start, _)| start)
                    .last()
                    .unwrap_or(0);
                if cut == 0 {
                    break;
                }
                text = &text[..cut];
            }
        }
    }
    (Parsed::default(), broken)
}

/// 一项算数的值。
fn counted<'a>(parsed: &'a Parsed, key: &str) -> Option<&'a Value> {
    parsed
        .entries
        .get(key)
        .filter(|entry| entry.counts)
        .map(|entry| &entry.value)
}

/// `head.<名字>.…` 那几项，照名字分：名字到「名字后面那一截、值」。
fn grouped(parsed: &Parsed, head: &str) -> BTreeMap<String, Vec<(String, Value)>> {
    let mut groups: BTreeMap<String, Vec<(String, Value)>> = BTreeMap::new();
    for (key, entry) in parsed.entries.iter().filter(|(_, entry)| entry.counts) {
        let Some(rest) = key
            .strip_prefix(head)
            .and_then(|rest| rest.strip_prefix('.'))
        else {
            continue;
        };
        let Some((name, leaf)) = rest.split_once('.') else {
            continue;
        };
        groups
            .entry(name.to_string())
            .or_default()
            .push((leaf.to_string(), entry.value.clone()));
    }
    groups
}

/// 一家：key 照密钥文件取，写得不对的几项数一下。
fn provider(
    id: String,
    mut entries: Vec<(String, Value)>,
    parsed: &Parsed,
    secrets: &Result<BTreeMap<String, Secret>, Unread>,
) -> Provider {
    let prefix = format!("providers.{id}.");
    let dropped = parsed
        .problems
        .iter()
        .filter(|problem| {
            problem.code != Code::UnknownKey
                && problem
                    .key
                    .as_deref()
                    .is_some_and(|key| key.starts_with(&prefix))
        })
        .count();
    let key = match entries.iter().position(|(leaf, _)| leaf == "key") {
        None => Key::None,
        Some(at) => match entries.remove(at).1 {
            Value::Secret(Reference::Env(name)) => Key::Env(name),
            Value::Secret(Reference::Secret(name)) => match secrets {
                Ok(stored) => stored
                    .get(&name)
                    .map_or(Key::Missing(name), |secret| Key::Found(secret.clone())),
                Err(why) => Key::Unread(*why),
            },
            _ => Key::None,
        },
    };
    let text = |leaf: &str| {
        entries.iter().find_map(|(at, value)| match value {
            Value::Text(text) if at == leaf => Some(text.to_string()),
            Value::Secret(reference) if at == leaf => Some(reference.toml()),
            _ => None,
        })
    };
    Provider {
        name: text("name"),
        url: text("base_url"),
        id,
        entries,
        key,
        dropped,
    }
}

/// Miyu 的密钥：名字到密钥。没有这份文件的是空的。
fn secrets_of(root: &Path) -> Result<BTreeMap<String, Secret>, Unread> {
    let path = root.join("etc").join("private").join(secrets::FILE);
    match secrets::read(&path) {
        Ok(None) => Ok(BTreeMap::new()),
        Ok(Some(stored)) => parse_file(&stored.text.text)
            .map(|file| file.entries)
            .map_err(|_| Unread::Broken),
        Err(error) => Err(unread(&error)),
    }
}

/// 读不了的原因。
fn unread(error: &ReadError) -> Unread {
    match error {
        ReadError::Unreadable(error) if error.kind() == ErrorKind::PermissionDenied => {
            Unread::Denied
        }
        ReadError::Unreadable(_) => Unread::Other,
        ReadError::TooBig => Unread::TooBig,
        ReadError::NotUtf8 => Unread::NotUtf8,
    }
}

#[cfg(test)]
mod tests;
