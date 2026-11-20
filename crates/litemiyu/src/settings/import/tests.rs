//! 从 Miyu 导入，往坏里测：全用造出来的假 Miyu 数据根（临时目录），绝不碰真实的家目录。每一种坏情况状态行是一句看得懂的
//! 话，不崩；Miyu 的文件一个字节不动、不多一个文件；LiteMiyu 写不进时不留半截文件；假 key 不出现在任何给人看的字、日志里。

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use lm_engine::store::paths::Paths;

use crate::settings::{Outcome, Popup, Settings, Target, Tone, Window};

/// 一眼看得出是假的 key。
const FAKE: &str = "sk-FAKE-miyu-key-0000-not-real";

/// 临时目录：LiteMiyu 的数据在 `lite/`，假 Miyu 的数据根在 `miyu/`。用完删掉（先把改过的权限改回来）。
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir =
            std::env::temp_dir().join(format!("litemiyu-import-{}-{name}", std::process::id()));
        if dir.exists() {
            restore(&dir);
            std::fs::remove_dir_all(&dir).unwrap();
        }
        std::fs::create_dir_all(Paths::under(&dir.join("lite")).config).unwrap();
        std::fs::create_dir_all(dir.join("miyu/etc/private")).unwrap();
        std::fs::write(dir.join("miyu/.miyu-layout"), "layout 3\n").unwrap();
        Scratch(dir)
    }

    fn paths(&self) -> Paths {
        Paths::under(&self.0.join("lite"))
    }

    fn root(&self) -> PathBuf {
        self.0.join("miyu")
    }

    /// 写 Miyu 的配置。
    fn config(&self, text: &str) -> &Scratch {
        std::fs::write(self.root().join("etc/config.toml"), text).unwrap();
        self
    }

    /// 写 Miyu 的密钥（0600，同 Miyu）。
    fn secrets(&self, text: &str) -> &Scratch {
        let path = self.root().join("etc/private/secrets.toml");
        std::fs::write(&path, text).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        self
    }

    /// 开配置界面，Miyu 指到假的数据根，在加一家的目录窗里选「从 Miyu 导入…」。
    fn import(&self) -> Settings {
        self.import_in("zh")
    }

    /// 同 [`Scratch::import`]，界面是 `language`。
    fn import_in(&self, language: &str) -> Settings {
        let mut settings = Settings::at(self.paths(), language, Outcome::Back).unwrap();
        settings.miyu = Some(self.root());
        settings.show_models();
        // 供应商分页按 `a` 弹目录，「从 Miyu 导入…」在最后一行。
        press(&mut settings, KeyCode::Char('a'));
        for _ in 0..80 {
            press(&mut settings, KeyCode::Char('j'));
        }
        press(&mut settings, KeyCode::Enter);
        settings
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        restore(&self.0);
        if std::fs::remove_dir_all(&self.0).is_err() {}
    }
}

/// 把改过的权限改回来，删得掉。
fn restore(dir: &Path) {
    for sub in ["miyu/etc/private/secrets.toml", "miyu/etc/config.toml"] {
        // 没有这个文件的（这个测试没造）不用改。
        if std::fs::set_permissions(dir.join(sub), std::fs::Permissions::from_mode(0o600)).is_err()
        {
        }
    }
    let config = Paths::under(&dir.join("lite")).config;
    if std::fs::set_permissions(config, std::fs::Permissions::from_mode(0o700)).is_err() {}
}

fn press(settings: &mut Settings, code: KeyCode) {
    settings.key(KeyEvent::new(code, KeyModifiers::NONE));
    // 写盘在写线程里（毛病二）：按完等排着的写完，下面照盘上的、写完以后的状态行查。
    settings.flush_writes();
}

/// 状态行。
fn status(settings: &Settings) -> (String, Tone) {
    settings.status.clone().expect("状态行有一句")
}

/// 勾选窗里的几行。
fn ticked(settings: &Settings) -> Vec<String> {
    match &settings.popup {
        Some(Popup {
            window: Window::Ticks(ticks),
            target: Target::Import,
        }) => ticks.names.clone(),
        other => panic!("勾选窗没开：{other:?}，状态行 {:?}", settings.status),
    }
}

/// LiteMiyu 的配置、密钥。
fn lite(scratch: &Scratch) -> (String, String) {
    let paths = scratch.paths();
    (
        std::fs::read_to_string(paths.config_file()).unwrap_or_default(),
        std::fs::read_to_string(paths.secrets_file()).unwrap_or_default(),
    )
}

/// 一棵目录里每个文件的路径和字节。
fn tree(dir: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut found = Vec::new();
    let mut dirs = vec![dir.to_path_buf()];
    while let Some(at) = dirs.pop() {
        for entry in std::fs::read_dir(&at).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                dirs.push(path);
            } else {
                found.push((path.clone(), std::fs::read(&path).unwrap_or_default()));
            }
        }
    }
    found.sort();
    found
}

/// 给人看过的字（状态行、窗里的每一行）都不带假 key。
fn no_fake(settings: &Settings) {
    let mut shown = format!("{:?}{:?}", settings.status, settings.popup);
    shown.push_str(&format!("{:?}", settings.importing));
    assert!(!shown.contains(FAKE), "假 key 出现在给人看的字里：{shown}");
}

const TWO: &str = "[providers.relay]\nname = \"中转\"\ndriver = \"openai-chat\"\nbase_url = \"http://127.0.0.1:9/v1\"\nkey = { secret = \"relay\" }\nprice_multiplier = 2.0\n\n[providers.relay.models.\"m-1\"]\nwindow = 64000\n\n[providers.local]\ndriver = \"openai-chat\"\nbase_url = \"http://127.0.0.1:8/v1\"\nkey = { env = \"LOCAL_KEY\" }\n\n[pools.daily]\nmodels = [\"relay/m-1\", \"gone/x\"]\nstrategy = \"ordered\"\n\n[models]\nchat = \"@daily\"\n\n[tui]\ntheme = \"whatever\"\n";

#[test]
fn two_providers_and_their_keys_come_over_but_no_pool_nor_a_pool_chat_and_miyu_is_untouched() {
    let scratch = Scratch::new("two");
    scratch
        .config(TWO)
        .secrets(&format!("relay = \"{FAKE}\"\n"));
    let before = tree(&scratch.root());
    let mut settings = scratch.import();
    let names = ticked(&settings);
    assert_eq!(names.len(), 2, "{names:?}");
    assert!(
        names[0].starts_with("local (local) · http://127.0.0.1:8/v1 · "),
        "{names:?}"
    );
    assert!(
        names
            .iter()
            .any(|n| n.contains("中转 (relay)") && n.contains("已找到密钥")),
        "{names:?}"
    );
    assert!(
        names
            .iter()
            .any(|n| n.contains("密钥来自环境变量 LOCAL_KEY")),
        "{names:?}"
    );
    assert_eq!(settings.status, None, "都认得出、没写坏：状态行不说话");
    no_fake(&settings);
    press(&mut settings, KeyCode::Enter);
    let (said, tone) = status(&settings);
    assert_eq!(tone, Tone::Good, "{said}");
    assert!(said.contains("导入 2 个供应商"), "{said}");
    no_fake(&settings);
    let (config, secrets) = lite(&scratch);
    assert!(config.contains("key = { secret = \"relay\" }"), "{config}");
    assert!(config.contains("key = { env = \"LOCAL_KEY\" }"), "{config}");
    assert!(
        config.contains("window = 64000"),
        "模型的资料一起抄：{config}"
    );
    assert!(
        !config.contains("[pools") && !config.contains("daily"),
        "池一概不导：{config}"
    );
    assert!(
        !config.contains("chat ="),
        "Miyu 的主对话指到池的不设：{config}"
    );
    assert!(
        !config.contains(FAKE) && !config.contains("price_multiplier") && !config.contains("theme")
    );
    assert!(secrets.contains(FAKE), "key 进了 LiteMiyu 自己的密钥文件");
    let journal = std::fs::read_to_string(scratch.paths().journal()).unwrap_or_default();
    assert!(!journal.contains(FAKE), "改动的记录里没有明文：{journal}");
    assert_eq!(
        tree(&scratch.root()),
        before,
        "Miyu 的文件一个字节没动、没多一个文件"
    );
}

#[test]
fn the_same_name_asks_and_overwrite_or_skip_does_what_it_says() {
    let scratch = Scratch::new("clash");
    scratch
        .config(TWO)
        .secrets(&format!("relay = \"{FAKE}\"\n"));
    let own = "[providers.relay]\ndriver = \"anthropic\"\nbase_url = \"http://127.0.0.1:7\"\ncatalog = \"contract\"\n";
    std::fs::write(scratch.paths().config_file(), own).unwrap();
    let mut settings = scratch.import();
    assert!(
        ticked(&settings)
            .iter()
            .any(|n| n.contains("relay") && n.contains("已有同名供应商"))
    );
    press(&mut settings, KeyCode::Enter);
    match &settings.popup {
        Some(Popup {
            window: Window::Choose(choose),
            target: Target::ImportClash(_),
        }) => {
            assert!(
                choose.title.contains("LiteMiyu 中已有 relay"),
                "{}",
                choose.title
            );
            assert_eq!(choose.names, ["覆盖", "跳过已有的", "取消"]);
        }
        other => panic!("没问同名的：{other:?}"),
    }
    // 跳过：relay 原样，local 导进来。
    press(&mut settings, KeyCode::Down);
    press(&mut settings, KeyCode::Enter);
    let (said, _) = status(&settings);
    assert!(
        said.contains("导入 1 个供应商：local") && said.contains("已跳过已有的 relay"),
        "{said}"
    );
    let (config, secrets) = lite(&scratch);
    assert!(config.starts_with(own), "跳过的一个字节没动：{config}");
    assert!(!secrets.contains(FAKE), "跳过的那一家的 key 不存");
    // 再导一次，覆盖：原来的 anthropic、catalog 没了，换成 Miyu 的（LM-8 以前这里用的是 `cache`，砍了模型池以后它是退役的键）。
    let mut settings = scratch.import();
    press(&mut settings, KeyCode::Enter);
    press(&mut settings, KeyCode::Enter);
    let (said, tone) = status(&settings);
    assert_eq!(tone, Tone::Good, "{said}");
    let (config, _) = lite(&scratch);
    assert!(
        !config.contains("anthropic") && !config.contains("contract"),
        "{config}"
    );
    assert!(
        config.contains("base_url = \"http://127.0.0.1:9/v1\""),
        "{config}"
    );
    // 取消：什么都不写。
    let mut settings = scratch.import();
    let before = lite(&scratch);
    press(&mut settings, KeyCode::Enter);
    press(&mut settings, KeyCode::Down);
    press(&mut settings, KeyCode::Down);
    press(&mut settings, KeyCode::Enter);
    assert_eq!(status(&settings).0, "已取消导入");
    assert_eq!(lite(&scratch), before);
}

/// 覆盖导入时，LiteMiyu 原来写着、Miyu 那边值一样的项留着（以前界面先删再写，值没变的那一项被当成没改丢掉）。
#[test]
fn overwriting_keeps_the_items_whose_value_did_not_change() {
    let scratch = Scratch::new("same");
    scratch
        .config(TWO)
        .secrets(&format!("relay = \"{FAKE}\"\n"));
    let own = "[providers.relay]\nname = \"中转\"\ndriver = \"anthropic\"\nbase_url = \"http://127.0.0.1:9/v1\"\ncatalog = \"contract\"\n";
    std::fs::write(scratch.paths().config_file(), own).unwrap();
    let mut settings = scratch.import();
    press(&mut settings, KeyCode::Enter);
    assert!(matches!(
        settings.popup,
        Some(Popup {
            target: Target::ImportClash(_),
            ..
        })
    ));
    // 覆盖。
    press(&mut settings, KeyCode::Enter);
    let (said, tone) = status(&settings);
    assert_eq!(tone, Tone::Good, "{said}");
    let (config, _) = lite(&scratch);
    assert!(
        config.contains("name = \"中转\""),
        "名字没变，留着：{config}"
    );
    assert!(
        config.contains("base_url = \"http://127.0.0.1:9/v1\""),
        "地址没变，留着：{config}"
    );
    assert!(
        config.contains("driver = \"openai-chat\"") && !config.contains("contract"),
        "变了的换成 Miyu 的、Miyu 没有的删掉：{config}"
    );
    // 界面读回的也一样。
    let reread = Settings::at(scratch.paths(), "zh", Outcome::Back).unwrap();
    assert_eq!(reread.provider_name("relay"), "中转");
    no_fake(&settings);
}

#[test]
fn a_missing_empty_or_old_config_says_so() {
    let scratch = Scratch::new("missing");
    let settings = scratch.import();
    let (said, tone) = status(&settings);
    assert_eq!(tone, Tone::Bad);
    assert!(
        said.starts_with("Miyu 的数据目录中没有配置文件："),
        "{said}"
    );
    assert!(said.ends_with("etc/config.toml"), "{said}");

    scratch.config("");
    assert_eq!(status(&scratch.import()).0, "Miyu 的配置中没有供应商");
    scratch.config("# 只有注释\n[tui]\ntheme = \"x\"\n");
    assert_eq!(status(&scratch.import()).0, "Miyu 的配置中没有供应商");

    std::fs::remove_file(scratch.root().join(".miyu-layout")).unwrap();
    std::fs::write(scratch.root().join(".miyu-root"), "").unwrap();
    assert!(status(&scratch.import()).0.contains("旧版 Miyu"));
    assert_eq!(lite(&scratch), (String::new(), String::new()), "什么都没写");
}

#[test]
fn a_half_written_config_lists_what_comes_before_the_break() {
    let scratch = Scratch::new("half");
    scratch.config("[providers.a]\nbase_url = \"http://127.0.0.1:9/v1\"\n\n[providers.b]\nbase_url = \"http://127.0.0.1:8/v1\n");
    let settings = scratch.import();
    let names = ticked(&settings);
    assert_eq!(names.len(), 1, "{names:?}");
    assert!(names[0].starts_with("a (a)"), "{names:?}");
    let (said, _) = status(&settings);
    assert!(
        said.contains("第 5 行") && said.contains("只列出这之前的供应商"),
        "{said}"
    );

    scratch.config("[providers.a]\nbase_url = \"http://127.0");
    let (said, tone) = status(&scratch.import());
    assert_eq!(tone, Tone::Bad);
    assert!(
        said.contains("第 2 行") && said.contains("这之前没有供应商"),
        "{said}"
    );
}

#[test]
fn providers_that_cannot_be_used_are_listed_with_why() {
    let scratch = Scratch::new("why");
    crate::settings::put_catalog(&scratch.paths());
    scratch.config("[providers.nourl]\ndriver = \"openai-chat\"\n\n[providers.deepseek]\nkey = { env = \"DS\" }\n\n[providers.\"Bad Name\"]\nbase_url = \"http://127.0.0.1:9\"\n\n[providers.typo]\nbase_url = \"http://127.0.0.1:9\"\nlocal = \"yes\"\nname = 3\n");
    let settings = scratch.import();
    let names = ticked(&settings);
    assert!(
        names
            .iter()
            .any(|n| n.starts_with("deepseek (deepseek) · 目录中的地址")),
        "认得出的照目录：{names:?}"
    );
    assert!(
        names
            .iter()
            .any(|n| n.contains("typo") && n.contains("2 项无效，不导入")),
        "{names:?}"
    );
    assert_eq!(names.len(), 2, "{names:?}");
    let (said, _) = status(&settings);
    assert!(said.contains("nourl（未设置地址且不在目录中）"), "{said}");
    assert!(said.contains("Bad Name（ID 格式无效）"), "{said}");

    scratch.config("[providers.nourl]\ndriver = \"openai-chat\"\n");
    let (said, tone) = status(&scratch.import());
    assert_eq!(tone, Tone::Bad);
    assert_eq!(
        said,
        "Miyu 的配置中没有可导入的供应商：nourl（未设置地址且不在目录中）"
    );
}

/// 英文界面认不出的几家（LM-9补）：原因用半角括号接在编号后面，逗号前不多一格（原来照中文的全角括号拼，英文里看着像
/// 「Bad Name (invalid ID) , nourl」）。
#[test]
fn english_lists_what_cannot_be_used_with_plain_brackets() {
    let scratch = Scratch::new("why-en");
    crate::settings::put_catalog(&scratch.paths());
    scratch.config("[providers.nourl]\ndriver = \"openai-chat\"\n\n[providers.deepseek]\nkey = { env = \"DS\" }\n\n[providers.\"Bad Name\"]\nbase_url = \"http://127.0.0.1:9\"\n");
    let (said, _) = status(&scratch.import_in("en"));
    assert!(said.contains("Bad Name (invalid ID)"), "{said}");
    assert!(
        said.contains("nourl (no URL, not in the catalog)"),
        "{said}"
    );
    assert!(!said.contains('（') && !said.contains(" ,"), "{said}");
}

/// 目录还没取到（LM-9：程序不带目录）：没写地址的一家，出厂档案里写了地址的（DeepSeek）照样认、能导；档案里也没有的照旧说
/// 「未设置地址且不在目录中」。
#[test]
fn without_a_catalog_providers_known_to_the_profiles_still_come_over() {
    let scratch = Scratch::new("no-catalog");
    scratch.config(
        "[providers.deepseek]
key = { env = \"DS\" }

[providers.zhipuai]
key = { env = \"ZP\" }
",
    );
    let settings = scratch.import();
    let names = ticked(&settings);
    assert_eq!(names.len(), 1, "{names:?}");
    assert!(names[0].starts_with("deepseek (deepseek)"), "{names:?}");
    let (said, _) = status(&settings);
    assert!(said.contains("zhipuai（未设置地址且不在目录中）"), "{said}");
}

#[test]
fn keys_that_cannot_be_read_come_over_without_a_key() {
    let scratch = Scratch::new("keys");
    let config = "[providers.a]\nbase_url = \"http://127.0.0.1:9/v1\"\nkey = { secret = \"a\" }\n\n[providers.b]\nbase_url = \"http://127.0.0.1:8/v1\"\nkey = { secret = \"nobody\" }\n";
    scratch.config(config).secrets(&format!("a = \"{FAKE}\"\n"));
    // 引用的名字密钥里没有。
    let names = ticked(&scratch.import());
    assert!(
        names
            .iter()
            .any(|n| n.contains("密钥库中没有 nobody，不导入密钥")),
        "{names:?}"
    );
    // 密钥文件读不了（权限）。
    let secrets = scratch.root().join("etc/private/secrets.toml");
    std::fs::set_permissions(&secrets, std::fs::Permissions::from_mode(0o000)).unwrap();
    if std::fs::read(&secrets).is_ok() {
        return; // 在 root 底下跑：权限拦不住，这一种测不了。
    }
    let mut settings = scratch.import();
    let names = ticked(&settings);
    assert!(
        names
            .iter()
            .all(|n| n.contains("无法读取密钥库（权限不足），不导入密钥")),
        "{names:?}"
    );
    press(&mut settings, KeyCode::Enter);
    assert_eq!(status(&settings).1, Tone::Good);
    let (config, secrets_text) = lite(&scratch);
    assert!(!config.contains("key ="), "取不到的不写引用：{config}");
    assert!(secrets_text.is_empty());
    // 密钥文件写坏了。
    restore(&scratch.0);
    scratch.secrets(&format!("a = \"{FAKE}\n"));
    let settings = scratch.import();
    assert!(
        ticked(&settings)
            .iter()
            .any(|n| n.contains("无法读取密钥库（格式错误）"))
    );
    no_fake(&settings);
    // Miyu 的配置本身读不了。
    std::fs::set_permissions(
        scratch.root().join("etc/config.toml"),
        std::fs::Permissions::from_mode(0o000),
    )
    .unwrap();
    assert_eq!(
        status(&scratch.import()).0,
        "无法读取 Miyu 的配置：权限不足"
    );
}

#[test]
fn a_read_only_config_dir_saves_nothing_and_leaves_no_half_file() {
    let scratch = Scratch::new("readonly");
    scratch
        .config(TWO)
        .secrets(&format!("relay = \"{FAKE}\"\n"));
    let config_dir = scratch.paths().config;
    std::fs::set_permissions(&config_dir, std::fs::Permissions::from_mode(0o500)).unwrap();
    if std::fs::write(config_dir.join("probe"), "").is_ok() {
        return; // root 底下拦不住。
    }
    let mut settings = scratch.import();
    press(&mut settings, KeyCode::Enter);
    let (said, tone) = status(&settings);
    assert_eq!(tone, Tone::Bad);
    assert!(said.starts_with("保存失败："), "{said}");
    no_fake(&settings);
    let left: Vec<_> = std::fs::read_dir(&config_dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name())
        .collect();
    assert!(left.is_empty(), "不留半截文件：{left:?}");
}
