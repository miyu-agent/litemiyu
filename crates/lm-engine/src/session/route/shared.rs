//! 核心一份的模型资料（`docs/blueprint/models.md`「在哪」`route/shared.rs`、「怎么走」第二条，施工 8-7）：档案、认原厂的表、
//! 在用的目录、用出来的、供应商的列表。路由每次请求照它查资料，协议的 `model.list` 照它列模型，核心起来时读好交进来，
//! 后台拉到新目录换上。
//!
//! - 目录在写了 `ready` 以后才读完（「起草时定的」第 13 条）：要它的（造会话、载入、`model.list`）先等它（[`ModelData::wait`]）。
//!   都读不了也算读完：目录是空的。缓存里还没有目录（LM-9：程序不带目录，第一次要用到时才去取）也算读完，取到了再换上
//!   （[`ModelData::replace_catalog`]）。
//! - 目录不常驻（施工 V-25 下，`recall.rs`）：起来时换上记下的那份（[`ModelData::recalled`]），合资料只照记下的答，没记过
//!   的、要整份目录的才临时读。没有缓存目录、不知道程序在哪的（测试、`models::routes()`）照旧常驻（[`ModelData::loaded`]）。
//!   「读完」说的是记下的那份读了、版本验过了，或者常驻的整份目录读完了。
//! - 用出来的、供应商的列表是派生数据，在 `state/models/` 下：读的时候坏了当没有（核心起来时 [`read_observed`]），变了
//!   就写（先写临时文件再改名，`crate::store::generated`），写不成的记一行 `WARN`，内存里照样用。
//! - LM-8 砍了模型池：池的指针（`state/models/pools.json`，旧的数据根里留着的那个文件不读也不删）、冷却表和 `[models.cooldown]`
//!   的规矩都没了。
//! - 两个 GET 的客户端：拉列表的（照环境变量的代理，施工 8-11），探本机的服务的（不走代理：代理不会自动绕过回环，施工
//!   8-11，[`ModelData::local`]）。拉某一家的列表、`provider.test` 照地址挑用哪个（[`ModelData::fetcher_for`]，施工 8-11
//!   补）：地址落在本机的也不走代理，和探本机的服务一样。

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use tokio::sync::watch;

use lm_kernel::config::merge::Resolved;
use lm_kernel::kernel::time::Timestamp;
use lm_kernel::models::Knowledge;
use lm_kernel::models::catalog::{Loaded, Recalled};
use lm_kernel::models::matching::Vendors;
use lm_kernel::models::observed::{Learned, ProviderList};
use lm_kernel::models::profile::Profiles;

mod recall;
#[cfg(test)]
mod tests;

pub use recall::{CatalogReader, Holding, RECALL_CAP, RECALL_FILE};
use recall::{Held, Recall};

/// 核心一份的模型资料。
#[derive(Debug)]
pub struct ModelData {
    profiles: Profiles,
    vendors: Vendors,
    /// 在用的目录：外面一层是「读完了没有」，里面是常驻的整份（读没读成）或者记下的那份（施工 V-25 下）。
    catalog: watch::Sender<Option<Held>>,
    /// 目录不常驻时要的（施工 V-25 下，`recall.rs`）：怎么临时读整份目录、临时读来的那一份、补算和写盘的锁。
    recall: Recall,
    observed: Mutex<Observed>,
    /// 用出来的、供应商的列表写在哪两处（[`ModelDirs`]）。没有的不写（测试里）。
    dir: Option<ModelDirs>,
    /// 占位工具给模型看的说明（施工 8-14 补）：档案点名了占位工具的供应商，工具面里缺这几件时补上
    /// （`route/placeholder.rs`）。没读到的（测试、老数据根）是空的，空的不补。
    placeholder_tool: String,
}

/// 用出来的、供应商的列表：核心起来时从 `state/models/`、`cache/models/providers/` 读回来的。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Observed {
    /// 用出来的。
    pub learned: Learned,
    /// 供应商的列表：配好的编号 → 列表。
    pub lists: BTreeMap<String, ProviderList>,
}

impl ModelData {
    /// 档案、认原厂的表读好了，目录还没读完。用出来的、供应商的列表写进 `dir` 那两处。
    pub fn new(profiles: Profiles, vendors: Vendors, dir: Option<ModelDirs>) -> ModelData {
        ModelData {
            profiles,
            vendors,
            catalog: watch::channel(None).0,
            recall: Recall::default(),
            observed: Mutex::new(Observed::default()),
            dir,
            placeholder_tool: String::new(),
        }
    }

    /// 同一份，占位工具给模型看的说明（施工 8-14 补）：核心起来时从资源目录读进来（`resources/core/drivers/placeholder-tool.txt`）。
    #[must_use]
    pub fn with_placeholder_tool(mut self, text: String) -> ModelData {
        self.placeholder_tool = text;
        self
    }

    /// 占位工具给模型看的说明；没读到的（测试、老数据根）是空的，空的不补。
    pub fn placeholder_tool(&self) -> &str {
        &self.placeholder_tool
    }

    /// 目录读完了（读没读成都算），常驻：连同读好的用出来的、供应商的列表一起换上，等着的都放行。
    pub fn loaded(&self, catalog: Option<Loaded>, observed: Observed) {
        *self.lock() = observed;
        self.catalog
            .send_replace(Some(Held::Full(catalog.map(Arc::new))));
    }

    /// 后台取到了新目录：换上。会话下一次查资料就用它（新会话当场用，开着的会话下一个回合）。不常驻的（施工 V-25 下）照它把
    /// 记下的键全部重算、原子地换上新的一份、写盘，新目录随后放掉；原来没有目录的（LM-9：第一次取到，[`ModelData::empty`]）照
    /// `config` 把配置里写过的模型记下、换成不常驻的那一份、写盘，新目录随后放掉。`of` 是新目录的版本串（和记下的那份同一个
    /// 算法），算不出来的从此改成常驻这一份（版本对不上就认不出记下的是哪一份）。
    pub fn replace_catalog(&self, catalog: Loaded, of: Option<String>, config: Option<&Resolved>) {
        let held = self.catalog.borrow().clone();
        match (held, of) {
            (Some(Held::Recalled(current)), Some(of)) => {
                self.swap_refreshed(&current, catalog, &of);
                // 新目录只借来重算，放掉了：还给系统（23 F3）。
                lm_heap::trim();
            }
            (Some(Held::Full(None)), Some(of)) if self.reads_later() => {
                let empty = Recalled::new(&of, catalog.source, &catalog.fetched);
                let recalled = match config {
                    Some(config) => self.fill(empty, &catalog, config),
                    None => empty,
                };
                drop(catalog);
                self.adopt(recalled);
                lm_heap::trim();
            }
            _ => {
                self.catalog
                    .send_replace(Some(Held::Full(Some(Arc::new(catalog)))));
            }
        }
    }

    /// 等目录读完：常驻的整份目录读完（读没读成都算），或者不常驻的记下的那份读了、版本验过了（施工 V-25 下）。
    pub async fn wait(&self) {
        let mut receiver = self.catalog.subscribe();
        // 发的一方就是自己，`wait_for` 只会因为它没了出错：那时也不用等了。
        if receiver.wait_for(Option::is_some).await.is_err() {}
    }

    /// 目录读完了（同 [`ModelData::wait`]，施工 8-18）：配置服务查思考强度的档位以前看它，读完以前不查。
    pub fn is_loaded(&self) -> bool {
        self.catalog.borrow().is_some()
    }

    /// 整份目录：常驻的就是那一份；不常驻的临时读一份（[`ModelData::hold`]，慢，在阻塞线程里调），借完放掉。还没读完、
    /// 都读不了的没有。
    pub fn catalog(&self) -> Option<Arc<Loaded>> {
        self.hold().into_catalog()
    }

    /// 借着手头的资料做一件事：用出来的、列表的锁拿着，做完就放开，别在里面等。配置里写过、真问过的用它（路由、回合开始、池的
    /// 成员、配置检查）：不常驻时没记过的临时读整份目录、记下（施工 V-25 下）。`work` 可能做两遍、作废的那一遍看到的是没有
    /// 目录时的答：只放查资料的纯活，别在里面断言（见 `recall.rs` 的 `asking`）。
    /// [`Knowledge::catalog`] 不常驻时是空的：要整份目录的用 [`ModelData::with_holding`]。
    pub fn with<R>(&self, work: impl FnMut(&Knowledge<'_>) -> R) -> R {
        self.asking(true, work)
    }

    /// 同 [`ModelData::with`]，只是没记过的不记：临时问一句的用它（`provider.test` 试一家还没写进配置的）。
    pub fn with_once<R>(&self, work: impl FnMut(&Knowledge<'_>) -> R) -> R {
        self.asking(false, work)
    }

    /// 重读存下来的供应商的列表（配置界面、命令行拉到了新的，`lm_engine::providers::lists`）：换上，`/model` 下一次列就有。
    /// 只读几个小文件；没有数据目录的（应急模式、测试）不读。
    pub fn reload_lists(&self) {
        let Some(dirs) = &self.dir else {
            return;
        };
        let lists = read_observed(dirs).lists;
        self.lock().lists = lists;
    }

    /// 档案。
    pub fn profiles(&self) -> &Profiles {
        &self.profiles
    }

    /// 记下用出来的窗口：比手头的小才记，记了写盘、记一行 `INFO learned window`（第二条第 9 条）。
    pub fn learn(&self, provider: &str, model: &str, window: u64, at: Timestamp) {
        let mut observed = self.lock();
        if !observed.learned.learn(provider, model, window, at) {
            return;
        }
        let text = observed.learned.to_json();
        drop(observed);
        self.write(Shelf::State, "learned.json", &text);
    }

    fn lock(&self) -> MutexGuard<'_, Observed> {
        self.observed.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// 写 `shelf` 那一处下的 `name`。写不成的记一行，内存里照样用：丢了重新学、重新拉。
    fn write(&self, shelf: Shelf, name: &str, text: &str) {
        let Some(dirs) = &self.dir else {
            return;
        };
        let path = name
            .split('/')
            .fold(dirs.of(shelf).to_path_buf(), |path, part| path.join(part));
        drop(crate::store::generated::write(&path, text.as_bytes()));
    }
}

/// 路由写东西的两处（施工 FH-2，设计 `55-数据根仿FHS.md`）：`state` 放用出来的（学到的上下文窗口、池的指针、供应商改名），
/// 丢了要重新学；`cache` 放拉到的（供应商的模型列表、目录答的记录），能重新拉。位置照 `DataRoot::model_state`、
/// `DataRoot::model_cache`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelDirs {
    /// 用出来的。
    pub state: PathBuf,
    /// 拉到的。
    pub cache: PathBuf,
}

impl ModelDirs {
    /// 两处放在同一个目录 `dir` 里（测试照它造；FH-3 以前数据根里也是同一个 `state/models/`）。
    pub fn in_one(dir: PathBuf) -> ModelDirs {
        ModelDirs {
            state: dir.clone(),
            cache: dir,
        }
    }

    /// `shelf` 那一处。
    fn of(&self, shelf: Shelf) -> &Path {
        match shelf {
            Shelf::State => &self.state,
            Shelf::Cache => &self.cache,
        }
    }
}

/// 写在 [`ModelDirs`] 的哪一处。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Shelf {
    /// 用出来的。
    State,
    /// 拉到的。
    Cache,
}

/// 读 `dirs` 两处下的用出来的、供应商的列表、池的指针（在阻塞线程里调）：没有的、读不了的、坏的当没有，坏的记一行 `WARN`。
pub fn read_observed(dirs: &ModelDirs) -> Observed {
    let read = |path: &Path| std::fs::read_to_string(path).ok();
    let dir = &dirs.state;
    let learned = read(&dir.join("learned.json"))
        .and_then(|text| warn_if_broken(Learned::parse(&text)))
        .unwrap_or_default();
    let mut lists = BTreeMap::new();
    if let Ok(entries) = std::fs::read_dir(dirs.cache.join("providers")) {
        for entry in entries.flatten() {
            let path = entry.path();
            let id = path
                .file_name()
                .and_then(|name| name.to_str())
                .and_then(|name| name.strip_suffix(".json"));
            let (Some(id), Some(text)) = (id, read(&path)) else {
                continue;
            };
            if let Some(list) = warn_if_broken(ProviderList::parse(&text)) {
                lists.insert(id.to_string(), list);
            }
        }
    }
    Observed { learned, lists }
}

/// 坏了的记一行，当没有。
fn warn_if_broken<T>(read: Result<T, String>) -> Option<T> {
    read.map_err(|_| ()).ok()
}
