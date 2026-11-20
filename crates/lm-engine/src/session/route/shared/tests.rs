//! 用出来的和拉到的分两处写、读回来照样两处读（施工 FH-2）。Miyu 这里还测后台拉列表的间隔、列表变了才通知（施工 V-26）：
//! 拉列表这一层 LiteMiyu 没搬（配置界面那一步再搬），那两个跟着没搬。

use lm_kernel::kernel::time::Timestamp;
use lm_kernel::models::matching::Vendors;
use lm_kernel::models::observed::{ListedModel, ProviderList};
use lm_kernel::models::profile::Profiles;

use super::ModelData;

/// 列着 `ids` 的一份，拉的时刻是 `at` 毫秒。
fn list(ids: &[&str], at: i64) -> ProviderList {
    ProviderList {
        fetched: Timestamp::from_unix_millis(at).expect("合写法"),
        models: ids
            .iter()
            .map(|id| ListedModel {
                id: (*id).to_string(),
                window: None,
                ..ListedModel::default()
            })
            .collect(),
    }
}

/// 用出来的和拉到的分两处写（施工 FH-2，设计 55）：学到的窗口在 `state`，供应商的列表在 `cache`；读回来照样两处读（LM-8 砍了
/// 模型池，原来也在 `state` 的池的指针 `pools.json` 不写了）。
#[test]
fn what_is_learned_and_what_is_fetched_are_kept_apart() {
    let scratch = std::env::temp_dir().join(format!("litemiyu-model-dirs-{}", std::process::id()));
    std::fs::create_dir_all(&scratch).expect("建得了");
    let dirs = super::ModelDirs {
        state: scratch.join("learned"),
        cache: scratch.join("fetched"),
    };
    let data = ModelData::new(Profiles::default(), Vendors::default(), Some(dirs.clone()));
    let at = Timestamp::from_unix_millis(1).expect("合写法");
    data.learn("dev", "m", 4096, at);
    // 写供应商的列表的是拉列表那一层（没搬）：照它的写法直接写进拉到的那一处。
    let providers = dirs.cache.join("providers");
    std::fs::create_dir_all(&providers).expect("建得了");
    std::fs::write(providers.join("dev.json"), list(&["m"], 1).to_json()).expect("写得进");
    assert!(dirs.state.join("learned.json").is_file());
    assert!(!dirs.state.join("pools.json").exists(), "池的指针不写了");
    assert!(
        !dirs.state.join("providers").exists(),
        "列表不在用出来的那一处"
    );
    assert!(
        !dirs.cache.join("learned.json").exists(),
        "学到的不在拉到的那一处"
    );
    let read = super::read_observed(&dirs);
    assert_eq!(read.lists.len(), 1);
    assert!(read.learned.window("dev", "m").is_some());
    drop(std::fs::remove_dir_all(&scratch));
}
