//! 读目录、取目录、后台更新（施工 8-7，`docs/blueprint/models.md`「怎么走」第二条第 1 到 3 条，从 Miyu `miyu-core/tests/catalog.rs`
//! 搬来；LM-9 起程序不带快照）：缓存里有的读、没有的、坏了的、半截的是空的；取到的写进缓存、换上，304、失败五分钟或一小时后
//! 再试、关掉 `update` 不取；起来时不连网；配置界面在自己的线程上取一次。用假服务器，不连外网。地址写成环境变量的引用的照环境取
//! （施工 8-8）。Miyu 这里另有解过的目录另存一份（`digest`）的和读快照的量尺：另存那一份没搬。

mod recall;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use serde_json::json;
use tokio::sync::watch;

use lm_engine::http::testkit::{Piece, Reply, Server};
use lm_engine::http::{Proxy, fetcher};
use lm_engine::models::catalog::{FILE, META, Meta, Places, load};
use lm_engine::models::refresh::{
    CONNECT, LIMIT, RETRY, RETRY_EMPTY, Refreshed, Refresher, Schedule, TIMEOUT, Unfetched,
    download, fetch_now, next_try, store,
};
use lm_engine::session::{ModelData, Observed};
use lm_kernel::config::secret::Reference;
use lm_kernel::config::{Address, Value, Values};
use lm_kernel::models::catalog::CatalogSource;
use lm_kernel::models::matching::Vendors;
use lm_kernel::models::profile::Profiles;
use lm_kernel::models::settings::CatalogSettings;

/// 用完就删的临时目录。
struct Dir(PathBuf);

impl Dir {
    fn new() -> Dir {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "litemiyu-catalog-{}-{}-{n}",
            std::process::id(),
            stamp()
        ));
        std::fs::create_dir_all(&dir).expect("建得了");
        Dir(dir)
    }

    fn at(&self, name: &str) -> PathBuf {
        let dir = self.0.join(name);
        std::fs::create_dir_all(&dir).expect("建得了");
        dir
    }
}

impl Drop for Dir {
    #[expect(clippy::let_underscore_must_use, reason = "删不掉就留在临时目录里")]
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// 只有一家 `p`、一个模型 `model` 的目录。
fn catalog(model: &str) -> String {
    json!({"p": {"id": "p", "models": {model: {"limit": {"context": 1000}}}}}).to_string()
}

/// 在 `dir` 写一份目录 `text`，`fetched` 是什么时候取的（没有的不写 `meta`）。
fn put(dir: &Path, text: &str, fetched: Option<&str>, etag: Option<&str>) {
    std::fs::write(dir.join(FILE), text).expect("写得进");
    if let Some(fetched) = fetched {
        let meta = Meta {
            source: "https://models.dev/api.json".to_string(),
            fetched: fetched.to_string(),
            etag: etag.map(str::to_string),
        };
        std::fs::write(dir.join(META), meta.to_json()).expect("写得进");
    }
}

/// 在用的目录里有没有模型 `model`，和它是哪一份。
fn has(places: &Places, model: &str) -> Option<(bool, CatalogSource)> {
    let loaded = load(places)?;
    Some((loaded.catalog.model("p", model).is_some(), loaded.source))
}

const OLD: &str = "2026-09-01T00:00:00.000Z";

/// 程序不带目录（LM-9）：缓存里取到过的照读（以前的版本留下的、带 `ETag` 的 `meta` 照认，`meta` 没有的也读）；缓存里没有、
/// 坏了、半截、读出来一家都没有、没有缓存目录的，是没有目录。
#[test]
fn the_cached_copy_is_read_and_anything_else_is_no_catalog() {
    let dir = Dir::new();
    let cache = dir.at("cache");
    let places = Places {
        cache: Some(cache.clone()),
    };
    assert_eq!(has(&places, "x"), None, "缓存里还没有");
    put(&cache, &catalog("old"), Some(OLD), Some("\"e\""));
    assert_eq!(
        has(&places, "old"),
        Some((true, CatalogSource::Cache)),
        "以前留下的照认"
    );
    assert_eq!(load(&places).map(|loaded| loaded.fetched), Some(OLD.into()));
    std::fs::remove_file(cache.join(META)).expect("删得掉");
    assert_eq!(
        has(&places, "old"),
        Some((true, CatalogSource::Cache)),
        "没有 meta 的照读"
    );
    let full = catalog("half");
    for broken in [
        "{not json",
        &full[..full.len() / 2],
        "{}",
        "[]",
        "\u{feff}\u{0}",
    ] {
        put(&cache, broken, Some(OLD), None);
        assert_eq!(has(&places, "x"), None, "坏的当没有：{broken}");
    }
    std::fs::write(cache.join(FILE), [0xff, 0xfe, 0x00]).expect("写得进");
    assert_eq!(has(&places, "x"), None, "不是 UTF-8");
    assert_eq!(has(&Places { cache: None }, "x"), None, "没有缓存目录");
}

#[test]
fn the_next_try_waits_for_every_or_after_a_failure() {
    let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000_000);
    let day = Duration::from_secs(86_400);
    assert_eq!(
        next_try(now, None, day, None),
        Duration::ZERO,
        "缓存里没有：马上取"
    );
    let fetched = now - Duration::from_secs(3600);
    assert_eq!(
        next_try(now, Some(fetched), day, None),
        day - Duration::from_secs(3600)
    );
    assert_eq!(
        next_try(now, Some(now - 2 * day), day, None),
        Duration::ZERO,
        "旧过了"
    );
    let minute_ago = Some(now - Duration::from_secs(60));
    assert_eq!(
        next_try(now, Some(now - 2 * day), day, minute_ago),
        RETRY - Duration::from_secs(60),
        "缓存里有：失败了一小时后再试"
    );
    assert_eq!(
        next_try(now, None, day, minute_ago),
        RETRY_EMPTY - Duration::from_secs(60),
        "缓存里没有：失败了五分钟后再试"
    );
    assert_eq!(RETRY, Duration::from_secs(3600));
    assert_eq!(RETRY_EMPTY, Duration::from_secs(300));
    assert_eq!(CONNECT, Duration::from_secs(5), "连不上早点认输");
    assert_eq!(TIMEOUT, Duration::from_secs(30));
}

/// 读完了、没有目录的模型资料。
fn data() -> Arc<ModelData> {
    let data = ModelData::new(Profiles::default(), Vendors::default(), None);
    data.loaded(None, Observed::default());
    Arc::new(data)
}

fn refresher(dir: &Dir, data: &Arc<ModelData>) -> Refresher {
    Refresher {
        data: Arc::clone(data),
        client: Some(fetcher(Proxy::Off).expect("造得出客户端")),
        cache: dir.at("cache"),
        version: None,
        config: None,
    }
}

fn ok(body: &str, etag: &str) -> Reply {
    Reply {
        status: 200,
        headers: vec![("ETag".to_string(), etag.to_string())],
        body: vec![Piece::Bytes(body.as_bytes().to_vec())],
    }
}

#[tokio::test]
async fn a_refresh_writes_the_cache_and_swaps_the_catalog() {
    let dir = Dir::new();
    let data = data();
    let refresher = refresher(&dir, &data);
    let server = Server::start(vec![
        ok(&catalog("fresh"), "\"v1\""),
        Reply::error(304, &[], ""),
        Reply::error(503, &[], "busy"),
        ok("{}", "\"v2\""),
        ok("[1, 2]", "\"v3\""),
        Reply {
            status: 200,
            headers: Vec::new(),
            body: vec![Piece::Bytes(vec![0xff, 0xfe, b'{'])],
        },
    ])
    .await;
    let url = format!("{}/api.json", server.base_url);
    // 200：写进缓存（连同 meta 和 ETag），换上。
    assert_eq!(refresher.once(&url).await, Refreshed::Replaced);
    let loaded = data.catalog().expect("换上了");
    assert_eq!(loaded.source, CatalogSource::Cache);
    assert!(loaded.catalog.model("p", "fresh").is_some());
    assert_eq!(
        std::fs::read_to_string(refresher.cache.join(FILE)).expect("写了"),
        catalog("fresh"),
        "原样的字节"
    );
    let meta = Meta::read(&refresher.cache).expect("写了 meta");
    assert_eq!(
        (meta.source.as_str(), meta.etag.as_deref()),
        (url.as_str(), Some("\"v1\""))
    );
    assert_eq!(meta.fetched, loaded.fetched);
    // 304：带上次的 ETag 去问，只改 meta 的时刻。
    let mut older = meta.clone();
    older.fetched = OLD.to_string();
    std::fs::write(refresher.cache.join(META), older.to_json()).expect("写得进");
    assert_eq!(refresher.once(&url).await, Refreshed::NotModified);
    assert_eq!(server.received()[1].header("if-none-match"), Some("\"v1\""));
    let after = Meta::read(&refresher.cache).expect("有");
    assert!(after.fetched.as_str() > OLD, "{}", after.fetched);
    assert_eq!(after.etag.as_deref(), Some("\"v1\""));
    // 503、一家供应商都没有的、不是一张表的、不是 UTF-8 的：失败，缓存不动。
    for _ in 0..4 {
        assert_eq!(refresher.once(&url).await, Refreshed::Failed);
    }
    assert_eq!(
        std::fs::read_to_string(refresher.cache.join(FILE)).expect("还在"),
        catalog("fresh")
    );
}

/// 外来数据太大（LM-9）：回的字节超过 [`LIMIT`] 的不收，缓存不动，原来没有目录的照旧没有。
#[tokio::test]
async fn a_catalog_over_the_limit_is_refused() {
    let dir = Dir::new();
    let data = data();
    let refresher = refresher(&dir, &data);
    let huge = vec![b' '; LIMIT + 1];
    let server = Server::start(vec![Reply {
        status: 200,
        headers: Vec::new(),
        body: vec![Piece::Bytes(huge)],
    }])
    .await;
    let url = format!("{}/api.json", server.base_url);
    assert_eq!(refresher.once(&url).await, Refreshed::Failed);
    assert!(!refresher.cache.join(FILE).exists(), "没写进缓存");
    assert!(data.catalog().is_none());
}

/// 名字里有怪字符的（LM-9）：编号、模型名带控制字符的跳过，显示名里的控制字符去掉，别的照收；收下的原文原样存。
#[tokio::test]
async fn odd_names_from_the_network_are_cleaned_when_read() {
    let dir = Dir::new();
    let data = data();
    let refresher = refresher(&dir, &data);
    let text = json!({
        "p": {"name": "P\u{1b}[2J 家", "models": {"模型/a \"b\"": {"name": "名\u{7}字"}, "x\ny": {}}},
        "q\u{1b}": {"models": {}}
    })
    .to_string();
    let server = Server::start(vec![ok(&text, "\"v1\"")]).await;
    let url = format!("{}/api.json", server.base_url);
    assert_eq!(refresher.once(&url).await, Refreshed::Replaced);
    let loaded = data.catalog().expect("换上了");
    let p = loaded.catalog.provider("p").expect("有");
    assert_eq!(p.name.as_deref(), Some("P[2J 家"));
    assert_eq!(p.models.len(), 1);
    let model = loaded.catalog.model("p", "模型/a \"b\"").expect("有");
    assert_eq!(model.name.as_deref(), Some("名字"));
    assert_eq!(
        loaded.catalog.providers().count(),
        1,
        "带控制字符的编号跳过"
    );
    assert_eq!(
        std::fs::read_to_string(refresher.cache.join(FILE)).expect("写了"),
        text
    );
}

/// 没取到的分得出是哪一种（LM-9补：配置界面照它说一句短的）：连不上的只留原话最里面那一层，回错的状态码、太大、不是 UTF-8、
/// 读不懂的各是一种。
#[tokio::test]
async fn a_failed_fetch_says_which_kind() {
    let dir = Dir::new();
    let cache = dir.at("cache");
    let client = fetcher(Proxy::Off).expect("造得出客户端");
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|listener| listener.local_addr())
        .expect("占得到端口")
        .port();
    let refused = download(
        &client,
        &cache,
        &format!("http://127.0.0.1:{port}/api.json"),
    )
    .await;
    match refused {
        Err(Unfetched::Connect(cause)) => {
            assert!(
                !cause.is_empty() && !cause.contains("error sending request"),
                "{cause}"
            );
        }
        other => panic!("连不上：{other:?}"),
    }
    let server = Server::start(vec![
        Reply::error(503, &[], "busy"),
        Reply {
            status: 200,
            headers: Vec::new(),
            body: vec![Piece::Bytes(vec![b' '; LIMIT + 1])],
        },
        Reply {
            status: 200,
            headers: Vec::new(),
            body: vec![Piece::Bytes(vec![0xff, 0xfe, b'{'])],
        },
    ])
    .await;
    let url = format!("{}/api.json", server.base_url);
    assert_eq!(
        download(&client, &cache, &url).await,
        Err(Unfetched::Status(503))
    );
    assert_eq!(
        download(&client, &cache, &url).await,
        Err(Unfetched::TooLarge)
    );
    assert!(matches!(
        download(&client, &cache, &url).await,
        Err(Unfetched::NotCatalog(_))
    ));
    for garbled in ["<html>", "[1, 2]", "{}"] {
        assert!(
            matches!(
                store(&cache, &url, garbled.to_string(), None),
                Err(Unfetched::NotCatalog(_))
            ),
            "{garbled}"
        );
    }
    assert!(!cache.join(FILE).exists(), "一样都没收");
}

/// 连不上的（LM-9：没网、端口没人听）：很快认输，照失败算，缓存不动。
#[tokio::test]
async fn an_unreachable_catalog_fails_quickly() {
    let dir = Dir::new();
    let data = data();
    let refresher = Refresher {
        client: None,
        ..refresher(&dir, &data)
    };
    // 先占一个端口再放掉：一般没人接着用，连上去被拒。
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .and_then(|listener| listener.local_addr())
        .expect("占得到端口")
        .port();
    let started = std::time::Instant::now();
    let url = format!("http://127.0.0.1:{port}/api.json");
    assert_eq!(refresher.once(&url).await, Refreshed::Failed);
    assert!(started.elapsed() < CONNECT + Duration::from_secs(1));
    assert!(data.catalog().is_none());
    assert!(!refresher.cache.join(FILE).exists());
}

#[tokio::test]
async fn the_loop_fetches_when_due_and_not_when_update_is_off() {
    let dir = Dir::new();
    let data = data();
    let server = Server::start(vec![ok(&catalog("looped"), "\"v1\"")]).await;
    let url = format!("{}/api.json", server.base_url);
    let settings = |update: bool| Schedule {
        update,
        url: Some(url.clone()),
        every: Duration::from_secs(86_400),
    };
    // 关着：不取。
    let (sender, receiver) = watch::channel(settings(false));
    let running = tokio::spawn(refresher(&dir, &data).run(receiver));
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(server.received().is_empty(), "关掉 update 不取");
    // 打开（当场生效）：缓存里没有，马上取。
    sender.send_replace(settings(true));
    let waited = tokio::time::timeout(Duration::from_secs(10), async {
        while data.catalog().is_none() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await;
    assert!(waited.is_ok(), "打开以后取了");
    assert_eq!(server.received().len(), 1);
    // 刚取过：等一天，不再取。
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(server.received().len(), 1);
    drop(sender);
    tokio::time::timeout(Duration::from_secs(60), running)
        .await
        .expect("配置没了就停")
        .expect("没有 panic");
}

/// 地址写成环境变量的引用（施工 8-8，8-6b 留下的：以前读成空的、一直拉不到）：照核心的环境取，后台照它取；没设、设成空的
/// 没有地址，到点了照失败算。
#[tokio::test]
async fn an_address_from_the_environment_is_fetched_from_where_it_points() {
    let mut values = Values::default();
    values.set(
        "models.catalog.url",
        Value::Secret(Reference::Env("CAT_URL".to_string())),
    );
    let settings = CatalogSettings::from(&values);
    assert_eq!(settings.url, Address::Env("CAT_URL".to_string()));
    let server = Server::start(vec![ok(&catalog("from-env"), "\"v1\"")]).await;
    let url = format!("{}/api.json", server.base_url);
    let env = |name: &str| (name == "CAT_URL").then(|| url.clone());
    let schedule = Schedule::of(settings.clone(), &env);
    assert_eq!(schedule.url.as_deref(), Some(url.as_str()));
    assert_eq!(Schedule::of(settings.clone(), &|_| None).url, None, "没设");
    assert_eq!(
        Schedule::of(settings, &|_| Some(String::new())).url,
        None,
        "设成空的"
    );
    let dir = Dir::new();
    let data = data();
    let (sender, receiver) = watch::channel(schedule);
    let running = tokio::spawn(refresher(&dir, &data).run(receiver));
    let waited = tokio::time::timeout(Duration::from_secs(10), async {
        while data.catalog().is_none() {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await;
    assert!(waited.is_ok(), "照环境变量里的地址取到了");
    assert_eq!(server.received().len(), 1);
    drop(sender);
    running.await.expect("没有 panic");
    // 没有地址的：到点了什么都取不到，照失败算，接着等配置变。
    let schedule = Schedule {
        update: true,
        url: None,
        every: Duration::from_secs(86_400),
    };
    let (sender, receiver) = watch::channel(schedule);
    let (other, nothing) = (Dir::new(), self::data());
    let running = tokio::spawn(refresher(&other, &nothing).run(receiver));
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(nothing.catalog().is_none());
    assert!(!running.is_finished(), "没有地址不停下");
    drop(sender);
    running.await.expect("没有 panic");
}

/// 配置界面取一次（LM-9，[`fetch_now`]）：在调的线程上取、写进缓存、交回读好的目录；再取一次回 304 的交回没有、只改时刻；
/// 取不到的交回原话、缓存不动。要不要取照 [`Schedule::due`]：关着、没地址的不取，缓存里没有、旧了的取，新的不取。
#[tokio::test]
async fn the_settings_fetch_once_on_their_own_thread() {
    let server = Server::start(vec![
        ok(&catalog("now"), "\"v1\""),
        Reply::error(304, &[], ""),
        Reply::error(500, &[], "down"),
    ])
    .await;
    let url = format!("{}/api.json", server.base_url);
    let dir = Dir::new();
    let cache = dir.at("cache");
    let schedule = Schedule {
        update: true,
        url: Some(url.clone()),
        every: Duration::from_secs(86_400),
    };
    let now = SystemTime::now();
    assert!(schedule.due(&cache, now), "缓存里没有：要取");
    assert!(
        !Schedule {
            update: false,
            ..schedule.clone()
        }
        .due(&cache, now)
    );
    assert!(
        !Schedule {
            url: None,
            ..schedule.clone()
        }
        .due(&cache, now)
    );
    // 配置界面在自己的线程上调（那里没有运行时）：测试的运行时照旧跑着假服务器。
    let fetch = |cache: PathBuf, url: String| {
        tokio::task::spawn_blocking(move || {
            std::thread::spawn(move || fetch_now(&cache, &url))
                .join()
                .expect("没有 panic")
        })
    };
    let loaded = fetch(cache.clone(), url.clone())
        .await
        .expect("没有 panic")
        .expect("取到了")
        .expect("新的");
    assert!(loaded.catalog.model("p", "now").is_some());
    assert!(!schedule.due(&cache, SystemTime::now()), "刚取过");
    assert!(
        schedule.due(&cache, SystemTime::now() + Duration::from_secs(2 * 86_400)),
        "旧了"
    );
    let again = fetch(cache.clone(), url.clone()).await.expect("没有 panic");
    assert!(again.expect("304").is_none());
    let failed = fetch(cache.clone(), url.clone()).await.expect("没有 panic");
    assert_eq!(failed.err(), Some(Unfetched::Status(500)), "照状态码认");
    assert!(
        load(&Places {
            cache: Some(cache.clone())
        })
        .is_some_and(|loaded| loaded.catalog.model("p", "now").is_some()),
        "缓存还在"
    );
}

/// 配置界面和引擎同时缺目录（LM-9补）：两边排同一处缓存的一把锁，只 GET 一次；后到的一方（这里是配置界面）不再取，照缓存里
/// 刚取到的那一份。
#[tokio::test]
async fn the_settings_and_the_engine_fetch_once_between_them() {
    let server = Server::start(vec![
        Reply {
            status: 200,
            headers: vec![("ETag".to_string(), "\"v1\"".to_string())],
            body: vec![
                Piece::HoldHead(Duration::from_millis(400)),
                Piece::Bytes(catalog("first").into_bytes()),
            ],
        },
        ok(&catalog("second"), "\"v2\""),
    ])
    .await;
    let url = format!("{}/api.json", server.base_url);
    let dir = Dir::new();
    let data = data();
    let refresher = refresher(&dir, &data);
    let cache = refresher.cache.clone();
    let engine = {
        let (refresher, url) = (refresher.clone(), url.clone());
        tokio::spawn(async move { refresher.once(&url).await })
    };
    // 引擎先排上、正等着回话时，配置界面也要取。
    tokio::time::sleep(Duration::from_millis(100)).await;
    let settings = tokio::task::spawn_blocking(move || {
        std::thread::spawn(move || fetch_now(&cache, &url))
            .join()
            .expect("没有 panic")
    });
    assert_eq!(engine.await.expect("没有 panic"), Refreshed::Replaced);
    let loaded = settings
        .await
        .expect("没有 panic")
        .expect("取到了")
        .expect("有目录");
    assert!(
        loaded.catalog.model("p", "first").is_some(),
        "照引擎刚取到的那一份"
    );
    assert_eq!(server.received().len(), 1, "只 GET 一次");
    assert!(
        data.catalog()
            .is_some_and(|loaded| loaded.catalog.model("p", "first").is_some())
    );
}

/// 纳秒时刻：临时目录名里加上它，Windows 很快复用进程号，光靠进程号和序号会撞上前一个测试进程留下的目录。
fn stamp() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos())
}
