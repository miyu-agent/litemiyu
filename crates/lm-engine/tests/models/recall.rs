//! 目录不常驻，起来时（施工 V-25 下，从 Miyu `miyu-core/tests/catalog_recall.rs` 搬来）：头一回读整份目录、把配置里写过的
//! 记下写进 `catalog.json`；下一回版本对上了不读整份目录（把原文换成同样大小、同样改动时间的乱码：真读了就读不出来，答的却
//! 照旧）；记下的那份坏了、写法的版本不对的，当没有、整份重算；后台拉到新目录，记下的那份照新目录换版本、写盘。
//!
//! 和 Miyu 不同：程序不带快照（LM-9），这里的目录是缓存的那一份；缓存里还没有目录的起来是空的，以后取到了照配置记下、换成
//! 不常驻的那一份；起来时不连网。认原厂的表编在程序里，「表换了」那一段写不出来，没搬；`model.list` 记下的那份没搬，跟着它的
//! 那一个没搬。

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use serde_json::json;

use lm_engine::http::testkit::{Piece, Reply, Server};
use lm_engine::http::{Proxy, fetcher};
use lm_engine::models::begin;
use lm_engine::models::catalog::{FILE, META, Meta, Places};
use lm_engine::models::recall::Version;
use lm_engine::models::refresh::{Refreshed, Refresher};
use lm_engine::session::{ModelData, ModelDirs, RECALL_FILE};
use lm_kernel::config::Layer;
use lm_kernel::config::merge::{Layers, Resolved, merge};
use lm_kernel::config::parse::parse;
use lm_kernel::models::catalog::Recalled;
use lm_kernel::models::facts::facts;
use lm_kernel::models::matching::Vendors;
use lm_kernel::models::profile::Profiles;
use lm_kernel::models::provider::provider;
use lm_kernel::models::settings::{ModelSettings, ProviderSettings, UseSettings};

/// 出厂的认原厂的表（版本串照它算）。
const VENDORS: &str = include_str!("../../../../resources/models/vendors.toml");

/// 用完就删的临时目录：缓存（里面有一份目录）、`state/models`。
struct Dir(PathBuf);

impl Dir {
    fn new() -> Dir {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let stamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos());
        let dir = std::env::temp_dir().join(format!(
            "litemiyu-recall-{}-{stamp}-{n}",
            std::process::id()
        ));
        for sub in ["cache", "state"] {
            std::fs::create_dir_all(dir.join(sub)).expect("建得了");
        }
        let meta = Meta {
            source: "https://models.dev/api.json".to_string(),
            fetched: "2099-01-01T00:00:00.000Z".to_string(),
            etag: None,
        };
        std::fs::write(dir.join("cache").join(META), meta.to_json()).expect("写得进");
        std::fs::write(dir.join("cache").join(FILE), catalog(1000)).expect("写得进");
        Dir(dir)
    }

    fn at(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for Dir {
    #[expect(clippy::let_underscore_must_use, reason = "删不掉就留在临时目录里")]
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// 只有一家 `p`（带地址）、一个模型 `m` 的目录，窗口是 `window`。
fn catalog(window: u64) -> String {
    json!({"p": {"id": "p", "api": "https://p.invalid/v1", "models": {"m": {"limit": {"context": window}}}}})
        .to_string()
}

const CONFIG: &str = "[providers.p]\nlocal = false\n\n[models]\nchat = \"p/m\"\n";

fn resolved() -> Resolved {
    resolved_from(CONFIG)
}

fn resolved_from(config: &str) -> Resolved {
    let items = [
        ProviderSettings::ITEMS,
        ModelSettings::ITEMS,
        UseSettings::ITEMS,
    ]
    .concat();
    let parsed = parse(&items, Layer::System, config).expect("写法对");
    assert!(parsed.problems.is_empty(), "{:?}", parsed.problems);
    let layers = Layers {
        system: Some(&parsed),
        ..Layers::default()
    };
    merge(&items, &layers, &|_| None)
}

/// 照 `dir` 起一回：读目录、读记下的那份，等读完。
async fn start(dir: &Dir) -> Arc<ModelData> {
    start_with(dir, resolved()).await
}

/// 同 [`start`]，配置照 `config`。
async fn start_with(dir: &Dir, config: Resolved) -> Arc<ModelData> {
    let dirs = || ModelDirs {
        state: dir.at("state"),
        cache: dir.at("state"),
    };
    let data = Arc::new(ModelData::new(
        Profiles::default(),
        Vendors::default(),
        Some(dirs()),
    ));
    let places = Places {
        cache: Some(dir.at("cache")),
    };
    drop(begin(Arc::clone(&data), places, Some(dirs()), Some(config)));
    tokio::time::timeout(Duration::from_secs(10), data.wait())
        .await
        .expect("读完了");
    data
}

/// `p/m` 的窗口：照记下的、或者整份目录。
fn window(data: &ModelData) -> Option<u64> {
    let resolved = resolved();
    let values = resolved.values();
    data.with(|knowledge| {
        let found = provider(&values, knowledge, "p").ok()?;
        facts(&resolved, knowledge, &found, "m").0.window.value
    })
}

fn recalled(dir: &Dir) -> Recalled {
    let text = std::fs::read_to_string(dir.at("state").join(RECALL_FILE)).expect("写了");
    Recalled::from_json(&text).expect("读得回来")
}

/// 把原文换成同样大小的乱码，改动时间照旧：指纹不变，真读就读不出来。
fn scramble(file: &Path) {
    let modified = std::fs::metadata(file)
        .and_then(|meta| meta.modified())
        .expect("有改动时间");
    let len = std::fs::metadata(file).expect("在").len();
    let garbage = "x".repeat(usize::try_from(len).expect("不大"));
    std::fs::write(file, garbage).expect("写得进");
    std::fs::File::options()
        .write(true)
        .open(file)
        .and_then(|opened| opened.set_modified(modified))
        .expect("改得回改动时间");
}

#[tokio::test]
async fn a_recall_that_matches_skips_reading_the_catalog() {
    let dir = Dir::new();
    let first = start(&dir).await;
    assert_eq!(window(&first), Some(1000));
    let saved = recalled(&dir);
    assert_eq!(saved.len(), (1, 1), "配置里写过的一家、一个模型");
    drop(first);
    scramble(&dir.at("cache").join(FILE));
    let second = start(&dir).await;
    assert_eq!(window(&second), Some(1000), "照记下的答，没读整份目录");
    assert_eq!(recalled(&dir).of(), saved.of());
    // 乱了的缓存读不出来（程序不带快照，没有另一份可退）：上面的窗口只能是照记下的答的。
    assert!(second.catalog().is_none(), "乱了的缓存读不出来");
}

/// 版本对上了的那一份起来时不再序列化、写一遍（核心 四）：盘上就是它。盘上那份末尾多一个换行（读得回、答一样，只是和
/// 序列化出来的字节不一样）：照旧写一遍的话会被盖回没有换行的那份。
#[tokio::test]
async fn a_recall_that_matches_is_not_written_again() {
    let dir = Dir::new();
    drop(start(&dir).await);
    let file = dir.at("state").join(RECALL_FILE);
    let text = format!("{}\n", std::fs::read_to_string(&file).expect("写了"));
    std::fs::write(&file, &text).expect("写得进");
    let second = start(&dir).await;
    assert_eq!(window(&second), Some(1000), "照记下的答");
    assert_eq!(
        std::fs::read_to_string(&file).expect("在"),
        text,
        "没有再写一遍"
    );
}

/// 配置改过以后起来（核心 四）：记下的那份版本对得上，配置里新写的模型却没记过——起来时就在读目录的那条线程里补上、写盘，
/// 不等第一句话在 tokio 的线程上读整份目录。照盘上的那份看：没问过一句，新写的已经记下了。
#[tokio::test]
async fn a_model_written_into_the_config_since_is_remembered_while_starting() {
    let dir = Dir::new();
    drop(start(&dir).await);
    assert_eq!(recalled(&dir).len(), (1, 1));
    let changed = resolved_from(
        "[providers.p]\nlocal = false\n\n[providers.p.models.\"m2\"]\nwindow = 500\n\n[models]\nchat = \"p/m\"\n",
    );
    let data = start_with(&dir, changed).await;
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while recalled(&dir).len() != (1, 2) {
        assert!(
            std::time::Instant::now() < deadline,
            "起来以后补记了新写的模型"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert_eq!(window(&data), Some(1000));
}

#[tokio::test]
async fn a_broken_or_old_recall_is_rebuilt_from_the_catalog() {
    let dir = Dir::new();
    drop(start(&dir).await);
    let good = recalled(&dir);
    let file = dir.at("state").join(RECALL_FILE);
    let text = good.to_json().expect("写得成");
    let older = text.replacen("{\"v\":1,", "{\"v\":0,", 1);
    assert_ne!(older, text);
    for broken in ["{", "not json at all", older.as_str()] {
        std::fs::write(&file, broken).expect("写得进");
        let data = start(&dir).await;
        assert_eq!(window(&data), Some(1000), "当没有，整份重算");
        assert_eq!(recalled(&dir), good, "重算完写回了好的一份");
    }
    // 目录换了一份（同样的 `fetched`，原文变了）：也对不上，照新的记。
    std::fs::write(dir.at("cache").join(FILE), catalog(2000)).expect("写得进");
    let data = start(&dir).await;
    assert_eq!(window(&data), Some(2000));
}

#[tokio::test]
async fn a_refresh_moves_the_recall_to_the_new_catalog() {
    let dir = Dir::new();
    let data = start(&dir).await;
    let before = recalled(&dir);
    let server = Server::start(vec![Reply {
        status: 200,
        headers: vec![("ETag".to_string(), "\"v1\"".to_string())],
        body: vec![Piece::Bytes(catalog(3000).into_bytes())],
    }])
    .await;
    let refresher = Refresher {
        data: Arc::clone(&data),
        client: Some(fetcher(Proxy::Off).expect("造得出客户端")),
        cache: dir.at("cache"),
        version: Some(Version::of_vendors(VENDORS)),
        config: None,
    };
    let url = format!("{}/api.json", server.base_url);
    assert_eq!(refresher.once(&url).await, Refreshed::Replaced);
    assert_eq!(window(&data), Some(3000), "记下的照新目录重算了");
    let after = recalled(&dir);
    assert_ne!(after.of(), before.of());
    assert_eq!(after.source().as_str(), "cache");
    // 下一回起来：新目录排第一，版本对上，不读整份目录。
    drop(data);
    let again = start(&dir).await;
    assert_eq!(window(&again), Some(3000));
    assert_eq!(recalled(&dir).of(), after.of());
}

/// 缓存里还没有目录（LM-9：程序不带目录）：起来时不连网，目录是空的，窗口不知道；后台取到了（[`Refresher`] 带着这一刻的配置）
/// 照配置把写过的记下、写盘，换成不常驻的那一份（整份目录不留在内存里）；下一回起来照记下的答，不读整份目录。
#[tokio::test]
async fn a_catalog_fetched_later_is_remembered_and_not_kept_whole() {
    let dir = Dir::new();
    std::fs::remove_file(dir.at("cache").join(FILE)).expect("删得掉");
    std::fs::remove_file(dir.at("cache").join(META)).expect("删得掉");
    let server = Server::start(vec![Reply {
        status: 200,
        headers: vec![("ETag".to_string(), "\"v1\"".to_string())],
        body: vec![Piece::Bytes(catalog(4000).into_bytes())],
    }])
    .await;
    let data = start(&dir).await;
    assert_eq!(window(&data), None, "没有目录：窗口不知道");
    assert!(data.catalog().is_none());
    assert!(server.received().is_empty(), "起来时不连网");
    assert!(
        !dir.at("state").join(RECALL_FILE).exists(),
        "没有目录时不记"
    );
    let refresher = Refresher {
        data: Arc::clone(&data),
        client: Some(fetcher(Proxy::Off).expect("造得出客户端")),
        cache: dir.at("cache"),
        version: Some(Version::of_vendors(VENDORS)),
        config: Some(Arc::new(|| Some(resolved()))),
    };
    let url = format!("{}/api.json", server.base_url);
    assert_eq!(refresher.once(&url).await, Refreshed::Replaced);
    let saved = recalled(&dir);
    assert_eq!(saved.len(), (1, 1), "取到时照配置记下了写过的");
    assert_eq!(data.recalled_of().as_deref(), Some(saved.of()), "不常驻");
    assert_eq!(window(&data), Some(4000), "取到了：窗口补上");
    drop(data);
    scramble(&dir.at("cache").join(FILE));
    let again = start(&dir).await;
    assert_eq!(window(&again), Some(4000), "下一回照记下的答");
}

/// 以前的版本照编进程序的快照记下的那份（`source` 是 `snapshot`）：认得出、版本对不上，照缓存里的目录重算、写回。
#[tokio::test]
async fn a_recall_made_from_the_old_snapshot_is_rebuilt() {
    let dir = Dir::new();
    let old = Recalled::new(
        "snapshot 0.0.0 225872 2026-10-01T03:25:54.000Z vendors=0",
        lm_kernel::models::catalog::CatalogSource::Snapshot,
        "2026-10-01T03:25:54.000Z",
    );
    std::fs::write(
        dir.at("state").join(RECALL_FILE),
        old.to_json().expect("写得成"),
    )
    .expect("写得进");
    let data = start(&dir).await;
    assert_eq!(window(&data), Some(1000));
    let rebuilt = recalled(&dir);
    assert_eq!(rebuilt.source().as_str(), "cache");
    assert_eq!(rebuilt.len(), (1, 1));
}

/// 配置界面取到了目录、写进同一处缓存（LM-9）：核心照 [`lm_engine::models::reload`] 读回来、照配置记下、换上；缓存没再变的
/// 再叫一次不动（记下的那份照旧）；缓存里没有的不动。
#[tokio::test]
async fn a_catalog_written_by_the_settings_is_picked_up() {
    let dir = Dir::new();
    std::fs::remove_file(dir.at("cache").join(FILE)).expect("删得掉");
    std::fs::remove_file(dir.at("cache").join(META)).expect("删得掉");
    let data = start(&dir).await;
    lm_engine::models::reload(Arc::clone(&data), dir.at("cache"), Some(resolved()))
        .await
        .expect("没有 panic");
    assert_eq!(window(&data), None, "缓存里没有：不动");
    std::fs::write(dir.at("cache").join(FILE), catalog(5000)).expect("写得进");
    let meta = Meta {
        source: "https://models.dev/api.json".to_string(),
        fetched: "2099-01-02T00:00:00.000Z".to_string(),
        etag: None,
    };
    std::fs::write(dir.at("cache").join(META), meta.to_json()).expect("写得进");
    lm_engine::models::reload(Arc::clone(&data), dir.at("cache"), Some(resolved()))
        .await
        .expect("没有 panic");
    assert_eq!(window(&data), Some(5000), "换上了");
    let saved = recalled(&dir);
    assert_eq!(saved.len(), (1, 1));
    lm_engine::models::reload(Arc::clone(&data), dir.at("cache"), Some(resolved()))
        .await
        .expect("没有 panic");
    assert_eq!(recalled(&dir).of(), saved.of(), "没变的不动");
}
