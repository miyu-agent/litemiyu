//! 取目录、后台更新（`docs/blueprint/models.md`「怎么走」第二条第 3 条，施工 8-7；LM-9 起程序不带目录，第一次也走这里）：
//! `models.catalog.update` 开着的，缓存里没有目录、或者缓存的 `fetched` 旧过 `every` 的，GET `url`，带上次的 `ETag`，连接 5 秒、
//! 整个 30 秒、最多 32 MiB（[`CONNECT`]、[`TIMEOUT`]、[`LIMIT`]）。没有谁等它：界面、发话照没有目录走，取到了再补上。
//!
//! - 200：先读一遍，至少有一家供应商才算好的；写进缓存目录（临时文件再改名，`meta` 一起，[`store`]），换上新的。目录不常驻的
//!   （施工 V-25 下）：照新目录把记下的键全部重算、原子地换上、写盘，新目录随后放掉；原来没有目录的照这一刻的配置把写过的
//!   模型记下、换成不常驻的那一份（[`crate::session::ModelData::replace_catalog`]）。304：只改 `meta` 的 `fetched`。
//! - 别的、读不了的、写不进的：失败，缓存不动。缓存里有目录的一小时后再试，没有的五分钟后再试（[`RETRY`]、[`RETRY_EMPTY`]）。
//! - 核心一直开着的，每过 `every` 再查一次；配置改了（当场生效）照新的算。缓存目录算不出来的不取。
//! - `url` 可以是环境变量的引用（施工 8-8，[`Schedule`]）：照核心的环境取，没设、设成空的到点了照失败算。
//! - 地址在本机的不走代理（测试的假服务器；同拉列表，`http.md`「客户端」第 5 条），别的照环境变量的代理、照系统和自带的证书
//!   （[`crate::http::fetcher_within`]）。
//!
//! 配置界面也照这里取（[`fetch_now`]：在调的线程上自己起单线程的运行时），取到的写进同一处缓存，核心照 [`super::reload`] 换上。

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tokio::sync::watch;

use crate::http::{Broke, Client, Get, Got, Proxy, fetcher_within, get_full, is_loopback_url};
use crate::session::ModelData;
use crate::session::blocking::in_own_thread;
use lm_kernel::config::Address;
use lm_kernel::config::merge::Resolved;
use lm_kernel::kernel::time::Timestamp;
use lm_kernel::models::catalog::{Catalog, CatalogSource, Loaded};
use lm_kernel::models::settings::CatalogSettings;

use super::catalog::{FILE, META, Meta};
use super::recall::Version;

/// 失败了多久以后再试：缓存里有目录的。
pub const RETRY: Duration = Duration::from_secs(60 * 60);

/// 失败了多久以后再试：缓存里还没有目录的（LM-9：没有目录时窗口、价格都不知道，早点再试；不频繁到每句话都连一次）。
pub const RETRY_EMPTY: Duration = Duration::from_secs(5 * 60);

/// 连上 models.dev 最多等多久（LM-9）：没网、连不上的早点认输，界面早点说「没取到，可以手填」。
pub const CONNECT: Duration = Duration::from_secs(5);

/// 整个最多多久（LM-9 从 60 秒改短）：真目录几 MB，按慢的网也够；没有谁等它。
pub const TIMEOUT: Duration = Duration::from_secs(30);

/// 最多多少字节：比真目录大出几倍的当是坏的。
pub const LIMIT: usize = 32 * 1024 * 1024;

/// 换上新目录时照的配置：这一刻的最终值（没有的只换目录、不预先记下配置里写过的模型）。
pub type ConfigNow = Arc<dyn Fn() -> Option<Resolved> + Send + Sync>;

/// 后台更新要的：交给谁、用哪个客户端、缓存在哪（`<缓存目录>/models`）、新目录的版本串怎么算。
#[derive(Clone)]
pub struct Refresher {
    /// 换上新目录的地方。
    pub data: Arc<ModelData>,
    /// GET 用的客户端；没有的照地址现造（[`client_for`]）。
    pub client: Option<Client>,
    /// 缓存的目录。
    pub cache: PathBuf,
    /// 算新目录的版本串要的（施工 V-25 下，[`super::recall::Version`]）：目录不常驻时照它把记下的那份换成新的；没有的（不知道
    /// 程序在哪）照旧常驻。
    pub version: Option<Version>,
    /// 换上新目录时照的配置（LM-9）：原来没有目录、换成不常驻的那一份时，先把配置里写过的模型记下。
    pub config: Option<ConfigNow>,
}

/// 后台更新照的：`[models.catalog]` 这一刻的最终值，地址照核心的环境取好了（施工 8-8）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Schedule {
    /// 取不取。
    pub update: bool,
    /// 从哪取：写死的照写的；是环境变量的引用的照核心的环境取，没设、设成空的是空的。
    pub url: Option<String>,
    /// 缓存旧过这么久才取。
    pub every: Duration,
}

impl Schedule {
    /// 照 `[models.catalog]` 的最终值 `settings`，环境变量的引用照 `env` 取（核心的环境：`config.md` 第九条第 5 条）。
    pub fn of(settings: CatalogSettings, env: &dyn Fn(&str) -> Option<String>) -> Schedule {
        let url = match settings.url {
            Address::Literal(text) => Some(text),
            Address::Env(name) => env(&name),
        };
        Schedule {
            update: settings.update,
            url: url.filter(|url| !url.is_empty()),
            every: settings.every,
        }
    }

    /// 缓存 `cache` 里的这一刻要不要取（配置界面用，LM-9）：开着、有地址，缓存里没有目录、或者旧过 `every` 的要取。
    pub fn due(&self, cache: &Path, now: SystemTime) -> bool {
        if !self.update || self.url.is_none() {
            return false;
        }
        let fetched = cache
            .join(FILE)
            .is_file()
            .then(|| Meta::read(cache).and_then(|meta| time(&meta.fetched)))
            .flatten();
        next_try(now, fetched, self.every, None) == Duration::ZERO
    }
}

/// 没取到目录的原因（LM-9补）：配置界面照它挑一句短的给人看（`catalog_why` 那几句），不再整句搬 reqwest 的英文原话。
/// 写成字（`Display`）是英文的一句原话，同以前交回的那一句。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unfetched {
    /// 连不上服务器：最里面那一层原话（「Connection refused (os error 111)」「dns error: …」「invalid peer certificate: …」）。
    Connect(String),
    /// 连接、整个下载等到时限也没完（[`CONNECT`]、[`TIMEOUT`]）。
    Timeout,
    /// 服务器回的不是 2xx 也不是 304：状态码。
    Status(u16),
    /// 超过 [`LIMIT`]。
    TooLarge,
    /// 下载到一半断了。
    Cut,
    /// 回的不是一份目录：不是 UTF-8、读不懂、一家供应商都没有、304 了缓存里却没有。原话。
    NotCatalog(String),
    /// 写不进缓存目录：系统的原话（「Permission denied (os error 13)」）。
    Write(String),
    /// 别的：原话最里面那一层。
    Other(String),
}

impl std::fmt::Display for Unfetched {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Unfetched::Connect(why) | Unfetched::NotCatalog(why) | Unfetched::Other(why) => {
                f.write_str(why)
            }
            Unfetched::Write(why) => write!(f, "cache not written: {why}"),
            Unfetched::Timeout => write!(f, "timed out"),
            Unfetched::Status(status) => write!(f, "HTTP {status}"),
            Unfetched::TooLarge => write!(f, "body over {LIMIT} bytes"),
            Unfetched::Cut => write!(f, "cut off"),
        }
    }
}

impl Unfetched {
    /// 一次 GET 没成（[`crate::http::Failed`]）是哪一种。
    fn of(failed: crate::http::Failed) -> Unfetched {
        match failed.kind {
            Broke::Connect => Unfetched::Connect(failed.cause),
            Broke::Timeout => Unfetched::Timeout,
            Broke::Status => Unfetched::Status(failed.status.unwrap_or_default()),
            Broke::TooLarge => Unfetched::TooLarge,
            Broke::Cut => Unfetched::Cut,
            Broke::Other => Unfetched::Other(failed.cause),
        }
    }
}

/// 取一次的结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refreshed {
    /// 拿到了新的，换上了。
    Replaced,
    /// 和缓存里的一样。
    NotModified,
    /// 没成。
    Failed,
}

/// 还要等多久再取：`fetched` 是缓存上次取的时刻（缓存里没有目录的没有：马上取），`failed` 是上次失败的时刻（缓存里有目录的
/// 一小时后，没有的五分钟后）。
pub fn next_try(
    now: SystemTime,
    fetched: Option<SystemTime>,
    every: Duration,
    failed: Option<SystemTime>,
) -> Duration {
    let due = match (failed, fetched) {
        (Some(failed), Some(_)) => failed + RETRY,
        (Some(failed), None) => failed + RETRY_EMPTY,
        (None, fetched) => fetched.map_or(now, |fetched| fetched + every),
    };
    due.duration_since(now).unwrap_or(Duration::ZERO)
}

impl Refresher {
    /// 一直跑：照 `settings`（配置改了当场换）算什么时候取，到了取一次。`settings` 的发送方没了就停。
    pub async fn run(self, mut settings: watch::Receiver<Schedule>) {
        let mut failed = None;
        loop {
            let now = settings.borrow_and_update().clone();
            let wait = now.update.then(|| {
                let fetched = self
                    .cache
                    .join(FILE)
                    .is_file()
                    .then(|| Meta::read(&self.cache).and_then(|meta| time(&meta.fetched)))
                    .flatten();
                next_try(SystemTime::now(), fetched, now.every, failed)
            });
            let due = match wait {
                Some(wait) => tokio::select! {
                    () = tokio::time::sleep(wait) => true,
                    changed = settings.changed() => match changed {
                        Ok(()) => false,
                        Err(_) => return,
                    },
                },
                None => match settings.changed().await {
                    Ok(()) => false,
                    Err(_) => return,
                },
            };
            if due {
                let refreshed = match &now.url {
                    Some(url) => self.once(url).await,
                    None => Refreshed::Failed,
                };
                failed = match refreshed {
                    Refreshed::Failed => Some(SystemTime::now()),
                    Refreshed::Replaced | Refreshed::NotModified => None,
                };
            }
        }
    }

    /// 取一次 `url`。
    pub async fn once(&self, url: &str) -> Refreshed {
        match self.fetch(url).await {
            Ok(refreshed) => refreshed,
            Err(_) => Refreshed::Failed,
        }
    }

    async fn fetch(&self, url: &str) -> Result<Refreshed, Unfetched> {
        let seen = fetched_mark(&self.cache);
        let client = match &self.client {
            Some(client) => client.clone(),
            None => client_for(url)?,
        };
        // 同一处缓存一次只取一趟（[`turn`]）：配置界面刚取过的，不再 GET，照缓存里那一份换上。
        let turn = turn(&self.cache);
        let _turn = turn.lock().await;
        let body = if fetched_mark(&self.cache).is_some_and(|now| Some(&now) != seen.as_ref()) {
            None
        } else {
            match download(&client, &self.cache, url).await? {
                None => return Ok(Refreshed::NotModified),
                Some(body) => Some(body),
            }
        };
        let (data, cache, version, config) = (
            Arc::clone(&self.data),
            self.cache.clone(),
            self.version.clone(),
            self.config.clone(),
        );
        let url = url.to_string();
        // 读、写、换上（整份目录几 MB 的小块）在单起、做完就退的线程里做，做完还堆（同起来时读目录，`super::begin`）。
        tokio::task::spawn_blocking(move || {
            let stored = in_own_thread("lm-catalog", move || {
                let loaded = match body {
                    Some((text, etag)) => store(&cache, &url, text, etag)?,
                    None => super::catalog::load(&super::catalog::Places {
                        cache: Some(cache.clone()),
                    })
                    .ok_or_else(|| {
                        Unfetched::NotCatalog("cached catalog unreadable".to_string())
                    })?,
                };
                // 版本串照写好的原文算（大小、改动时间是写完以后的）：和以后临时读回来、下次起来时算的对得上。
                let of = version.as_ref().and_then(|version| {
                    version.of(CatalogSource::Cache, Some(&cache), &loaded.fetched)
                });
                let config = config.as_ref().and_then(|config| config());
                data.replace_catalog(loaded, of, config.as_ref());
                Ok::<(), Unfetched>(())
            });
            lm_heap::trim();
            stored
        })
        .await
        .map_err(|error| Unfetched::Other(error.to_string()))??;
        Ok(Refreshed::Replaced)
    }
}

/// GET 一次 `url`，带缓存 `cache` 里上次的 `ETag`（缓存的目录本体还在才带：不然 304 了手里什么都没有）。交回原文和这一次的
/// `ETag`；304 的只改 `meta` 的 `fetched`，交回没有。
///
/// # Errors
///
/// 连不上、超时、回的不是 2xx 也不是 304、超过 [`LIMIT`]、不是 UTF-8、304 了缓存里却没有：[`Unfetched`] 的那一种。
pub async fn download(
    client: &Client,
    cache: &Path,
    url: &str,
) -> Result<Option<(String, Option<String>)>, Unfetched> {
    let meta = Meta::read(cache);
    let etag = meta
        .as_ref()
        .filter(|_| cache.join(FILE).is_file())
        .and_then(|meta| meta.etag.clone());
    let got = get_full(Get {
        client,
        url,
        headers: &[],
        etag: etag.as_deref(),
        timeout: TIMEOUT,
        limit: LIMIT,
    })
    .await
    .map_err(Unfetched::of)?;
    match got {
        Got::NotModified => {
            let mut meta = meta.filter(|_| etag.is_some()).ok_or_else(|| {
                Unfetched::NotCatalog("not modified without a cached copy".to_string())
            })?;
            meta.fetched = now_text().map_err(Unfetched::Other)?;
            write(cache, META, meta.to_json().as_bytes()).await?;
            Ok(None)
        }
        Got::Body { bytes, etag } => {
            let text = String::from_utf8(bytes)
                .map_err(|_| Unfetched::NotCatalog("catalog is not UTF-8".to_string()))?;
            Ok(Some((text, etag)))
        }
    }
}

/// 取到的原文 `text`：先读一遍（读得懂、至少有一家供应商才算好的），写进缓存目录 `cache`（原文，旁边的 `meta` 记着从 `url`
/// 取的、什么时候、`ETag`），交回读好的目录。写都是临时文件再改名（`crate::store::generated`）。在阻塞的地方调。
///
/// # Errors
///
/// 读不懂、一家供应商都没有（[`Unfetched::NotCatalog`]）、写不进（[`Unfetched::Write`]）。
pub fn store(
    cache: &Path,
    url: &str,
    text: String,
    etag: Option<String>,
) -> Result<Loaded, Unfetched> {
    let catalog = Catalog::parse(&text)
        .map_err(Unfetched::NotCatalog)?
        .catalog;
    if catalog.providers().next().is_none() {
        return Err(Unfetched::NotCatalog(
            "catalog has no providers".to_string(),
        ));
    }
    let fetched = now_text().map_err(Unfetched::Other)?;
    let meta = Meta {
        source: url.to_string(),
        fetched: fetched.clone(),
        etag,
    };
    write_now(cache, FILE, text.as_bytes())?;
    drop(text);
    write_now(cache, META, meta.to_json().as_bytes())?;
    Ok(Loaded {
        catalog,
        source: CatalogSource::Cache,
        fetched,
    })
}

/// 配置界面取一次（LM-9）：在调的线程上自己起单线程的运行时，GET、读、写进缓存目录 `cache`。取到新的交回读好的目录，没变的
/// （304）交回没有。
///
/// # Errors
///
/// 同 [`download`]、[`store`]；运行时、客户端造不出来（[`Unfetched::Other`]）。
pub fn fetch_now(cache: &Path, url: &str) -> Result<Option<Loaded>, Unfetched> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| Unfetched::Other(error.to_string()))?;
    let seen = fetched_mark(cache);
    let client = client_for(url)?;
    // 同一处缓存一次只取一趟（[`turn`]）：引擎正在取的，等它取完照缓存里那一份，不再 GET 一遍。
    let loaded = runtime.block_on(async {
        let turn = turn(cache);
        let _turn = turn.lock().await;
        if fetched_mark(cache).is_some_and(|now| Some(&now) != seen.as_ref()) {
            return super::catalog::load(&super::catalog::Places {
                cache: Some(cache.to_path_buf()),
            })
            .map(Some)
            .ok_or_else(|| Unfetched::NotCatalog("cached catalog unreadable".to_string()));
        }
        download(&client, cache, url)
            .await?
            .map(|(text, etag)| store(cache, url, text, etag))
            .transpose()
    });
    drop(client);
    drop(runtime);
    loaded
}

/// 一处缓存（`<缓存>/models`）取目录的那一把锁（LM-9补）：配置界面（[`fetch_now`]，自己的线程、自己的运行时）和引擎
/// （[`Refresher`]）在同一个进程里，两边同时缺目录时原来各 GET 一遍。现在两边都排这一把：后拿到的一方看缓存是不是在它开始
/// 等以后有没有人取过（[`fetched_mark`]），有的就不再 GET，照缓存里那一份。照缓存的路径分（测试里一个进程好几处缓存，互不等）。
fn turn(cache: &Path) -> Arc<tokio::sync::Mutex<()>> {
    static TURNS: std::sync::Mutex<Vec<(PathBuf, Arc<tokio::sync::Mutex<()>>)>> =
        std::sync::Mutex::new(Vec::new());
    let mut turns = TURNS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some((_, turn)) = turns.iter().find(|(at, _)| at == cache) {
        return Arc::clone(turn);
    }
    let turn = Arc::new(tokio::sync::Mutex::new(()));
    turns.push((cache.to_path_buf(), Arc::clone(&turn)));
    turn
}

/// 缓存 `cache` 里那一份目录的 `fetched`（没有目录、没有 `meta` 的没有）：开始等锁以前记一次，拿到锁以后再看一次，变了的是别人
/// 在这中间取过（[`turn`]）。只比字、不比时钟：测试、时钟不准的机器上 `fetched` 可以在将来。
fn fetched_mark(cache: &Path) -> Option<String> {
    cache
        .join(FILE)
        .is_file()
        .then(|| Meta::read(cache).map(|meta| meta.fetched))
        .flatten()
}
/// 取目录的客户端：地址在本机的不走代理，别的照环境变量（同拉列表）；连接最多 [`CONNECT`]。
fn client_for(url: &str) -> Result<Client, Unfetched> {
    let proxy = match is_loopback_url(url) {
        true => Proxy::Off,
        false => Proxy::FromEnvironment,
    };
    fetcher_within(proxy, CONNECT).map_err(|error| Unfetched::Other(error.to_string()))
}

/// 在阻塞线程里写 `dir/name`：先写临时文件再改名。
async fn write(dir: &Path, name: &str, bytes: &[u8]) -> Result<(), Unfetched> {
    let (dir, name, bytes) = (dir.to_path_buf(), name.to_string(), bytes.to_vec());
    tokio::task::spawn_blocking(move || write_now(&dir, &name, &bytes))
        .await
        .map_err(|error| Unfetched::Other(error.to_string()))?
}

/// 写 `dir/name`：先写临时文件再改名。
fn write_now(dir: &Path, name: &str, bytes: &[u8]) -> Result<(), Unfetched> {
    crate::store::generated::write(&dir.join(name), bytes)
        .map(|_| ())
        .map_err(|error| Unfetched::Write(error.to_string()))
}

/// `meta` 里的时刻；读不懂的当没有。
fn time(text: &str) -> Option<SystemTime> {
    let millis = u64::try_from(Timestamp::parse(text).ok()?.unix_millis()).ok()?;
    Some(UNIX_EPOCH + Duration::from_millis(millis))
}

/// 此刻，`meta` 里的写法。
fn now_text() -> Result<String, String> {
    Timestamp::from_unix_millis(now_millis())
        .map(|at| at.to_string())
        .ok_or_else(|| "the clock is out of range".to_string())
}

/// 此刻，Unix 毫秒。
pub(super) fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| {
            i64::try_from(since.as_millis()).unwrap_or(i64::MAX)
        })
}
