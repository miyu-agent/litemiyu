//! 供应商的模型列表存下来（Miyu 核心 `route/lists.rs` 的 `refresh_list`、`ModelData::set_list`）：配好的一家列出来的模型名写进
//! `<缓存>/models/providers/<编号>.json`（同 Miyu，会话的路由起来时照 [`crate::session::read_observed`] 读同一处），配置界面、
//! `/model`、开头都照它列，不用每次重拉。
//!
//! **只记名字**：Miyu 的列表还记供应商报的窗口，会话照它推模型资料，发给模型的请求（输出上限、压缩线）就跟着变；这里只记
//! 名字，窗口、最大输出、能不能调工具、看不看得了图那几格一律不写，模型资料照旧照配置、学到的、目录推，请求一个字节不变
//! （`lm_kernel::models::facts` 读列表只取这几格，测试守着）。
//!
//! 写照 `store::generated::write`：先写临时文件、同步、再改名（会等盘）：只在后台的线程里调，界面线程不调。

use std::collections::BTreeMap;
use std::io;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use lm_kernel::kernel::time::Timestamp;
use lm_kernel::models::observed::{ListedModel, ProviderList};

use crate::store::paths::Paths;

/// 存列表的目录。
fn dir(paths: &Paths) -> PathBuf {
    paths.model_cache().join("providers")
}

/// 存下来的每一家的模型名，照编号；没有的、读不了的、坏的当没有。读几个小文件，不写。
pub fn stored(paths: &Paths) -> BTreeMap<String, Vec<String>> {
    let mut lists = BTreeMap::new();
    let Ok(entries) = std::fs::read_dir(dir(paths)) else {
        return lists;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(id) = path
            .file_name()
            .and_then(|name| name.to_str())
            .and_then(|name| name.strip_suffix(".json"))
        else {
            continue;
        };
        let Some(list) = std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| ProviderList::parse(&text).ok())
        else {
            continue;
        };
        lists.insert(
            id.to_string(),
            list.models.into_iter().map(|model| model.id).collect(),
        );
    }
    lists
}

/// 存编号 `id` 那一家列出来的 `names`（只记名字，见模块说明）。会等盘：在后台的线程里调。
///
/// # Errors
///
/// 编号不能当文件名（带 `/`、`.` 开头这类），写不进。
pub fn store(paths: &Paths, id: &str, names: &[String]) -> io::Result<()> {
    if id.is_empty() || id.starts_with('.') || id.contains(['/', '\\', '\0']) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "provider id is not a file name",
        ));
    }
    let fetched = Timestamp::from_unix_millis(jiff::Timestamp::now().as_millisecond())
        .ok_or_else(|| io::Error::other("the clock is out of range"))?;
    let list = ProviderList {
        fetched,
        models: names
            .iter()
            .map(|name| ListedModel {
                id: name.clone(),
                ..ListedModel::default()
            })
            .collect(),
    };
    let path = dir(paths).join(format!("{id}.json"));
    crate::store::generated::write(&path, list.to_json().as_bytes()).map(|_| ())
}

/// 后台拉同一家最多多久一次（Miyu 核心 `route/lists.rs` 的 `REFETCH`，施工 V-26）：开配置页、选模型的框时列整份的，每家最多这么久
/// 在后台拉一次；上游换了模型，开一次就跟上。
pub const REFETCH: Duration = Duration::from_secs(60);

/// 每一家最近一次在后台去拉的时刻（Miyu `ModelData::claim_list` 记的那一份）：拉成拉不成都算拉过，拉不到的一家不会每开一次就去
/// 拉一次。只在这个进程的内存里；几份共用一张（配置界面、`/model` 拿的是同一份）。
#[derive(Debug, Clone, Default)]
pub struct Claims(Arc<Mutex<BTreeMap<String, Instant>>>);

impl Claims {
    /// 后台要拉 `id` 这一家：上一次离现在不到 `every` 的不拉，交回假；不然记下这一刻、交回真（同 Miyu `claim_list`）。
    pub fn claim(&self, id: &str, every: Duration) -> bool {
        let now = Instant::now();
        let mut claimed = self.0.lock().unwrap_or_else(PoisonError::into_inner);
        if claimed
            .get(id)
            .is_some_and(|at| now.saturating_duration_since(*at) < every)
        {
            return false;
        }
        claimed.insert(id.to_string(), now);
        true
    }
}

/// 存下的 `id` 那一家的列表是不是 `within` 以内拉的（看文件里记的 `fetched`）；没有、读不了、坏的不算。
pub fn fetched_within(paths: &Paths, id: &str, within: Duration) -> bool {
    let Some(list) = std::fs::read_to_string(dir(paths).join(format!("{id}.json")))
        .ok()
        .and_then(|text| ProviderList::parse(&text).ok())
    else {
        return false;
    };
    let now = jiff::Timestamp::now().as_millisecond();
    let age = now.saturating_sub(list.fetched.unix_millis());
    (0..i64::try_from(within.as_millis()).unwrap_or(i64::MAX)).contains(&age)
}

/// 开「供应商和模型」、开头的模型那一步、`/model` 的框时，`id` 这一家该不该在后台拉（照 Miyu 列整份的 `model.list`：每家最多
/// [`REFETCH`] 一次）。Miyu 的核心常驻，记的时刻跨着开几次终端都在；LiteMiyu 每开一个程序是一个新进程，所以另看存下的列表：
/// [`REFETCH`] 以内（别的窗口、上一次运行）拉过的也不拉，免得每起一次程序就打一遍网络。
pub fn due(paths: &Paths, claims: &Claims, id: &str) -> bool {
    !fetched_within(paths, id, REFETCH) && claims.claim(id, REFETCH)
}

#[cfg(test)]
mod tests;
