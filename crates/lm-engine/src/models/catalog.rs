//! 读目录（`docs/blueprint/models.md`「怎么走」第二条第 1、2 条，施工 8-7，从 Miyu `miyu-core/src/models/catalog.rs` 搬来）：
//! 缓存目录里取来的那一份（[`super::refresh`]）。LM-9 起程序不带快照：缓存里没有、读不了、读出来一家供应商都没有的，目录是
//! 空的，照样起来（以前的版本取过、留在缓存目录里的照样认）。
//!
//! 缓存的是原样的 `api.json`（`models-dev.json`），旁边一份 `models-dev.meta.json`：`{"source":<网址>,"fetched":<时刻>,"etag":…}`。
//! `meta` 没有、读不了的，目录照用，当作最旧（到点就重取）。原文还是整理过的样子量过再定（LM-9）：平常发话只读记下的小表
//! （`recall`），整份目录只在第一次、换了目录、遇到没记过的模型时读一次，读完放掉；存原文省一份整理的代码，`ETag` 也对得上。
//!
//! 和 Miyu 不同：Miyu 解过一次就在缓存目录里另存一份整理好的（`digest.rs`，施工 V-5 中），LiteMiyu 没搬：整份目录只在没记过的
//! 输入、换了目录时才读（`session` 的路由里记下来的那份），多写一份 5 MB 上下的文件换不回多少。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use lm_kernel::models::catalog::{Catalog, CatalogSource, Loaded};

/// 缓存的目录的文件名。
pub const FILE: &str = "models-dev.json";

/// 旁边那一份的文件名。
pub const META: &str = "models-dev.meta.json";

/// 一份目录旁边的 `meta`。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Meta {
    /// 从哪拉的。
    pub source: String,
    /// 什么时候拉的：RFC 3339 的 UTC 时刻，照字的先后就是时间的先后。
    pub fetched: String,
    /// 上次回的 `ETag`：缓存的才有。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub etag: Option<String>,
}

impl Meta {
    /// 读 `dir` 里的 `meta`；没有的、坏的是空的。
    pub fn read(dir: &Path) -> Option<Meta> {
        let text = std::fs::read_to_string(dir.join(META)).ok()?;
        lm_kernel::kernel::json::from_str(&text).ok()
    }

    /// 写成 JSON。
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

/// 缓存的那一份在哪：`<缓存>/models`（`store::paths::Paths::model_cache`）；算不出来的没有，没有目录。
#[derive(Debug, Clone)]
pub struct Places {
    /// 缓存所在的目录。
    pub cache: Option<PathBuf>,
}

/// 读目录：缓存目录里的那一份；没有、读不了、一家供应商都没有的没有。在阻塞线程里调。
pub fn load(places: &Places) -> Option<Loaded> {
    read(places)
}

/// 同 [`load`]，目录不常驻时临时读一份（施工 V-25 下：没记过的输入、要整份目录的地方）。
pub fn load_again(places: &Places) -> Option<Loaded> {
    read(places)
}

/// 缓存里那一份和它的 `meta`（没有缓存目录的没有）：起来时照它认记下的那份还能不能用（施工 V-25 下，`super::recall`）。
pub fn ranked(places: &Places) -> Vec<(CatalogSource, Option<Meta>)> {
    places
        .cache
        .as_ref()
        .map(|cache| (CatalogSource::Cache, Meta::read(cache)))
        .into_iter()
        .collect()
}

/// [`load`]、[`load_again`]：读缓存的原文，读得懂、至少一家供应商的才算。
fn read(places: &Places) -> Option<Loaded> {
    let cache = places.cache.as_ref()?;
    let text = std::fs::read_to_string(cache.join(FILE)).ok()?;
    let read = Catalog::parse(&text).ok()?;
    drop(text);
    read.catalog.providers().next()?;
    let fetched = Meta::read(cache)
        .map(|meta| meta.fetched)
        .unwrap_or_default();
    Some(Loaded {
        catalog: read.catalog,
        source: CatalogSource::Cache,
        fetched,
    })
}
