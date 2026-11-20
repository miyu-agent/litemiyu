//! 目录不常驻，读目录这一头（施工 V-25 下，`docs/blueprint/models.md`「怎么走」第二条第 2 条，从 Miyu
//! `miyu-core/src/models/recall.rs` 搬来）：记下的那份（模型目录拉到的那一处的 `catalog.json`，[`crate::session::ModelData`]
//! 管怎么记、怎么换）认哪一份目录、起来时读不读整份目录、临时怎么读。
//!
//! - 版本串 `of`（[`Version::of`]）：哪一份目录、`fetched`，加认原厂的表的内容哈希。对不上的整份作废、重算。和 Miyu 不同：
//!   Miyu 照原文和程序的大小、改动时间认（安装包换了快照，程序跟着换）；LiteMiyu 不带快照（LM-9），只认缓存的那一份，照那个
//!   文件的大小和改动时间。以前照编进程序的快照记下的那份（`snapshot …`）对不上，照缓存里的目录重算。
//! - 起来时（[`early`]）：先读记下的那份、算排第一那份目录的版本串，对上了就不读整份目录——省内存就省在这里；对不上、没有、
//!   读不懂的读整份目录，由 `super::begin` 把配置里写过的都查一遍记下、写盘，再放掉整份目录。
//! - 没有缓存目录的（测试）不走这条：照旧常驻。

use std::hash::{DefaultHasher, Hasher};
use std::path::Path;
use std::sync::Arc;
use std::time::UNIX_EPOCH;

use lm_kernel::models::catalog::{CatalogSource, Loaded, Recalled};

use super::catalog::{self, FILE, Places};
use crate::session::{CatalogReader, RECALL_FILE};

/// 算版本串要的：认原厂的表的哈希。后台更新拉到新目录时也照它算（[`super::refresh::Refresher`]）。
#[derive(Debug, Clone)]
pub struct Version {
    /// 认原厂的表的内容哈希。
    pub vendors: String,
}

impl Version {
    /// 照认原厂的表的原文 `vendors` 造。
    pub fn of_vendors(vendors: &str) -> Version {
        Version {
            vendors: hash(vendors),
        }
    }

    /// `source` 那一份目录（缓存的在 `cache` 里，`meta` 里写的 `fetched`）的版本串；缓存的原文读不到大小和改动时间的没有，快照
    /// （LM-9 起没有了）没有。
    pub fn of(&self, source: CatalogSource, cache: Option<&Path>, fetched: &str) -> Option<String> {
        let print = match source {
            // LM-9 起程序不带快照：以前照快照记下的那份对不上，照缓存里的目录重算。
            CatalogSource::Snapshot => return None,
            CatalogSource::Cache => {
                let meta = std::fs::metadata(cache?.join(FILE)).ok()?;
                let modified = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
                format!("cache {} {} {fetched}", meta.len(), modified.as_nanos())
            }
        };
        Some(format!("{print} vendors={}", self.vendors))
    }
}

/// 认原厂的表的内容哈希：同一个程序算出来的一定一样（换了程序、表变了的，记下的那份对不上，重算）。
fn hash(text: &str) -> String {
    let mut hasher = DefaultHasher::new();
    hasher.write(text.as_bytes());
    format!("{:016x}", hasher.finish())
}

/// 起来时读到的（[`early`]）。
pub enum Read {
    /// 整份目录：都读不了的没有；带上它的版本串的，由 `super::begin` 照它记下配置里写过的、放掉整份目录。
    Full(Option<Loaded>, Option<String>),
    /// 记下的那份，版本对上了：没读整份目录。
    Recalled(Recalled),
}

/// 起来时：`version`、`state`（模型目录拉到的那一处，`Paths::model_cache`）都有、又有缓存目录的，先读记下的那份，版本串和排
/// 第一那份目录的对上了就交回它、不读整份目录，记一行 `INFO catalog recalled`；别的读整份目录。记下的那份读不懂、写法的版本
/// 不对的记一行 `WARN catalog recall unreadable`，当没有。在阻塞线程里调。
pub fn early(places: &Places, version: Option<&Version>, state: Option<&Path>) -> Read {
    let Some((version, state)) = version.zip(state).filter(|_| places.cache.is_some()) else {
        return Read::Full(catalog::load(places), None);
    };
    let first = catalog::ranked(places)
        .into_iter()
        .next()
        .and_then(|(source, meta)| {
            let fetched = meta.map(|meta| meta.fetched).unwrap_or_default();
            version.of(source, places.cache.as_deref(), &fetched)
        });
    if let (Some(first), Some(recalled)) = (first, read(state))
        && recalled.of() == first
    {
        return Read::Recalled(recalled);
    }
    let loaded = catalog::load(places);
    let of = loaded
        .as_ref()
        .and_then(|loaded| of_loaded(places, version, loaded));
    Read::Full(loaded, of)
}

/// 读回来的整份目录的版本串：照它是哪一份、`fetched` 现算。
fn of_loaded(places: &Places, version: &Version, loaded: &Loaded) -> Option<String> {
    version.of(loaded.source, places.cache.as_deref(), &loaded.fetched)
}

/// 读拉到的那一处的 `catalog.json`：没有的没有；读不了、读不懂、写法的版本不对的记一行 `WARN`，当没有（整份重算）。
fn read(state: &Path) -> Option<Recalled> {
    let path = state.join(RECALL_FILE);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
        Err(_) => {
            return None;
        }
    };
    Recalled::from_json(&text).map_err(|_| ()).ok()
}

/// 临时读整份目录的办法（交给 `ModelData`）：照起来时同样的路挑一份读（读完那一行记 `DEBUG`），连同它的版本串；版本串算不
/// 出来的给空字（和哪一份记下的都对不上，照它重算）。
pub fn reader(places: Places, version: Version) -> CatalogReader {
    Arc::new(move || {
        let loaded = catalog::load_again(&places)?;
        let of = of_loaded(&places, &version, &loaded).unwrap_or_default();
        Some((loaded, of))
    })
}
