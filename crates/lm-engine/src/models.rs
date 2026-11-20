//! 模型这一块碰文件的一半（`docs/00-设计.md` 第五节第 4 条）。纯的读法在 `lm_kernel::models`。
//!
//! 从 Miyu `miyu-core/src/models.rs` 搬来（L3 三下）：起来时读编进程序的供应商档案（`models/profiles.toml`）、认原厂的表
//! （`models/vendors.toml`），造每个会话的路由（[`crate::session::Routes`]）和核心一份的模型资料（[`crate::session::ModelData`]，
//! [`prepare`]）。在阻塞线程里读目录（[`catalog`]）、用出来的、供应商的列表，读完放行等着它的（[`begin`]）；再在后台取、更新目录
//! （[`refresh`]，[`follow`]）。档案、认原厂的表是 TOML，这里读成 JSON 交给 `lm_kernel::models`。
//!
//! 程序里不带目录（LM-9，2026-11-19 项目主人定）：models.dev 的目录第一次要用到时才去取，存进缓存目录（[`refresh`]），以后读
//! 缓存。缓存里没有、取不到的时候目录是空的，照常起来：模型的资料照配置写的、用出来的，窗口不知道的不设默认值（内核照供应商回的
//! 「上下文超长」被动压缩），没有价格的不算花费。平时查模型照记下来的小表（`lm_kernel::models::catalog` 的 `Recalled`，
//! [`recall`]），整份目录只在没记过的输入、或者目录换了要重算小表时才读。
//!
//! Miyu 这里另有的没搬：供应商的图标（`logos.rs`）、解过的目录另存一份（`digest.rs`，见 [`catalog`]）、第一次接入探本机的服务
//! 和拉列表的客户端（L8）；跟着配置换目录的更新设置、记下配置里写过的（`catalog_settings`、
//! `follow_recall`）要配置服务，接主程序（L4）时搬。

pub mod catalog;
pub mod recall;
pub mod refresh;

use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::watch;

use lm_kernel::config::merge::Resolved;
use lm_kernel::models::catalog::Recalled;
use lm_kernel::models::matching::Vendors;
use lm_kernel::models::profile::Profiles;

use crate::http::{Proxy, client};
use crate::session::blocking::in_own_thread;
use crate::session::{IDLE, ModelData, ModelDirs, Observed, Routes, read_observed};
use catalog::Places;
use recall::{Read, Version};
use refresh::{Refresher, Schedule};

/// 出厂的供应商档案。
const PROFILES: &str = include_str!("../../../resources/models/profiles.toml");

/// 出厂的认原厂的表。
const VENDORS: &str = include_str!("../../../resources/models/vendors.toml");

/// 占位工具给模型看的说明（施工 8-14 补）。
const PLACEHOLDER_TOOL: &str = include_str!("../../../resources/core/drivers/placeholder-tool.txt");

/// 算目录版本串要的（[`recall::Version`]）：照出厂的认原厂的表算。
pub fn version() -> Version {
    Version::of_vendors(VENDORS)
}

/// 编进程序的档案和认原厂的表（配置界面列目录里的几家要它们，`crate::settings::catalog`）。
///
/// # Errors
///
/// 读不进来（安装坏了）。
pub fn shipped() -> Result<(Profiles, Vendors), String> {
    Ok((profiles(PROFILES)?, vendors(VENDORS)?))
}

/// 造路由：读编进程序的档案、认原厂的表、占位工具的说明，造两个客户端（照环境变量走代理的、不走代理的：地址落在本机的用它，
/// 施工 8-11 补）。用出来的、拉到的写进 `state` 那两处（没有的不写）。目录还没读：读完以前造会话、载入的等着（[`begin`]）。
///
/// # Errors
///
/// 出厂的档案、表读不进来（安装坏了），造不出客户端。
pub fn prepare(state: Option<ModelDirs>) -> Result<Routes, String> {
    let profiles = profiles(PROFILES)?;
    let vendors = vendors(VENDORS)?;
    let direct = client(Proxy::Off).map_err(|error| error.to_string())?;
    let client = client(Proxy::FromEnvironment).map_err(|error| error.to_string())?;
    let data = ModelData::new(profiles, vendors, state)
        .with_placeholder_tool(PLACEHOLDER_TOOL.to_string());
    Ok(Routes {
        client,
        direct,
        data: Arc::new(data),
        idle: IDLE,
    })
}

/// 在阻塞线程里读目录、用出来的、供应商的列表，读完交给 `data`、放行等着的（施工 8-7）。不常驻（施工 V-25 下，[`recall`]）：
/// 有缓存目录 `places.cache`、有 `state` 的先读记下的那份，版本对上了就不读整份目录；对不上的读整份目录，照配置 `config` 把写过
/// 的都查一遍记下、写盘（`INFO catalog recall built`），再放掉整份目录、把空着的堆还给系统。缓存里还没有目录的（LM-9：程序不带
/// 目录，第一次要用到时才去取）目录是空的，放行；以后取到了照 [`ModelData::replace_catalog`] 换上。没有缓存目录的照旧常驻。不联网。
pub fn begin(
    data: Arc<ModelData>,
    places: Places,
    state: Option<ModelDirs>,
    config: Option<Resolved>,
) -> Reading {
    let reading = Arc::clone(&data);
    let version = places.cache.as_ref().map(|_| version());
    let read_version = version.clone();
    let cache = places.cache.clone();
    // 读、解、放整份目录（几 MB 的小块）在单起、做完就退的线程里做，退完再还堆（占用线一）：在常驻的阻塞线程里做的话，放掉的
    // 小块有几块留在那条线程自己的缓存（glibc 的 tcache）里，钉着那个分配区的几百 KB；线程闲两秒退的时候才还回分配区，这时
    // 引擎的那一下还堆要是已经做过，就一直留到下一轮。
    let task = tokio::task::spawn_blocking(move || {
        in_own_thread("lm-catalog", move || {
            read(reading, places, state, config, read_version)
        });
        lm_heap::trim();
    });
    Reading {
        data,
        task,
        version,
        cache,
    }
}

/// 应急模式的一次运行（`litemiyu ask --rescue`，设计第五节第 16 条）的目录：没有目录（LM-9：程序不带目录，不读也不写缓存、
/// 不联网；全屏界面读缓存里现成的，[`read_only`]）。模型的资料照配置写的
/// 和缺省，窗口不知道的照供应商回的「上下文超长」被动压缩。
pub fn without_catalog(data: &ModelData) {
    data.loaded(None, Observed::default());
}

/// 应急模式的全屏界面（设计第五节第 16 条）的目录：只读、不联网。`data` 要是不带写盘那两处造的（[`prepare`] 给 `None`）：
/// 缓存里现成的目录、记下的那份、用出来的、供应商的列表照平常读（[`begin`]，位置照 `dirs`），该记下的只记在内存里，一个
/// 字节都不写；读不到、读不懂的（数据目录不存在、缓存里没有）目录是空的，同 [`without_catalog`]。不接后台取、更新
/// （[`follow`]）。读的线程 panic 了的目录当空的放行（同 [`follow`]）。
pub fn read_only(data: Arc<ModelData>, dirs: ModelDirs, config: Resolved) {
    let places = Places {
        cache: Some(dirs.cache.clone()),
    };
    let Reading { data, task, .. } = begin(data, places, Some(dirs), Some(config));
    tokio::spawn(async move {
        if task.await.is_err() {
            data.loaded(None, Observed::default());
        }
    });
}

/// 读目录、用出来的、供应商的列表，读完交给 `reading`、放行等着的（[`begin`] 在单起的线程里调）。
fn read(
    reading: Arc<ModelData>,
    places: Places,
    state: Option<ModelDirs>,
    config: Option<Resolved>,
    read_version: Option<Version>,
) {
    let observed = state.as_ref().map(read_observed).unwrap_or_default();
    let at = state.as_ref().map(|dirs| dirs.cache.clone());
    let early = recall::early(&places, read_version.as_ref(), at.as_deref());
    let reader = read_version
        .clone()
        .map(|version| recall::reader(places.clone(), version));
    match (early, reader, config) {
        (Read::Recalled(recalled), Some(reader), config) => {
            reading.recalled_as_read(recalled, observed, reader);
            // 配置改过的（新写的模型、改了地址的供应商）记下的那份里没有：趁还在这条做完就退的线程里照整份目录补上、写盘。
            // 不补的话第一句话在 tokio 的线程上同步读整份目录，读剩的碎块一直留在那条线程的分配区里（核心 四：约 500 kB）。
            if let Some(config) = config {
                reading.remember_configured(&config);
            }
        }
        (Read::Full(Some(loaded), Some(of)), Some(reader), Some(config)) => {
            let empty = Recalled::new(&of, loaded.source, &loaded.fetched);
            let recalled = reading.fill(empty, &loaded, &config);
            drop(loaded);
            reading.recalled(recalled, observed, reader);
        }
        // 缓存里还没有目录（LM-9）：空着放行，装上临时读整份目录的办法，取到了照它换成不常驻的那一份。
        (Read::Full(None, _), Some(reader), _) => reading.empty(observed, reader),
        (Read::Full(loaded, _), _, _) => reading.loaded(loaded, observed),
        (Read::Recalled(_), None, _) => reading.loaded(None, observed),
    }
}

/// 正在读的目录（[`begin`]）：读完以后接着在后台取、更新（[`follow`]）。
pub struct Reading {
    data: Arc<ModelData>,
    task: tokio::task::JoinHandle<()>,
    version: Option<Version>,
    cache: Option<PathBuf>,
}

/// 读完以后在后台取、更新目录（施工 8-7，LM-9，[`refresh`]）：照 `settings`（`[models.catalog]`）到点去取，取到的写进缓存目录、
/// 照 `config`（这一刻的配置）把写过的模型记下、换上。缓存里没有目录的一接上就取（调的一方在第一次要用到目录时才接，起来时
/// 不接）。读的线程 panic 了的记一行 `ERROR catalog read panicked`，目录当空的放行（只在测试的配置下走得到：发行配置
/// panic = abort（`docs/00-设计.md` 第九节））。没有缓存目录的不取。
pub fn follow(reading: Reading, settings: watch::Receiver<Schedule>, config: refresh::ConfigNow) {
    tokio::spawn(async move {
        let Reading {
            data,
            task,
            version,
            cache,
        } = reading;
        if task.await.is_err() {
            data.loaded(None, Observed::default());
        }
        let Some(cache) = cache else {
            return;
        };
        Refresher {
            data,
            client: None,
            cache,
            version,
            config: Some(config),
        }
        .run(settings)
        .await;
    });
}

/// 缓存目录 `cache` 里的目录换过了（配置界面取到的，LM-9）：在单起、做完就退的线程里先照文件的大小、改动时间、`meta` 算版本串，
/// 和在用的那一份一样的不读；不一样的读回来，照 `config` 记下、换上（[`ModelData::replace_catalog`]）。读不了的不动。
pub fn reload(
    data: Arc<ModelData>,
    cache: PathBuf,
    config: Option<Resolved>,
) -> tokio::task::JoinHandle<()> {
    tokio::task::spawn_blocking(move || {
        in_own_thread("lm-catalog", move || {
            let fetched = catalog::Meta::read(&cache)
                .map(|meta| meta.fetched)
                .unwrap_or_default();
            let of = version().of(
                lm_kernel::models::catalog::CatalogSource::Cache,
                Some(&cache),
                &fetched,
            );
            if of.is_none() || of == data.recalled_of() {
                return;
            }
            let places = Places { cache: Some(cache) };
            let Some(loaded) = catalog::load(&places) else {
                return;
            };
            data.replace_catalog(loaded, of, config.as_ref());
        });
        lm_heap::trim();
    })
}

/// 读认原厂的表（`models/vendors.toml`）。
///
/// # Errors
///
/// TOML 写错了、表的写法不对。
pub fn vendors(text: &str) -> Result<Vendors, String> {
    let document = toml_edit::Document::parse(text).map_err(|error| {
        format!(
            "models/vendors.toml not readable: {}",
            error.message().trim()
        )
    })?;
    Vendors::parse(&table(document.as_table())?)
}

/// 读供应商档案（`models/profiles.toml`）。
///
/// # Errors
///
/// TOML 写错了、档案的写法不对（照 `lm_kernel::models::profile::Profiles::parse`）。
pub fn profiles(text: &str) -> Result<Profiles, String> {
    let document = toml_edit::Document::parse(text).map_err(|error| {
        format!(
            "models/profiles.toml not readable: {}",
            error.message().trim()
        )
    })?;
    let json = table(document.as_table())?;
    Profiles::parse(&json)
}

/// TOML 的表照样转成 JSON 的对象。
fn table(table: &dyn toml_edit::TableLike) -> Result<serde_json::Value, String> {
    let mut object = serde_json::Map::new();
    for (key, node) in table.iter() {
        let value = match node {
            toml_edit::Item::Value(value) => value_json(value)?,
            toml_edit::Item::Table(inner) => self::table(inner)?,
            toml_edit::Item::ArrayOfTables(array) => serde_json::Value::Array(
                array
                    .iter()
                    .map(|inner| self::table(inner))
                    .collect::<Result<_, _>>()?,
            ),
            toml_edit::Item::None => continue,
        };
        object.insert(key.to_string(), value);
    }
    Ok(serde_json::Value::Object(object))
}

/// TOML 的一个值转成 JSON：日期没人写，写了的报错。
fn value_json(value: &toml_edit::Value) -> Result<serde_json::Value, String> {
    Ok(match value {
        toml_edit::Value::String(text) => serde_json::Value::from(text.value().as_str()),
        toml_edit::Value::Integer(number) => serde_json::Value::from(*number.value()),
        toml_edit::Value::Float(number) => serde_json::Value::from(*number.value()),
        toml_edit::Value::Boolean(on) => serde_json::Value::from(*on.value()),
        toml_edit::Value::Array(array) => {
            serde_json::Value::Array(array.iter().map(value_json).collect::<Result<_, _>>()?)
        }
        toml_edit::Value::InlineTable(inner) => table(inner)?,
        toml_edit::Value::Datetime(_) => {
            return Err("models/profiles.toml not readable: a date is not expected".to_string());
        }
    })
}

#[cfg(test)]
mod tests;
