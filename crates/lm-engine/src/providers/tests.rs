//! 不带界面的供应商入口：配了的几家、密钥还有没有人引用、删一个密钥只动那一行；从 Miyu 导入算的和写的（同名覆盖时原来的
//! 密钥没人用了删掉、值一样的项不丢）。Miyu 的数据根全是造出来的，不读真实的 `~/.miyu`。

use lm_kernel::config::Value;
use lm_kernel::config::secret::{Reference, Secret};

use super::import::{Clash, apply, auto, clashes, plan};
use super::miyu::{self, Key};
use super::{configured, forget_secret, secret_used};
use crate::settings::{Edit, Settings};
use crate::store::paths::Paths;
use crate::store::test_support::Scratch;

fn settings(scratch: &Scratch, config: &str) -> Settings {
    let paths = Paths::under(&scratch.path().join("lite"));
    std::fs::create_dir_all(&paths.config).expect("建得了配置那一处");
    std::fs::write(paths.config_file(), config).expect("写得进");
    Settings::new(paths, |_| None)
}

/// 造一份 Miyu 的数据根：布局 3 的标记、配置、密钥。
fn miyu_root(scratch: &Scratch, config: &str, secrets: &str) -> std::path::PathBuf {
    let root = scratch.path().join("miyu");
    std::fs::create_dir_all(root.join("etc/private")).expect("建得了");
    std::fs::write(root.join(".miyu-layout"), "3\n").expect("写得进");
    std::fs::write(root.join("etc/config.toml"), config).expect("写得进");
    std::fs::write(root.join("etc/private/secrets.toml"), secrets).expect("写得进");
    root
}

#[test]
fn configured_providers_and_secret_references_are_read_from_the_written_keys() {
    let scratch = Scratch::new();
    let settings = settings(
        &scratch,
        "[providers.b]\nbase_url = \"http://x\"\nkey = { secret = \"kb\" }\n[providers.a]\ncatalog = \"deepseek\"\n",
    );
    let written = settings.read().expect("读得了").written;
    assert_eq!(configured(&written), ["a", "b"]);
    assert!(secret_used(&written, "kb"));
    assert!(!secret_used(&written, "ka"));
}

#[test]
fn forgetting_a_secret_removes_only_that_line() {
    let scratch = Scratch::new();
    let settings = settings(&scratch, "");
    let one = Secret::new("sk-one").expect("合写法");
    let two = Secret::new("sk-two").expect("合写法");
    assert_eq!(settings.store_secret("a", &one).expect("存得进"), "a");
    assert_eq!(settings.store_secret("b", &two).expect("存得进"), "b");
    assert!(forget_secret(&settings, "a").expect("删得了"));
    assert!(!forget_secret(&settings, "a").expect("没有的照实说"));
    let names = settings.read().expect("读得了").secrets;
    assert_eq!(names.into_iter().collect::<Vec<_>>(), ["b"]);
}

#[test]
fn importing_plans_clashes_and_writes_keys_but_no_pool_nor_a_chat_pool() {
    let scratch = Scratch::new();
    let settings = settings(
        &scratch,
        "[providers.ds]\nbase_url = \"http://old\"\nkey = { secret = \"ds\" }\n",
    );
    settings
        .store_secret("ds", &Secret::new("sk-old").expect("合写法"))
        .expect("存得进");
    let root = miyu_root(
        &scratch,
        "[providers.ds]\nbase_url = \"http://new\"\nkey = { secret = \"ds\" }\n\
         [providers.kimi]\nbase_url = \"http://kimi\"\nkey = { env = \"KIMI_KEY\" }\n\
         [pools.fast]\nmodels = [\"kimi/k2\", \"other/x\"]\n[models]\nchat = \"@fast\"\n",
        "ds = \"sk-from-miyu\"\n",
    );
    let found = miyu::read(&root, settings.items(), &mut |_, _| false).expect("读得出");
    assert_eq!(found.providers.len(), 2);
    assert!(matches!(found.providers[0].key, Key::Found(_)));
    let written = settings.read().expect("读得了").written;
    let all: Vec<usize> = (0..found.providers.len()).collect();
    assert_eq!(clashes(&written, &found, &all), ["ds"]);

    let skip = plan(&written, &found, &all, Clash::Skip);
    assert_eq!(skip.import, ["kimi"]);
    assert_eq!(skip.passed, ["ds"]);
    assert_eq!(skip.chat, None, "Miyu 的主对话指到池的不设，让人自己挑");

    let over = plan(&written, &found, &all, Clash::Overwrite);
    assert_eq!(over.import, ["ds", "kimi"]);
    assert_eq!(over.overwrite, ["ds"]);
    apply(&settings, &written, &found, &over).expect("写得进");
    let read = settings.read().expect("读得了");
    let get = |key: &str| read.written.get(key).cloned();
    assert_eq!(
        get("providers.ds.base_url"),
        Some(Value::Text("http://new".to_string().into()))
    );
    assert_eq!(
        get("providers.ds.key"),
        Some(Value::Secret(Reference::Secret("ds-2".to_string()))),
        "新的 key 存成错开的名字"
    );
    assert!(
        !read.secrets.contains("ds"),
        "原来那个没人引用了，删掉：{:?}",
        read.secrets
    );
    assert_eq!(
        get("providers.kimi.key"),
        Some(Value::Secret(Reference::Env("KIMI_KEY".to_string())))
    );
    assert!(
        read.written.keys().all(|key| !key.starts_with("pools.")),
        "池一概不导：{:?}",
        read.written.keys().collect::<Vec<_>>()
    );
    assert_eq!(get("models.chat"), None, "指到池的主对话模型不设");
}

#[test]
fn a_chat_model_naming_an_imported_provider_comes_over_and_one_naming_another_does_not() {
    let scratch = Scratch::new();
    let settings = settings(&scratch, "");
    let root = miyu_root(
        &scratch,
        "[providers.kimi]\nbase_url = \"http://kimi\"\n\
         [pools.flagship]\nmodels = [\"kimi/k2\"]\n[models]\nchat = \"kimi/k2\"\n",
        "",
    );
    let found = miyu::read(&root, settings.items(), &mut |_, _| false).expect("读得出");
    let written = settings.read().expect("读得了").written;
    let chosen = plan(&written, &found, &[0], Clash::Skip);
    assert_eq!(chosen.chat.as_deref(), Some("kimi/k2"));
    apply(&settings, &written, &found, &chosen).expect("写得进");
    let read = settings.read().expect("读得了");
    assert_eq!(
        read.written.get("models.chat"),
        Some(&Value::Text("kimi/k2".to_string().into()))
    );
    assert!(read.written.keys().all(|key| !key.starts_with("pools.")));

    let other = miyu_root(
        &scratch,
        "[providers.kimi]\nbase_url = \"http://kimi\"\n[models]\nchat = \"gone/x\"\n",
        "",
    );
    let found = miyu::read(&other, settings.items(), &mut |_, _| false).expect("读得出");
    let nothing = std::collections::BTreeMap::new();
    assert_eq!(
        plan(&nothing, &found, &[0], Clash::Skip).chat,
        None,
        "指到没导的一家的不设"
    );
}

#[test]
fn overwriting_with_the_same_value_keeps_the_key() {
    let scratch = Scratch::new();
    let settings = settings(&scratch, "[providers.ds]\nbase_url = \"http://same\"\n");
    let root = miyu_root(&scratch, "[providers.ds]\nbase_url = \"http://same\"\n", "");
    let found = miyu::read(&root, settings.items(), &mut |_, _| false).expect("读得出");
    let written = settings.read().expect("读得了").written;
    let over = plan(&written, &found, &[0], Clash::Overwrite);
    apply(&settings, &written, &found, &over).expect("写得进");
    let read = settings.read().expect("读得了");
    assert_eq!(
        read.written.get("providers.ds.base_url"),
        Some(&Value::Text("http://same".to_string().into()))
    );
    // 别的编辑照样走配置服务。
    settings
        .set(&[Edit::Unset("providers.ds.base_url".to_string())])
        .expect("删得了");
}

#[test]
fn the_automatic_import_brings_everything_over_once_and_only_when_nothing_is_configured() {
    // 项目主人 2026-11-23 定：一家都没配、电脑上有 Miyu 的配置时不问，直接导；只导一次。
    let scratch = Scratch::new();
    let root = miyu_root(
        &scratch,
        "[providers.ds]\nbase_url = \"http://ds\"\nkey = { secret = \"ds\" }\n\
         [providers.kimi]\nbase_url = \"http://kimi\"\nkey = { env = \"KIMI_KEY\" }\n\
         [models]\nchat = \"ds/deep\"\n",
        "ds = \"sk-from-miyu\"\n",
    );
    let home = root.to_string_lossy().into_owned();
    let var = move |name: &str| (name == "MIYU_HOME").then(|| home.clone());

    // 已经配了一家的：不动。
    let busy = settings(&scratch, "[providers.mine]\nbase_url = \"http://mine\"\n");
    assert_eq!(auto(&busy, &var).expect("读得了"), None);
    assert_eq!(
        configured(&busy.read().expect("读得了").written),
        ["mine"],
        "已经配了的不自动导"
    );

    // 一家都没配的：认得出的全导进来，连 key 和主对话的模型。
    let empty = Scratch::new();
    let fresh = settings(&empty, "");
    let plan = auto(&fresh, &var).expect("写得进").expect("导了");
    assert_eq!(plan.import, ["ds", "kimi"]);
    assert_eq!(plan.chat.as_deref(), Some("ds/deep"));
    let read = fresh.read().expect("读得了");
    assert_eq!(configured(&read.written), ["ds", "kimi"]);
    assert!(
        read.secrets.contains("ds"),
        "key 存进了密钥文件：{:?}",
        read.secrets
    );
    assert_eq!(
        read.written.get("models.chat"),
        Some(&Value::Text("ds/deep".to_string().into()))
    );

    // 只导一次：人把导进来的都删了，下一次不再导回来。
    let gone: Vec<Edit> = read
        .written
        .keys()
        .filter(|key| key.starts_with("providers.") || key.as_str() == "models.chat")
        .map(|key| Edit::Unset(key.clone()))
        .collect();
    fresh.set(&gone).expect("删得了");
    assert!(configured(&fresh.read().expect("读得了").written).is_empty());
    assert_eq!(
        auto(&fresh, &var).expect("读得了"),
        None,
        "导过一次的不再导"
    );

    // 没有 Miyu 的数据根、Miyu 的配置里一家都没有：什么都不动，也不留记号。
    let none = Scratch::new();
    let alone = settings(&none, "");
    let nowhere = none.path().join("no-miyu").to_string_lossy().into_owned();
    let missing = move |name: &str| (name == "MIYU_HOME").then(|| nowhere.clone());
    assert_eq!(auto(&alone, &missing).expect("读得了"), None);
    assert!(
        !alone.read().expect("读得了").exists
            || configured(&alone.read().expect("读得了").written).is_empty()
    );
    let later = auto(&alone, &var).expect("写得进");
    assert!(
        later.is_some(),
        "以后装了 Miyu，照样导得了（上一次没导成不留记号）"
    );
}
