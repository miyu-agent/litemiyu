//! 「供应商和模型」整页走一遍：加一家（照目录只填 key、自定义）、改名、删、选默认模型、建池删池；返工以前写的配置文件照样读得
//! 出、改得了、存回去不丢项。数据都在临时目录，key 是假的。

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use lm_engine::store::paths::Paths;

use lm_kernel::config::Value;

use super::Popup;
use super::forms::Field;
use crate::settings::{Outcome, Settings};

/// 临时的数据目录，用完删掉。
struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!(
            "litemiyu-settings-models-{}-{name}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }

    fn paths(&self) -> Paths {
        let paths = Paths::under(&self.0);
        std::fs::create_dir_all(&paths.config).unwrap();
        paths
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if std::fs::remove_dir_all(&self.0).is_err() {}
    }
}

/// 假 key：测试守着它不进配置文件。
const FAKE_KEY: &str = "sk-fake-litemiyu-models-0000";

fn press(settings: &mut Settings, code: KeyCode) -> Outcome {
    let outcome = settings.key(KeyEvent::new(code, KeyModifiers::NONE));
    // 写盘在写线程里（毛病二）：按完等排着的写完，下面照盘上的、写完以后的状态行查。
    settings.flush_writes();
    outcome
}

fn typed(settings: &mut Settings, text: &str) {
    for c in text.chars() {
        press(settings, KeyCode::Char(c));
    }
}

fn open(paths: Paths) -> Settings {
    let mut settings = Settings::at(paths, "zh", Outcome::Quit).unwrap();
    settings.show_models();
    settings
}

fn config(paths: &Paths) -> String {
    std::fs::read_to_string(paths.config_file()).unwrap_or_default()
}

/// 配置里写着的：真的键到值（照配置服务读的）。
fn written(paths: &Paths) -> std::collections::BTreeMap<String, Value> {
    lm_engine::settings::Settings::new(paths.clone(), |_| None)
        .read()
        .unwrap()
        .written
}

fn text(value: &str) -> Value {
    Value::Text(value.to_string().into())
}

fn popup(settings: &Settings) -> Option<&Popup> {
    settings.models.as_ref().and_then(|m| m.popup.as_ref())
}

/// 返工以前的界面写出来的那种配置文件（主人手里的那一份就是这个样子）：一家照目录加的、一家自定义的，key 是引用，主对话的
/// 模型、一个池（LM-8 起是退役的键：读得进、不进最终值、存的时候原样留着）、终端的几项都写着。
const OLD: &str = r#"[providers.deepseek]
key = { secret = "deepseek" }

[providers.relay]
name = "中转"
driver = "openai-chat"
base_url = "http://127.0.0.1:9/v1"
key = { env = "RELAY_KEY" }
catalog = "relay"

[providers.relay.models."m-old"]
window = 64000

[models]
chat = "deepseek/deepseek-chat"

[pools.daily]
models = ["deepseek/deepseek-chat", "relay/m-old"]
strategy = "rotate"

[tui]
keys = "vim"
theme_background = false

[sessions]
default = "bare"
"#;

#[test]
fn an_old_config_reads_edits_and_saves_without_losing_anything() {
    let scratch = Scratch::new("old");
    let paths = scratch.paths();
    crate::settings::put_catalog(&paths);
    std::fs::write(paths.config_file(), OLD).unwrap();
    let before = written(&paths);
    assert_eq!(before.len(), 11, "池的两项退役了，不算：{before:?}");
    assert!(
        !before.keys().any(|key| key.starts_with("pools.")),
        "{before:?}"
    );
    let mut settings = open(paths.clone());
    let models = settings.models.as_ref().unwrap();
    let ids: Vec<&str> = models
        .data
        .providers
        .iter()
        .map(|p| p.id.as_str())
        .collect();
    assert_eq!(ids, ["deepseek", "relay"]);
    assert_eq!(models.data.chat.as_deref(), Some("deepseek/deepseek-chat"));
    let relay = models.data.provider("relay").unwrap();
    assert_eq!(relay.shown(), "中转");
    assert!(
        relay.models.iter().any(|m| m.name == "m-old"),
        "写了资料的模型列得出"
    );
    assert!(
        models.data.provider("deepseek").unwrap().models.len() > 1,
        "照目录列 DeepSeek 的模型"
    );
    // 改中转的显示名，存。
    press(&mut settings, KeyCode::Char('j'));
    press(&mut settings, KeyCode::Enter);
    assert!(matches!(popup(&settings), Some(Popup::Form(_))));
    press(&mut settings, KeyCode::Enter);
    for _ in 0..2 {
        press(&mut settings, KeyCode::Backspace);
    }
    typed(&mut settings, "站点");
    press(&mut settings, KeyCode::Enter);
    press(&mut settings, KeyCode::Char('s'));
    assert!(popup(&settings).is_none(), "存成了关窗");
    // 只改了显示名，别的一项不少、一项不变。
    let mut expected = before.clone();
    expected.insert("providers.relay.name".into(), text("站点"));
    assert_eq!(written(&paths), expected);
    let saved = config(&paths);
    assert!(
        saved.contains("[pools.daily]\nmodels = [\"deepseek/deepseek-chat\", \"relay/m-old\"]\nstrategy = \"rotate\""),
        "退役的池原样留在文件里：{saved}"
    );
}

#[test]
fn adding_a_featured_provider_asks_only_for_the_key_and_keeps_it_out_of_the_config() {
    let scratch = Scratch::new("featured");
    let paths = scratch.paths();
    crate::settings::put_catalog(&paths);
    let mut settings = open(paths.clone());
    press(&mut settings, KeyCode::Char('a'));
    let Some(Popup::Catalog(pick)) = popup(&settings) else {
        panic!("先弹目录");
    };
    let at = pick
        .rows()
        .iter()
        .position(|row| matches!(row, super::catalog::Row::Provider(p) if p.id == "deepseek"))
        .unwrap();
    for _ in 0..at {
        press(&mut settings, KeyCode::Char('j'));
    }
    press(&mut settings, KeyCode::Enter);
    let Some(Popup::Form(form)) = popup(&settings) else {
        panic!("选了一家开只填 key 的窗");
    };
    assert_eq!(form.key_only.as_deref(), Some("DeepSeek"));
    assert!(form.editing.is_some(), "直接打字");
    typed(&mut settings, FAKE_KEY);
    press(&mut settings, KeyCode::Enter);
    assert!(popup(&settings).is_none(), "回车就存");
    let text = config(&paths);
    assert!(text.contains("[providers.deepseek]"), "{text}");
    assert!(text.contains(r#"key = { secret = "deepseek" }"#), "{text}");
    assert!(!text.contains(FAKE_KEY), "明文不进配置文件");
    let secrets = std::fs::read_to_string(paths.secrets_file()).unwrap();
    assert!(secrets.contains(FAKE_KEY));
    let models = settings.models.as_ref().unwrap();
    assert_eq!(
        models.nav.provider(&models.data).map(|p| p.id.as_str()),
        Some("deepseek"),
        "存完选中新加的一家"
    );
}

#[test]
fn a_custom_provider_needs_a_valid_id_and_an_env_key_writes_a_reference() {
    let scratch = Scratch::new("custom");
    let paths = scratch.paths();
    let mut settings = open(paths.clone());
    press(&mut settings, KeyCode::Char('a'));
    let Some(Popup::Catalog(pick)) = popup(&settings) else {
        panic!("先弹目录");
    };
    let custom = pick
        .rows()
        .iter()
        .position(|row| *row == super::catalog::Row::Custom)
        .unwrap();
    while settings
        .models
        .as_ref()
        .and_then(|m| match &m.popup {
            Some(Popup::Catalog(pick)) => Some(pick.sel),
            _ => None,
        })
        .is_some_and(|sel| sel < custom)
    {
        press(&mut settings, KeyCode::Char('j'));
    }
    press(&mut settings, KeyCode::Enter);
    // ID 填个不合写法的：存不成，窗里红字。
    press(&mut settings, KeyCode::Enter);
    typed(&mut settings, "Bad Id");
    press(&mut settings, KeyCode::Enter);
    press(&mut settings, KeyCode::Char('s'));
    let Some(Popup::Form(form)) = popup(&settings) else {
        panic!("存不成窗留着");
    };
    assert!(form.error.as_deref().is_some_and(|e| e.contains("Bad Id")));
    // 改成合写法的，填地址，认证换环境变量、填变量名。
    press(&mut settings, KeyCode::Enter);
    for _ in 0..6 {
        press(&mut settings, KeyCode::Backspace);
    }
    typed(&mut settings, "my-relay");
    press(&mut settings, KeyCode::Enter);
    press(&mut settings, KeyCode::Char('j'));
    press(&mut settings, KeyCode::Char('j'));
    press(&mut settings, KeyCode::Enter);
    typed(&mut settings, "http://127.0.0.1:9/v1");
    press(&mut settings, KeyCode::Enter);
    press(&mut settings, KeyCode::Char('j'));
    press(&mut settings, KeyCode::Char('j'));
    press(&mut settings, KeyCode::Char('l'));
    let Some(Popup::Form(form)) = popup(&settings) else {
        panic!("窗开着");
    };
    assert_eq!(form.choice(Field::Auth), Some("env"));
    press(&mut settings, KeyCode::Char('j'));
    press(&mut settings, KeyCode::Enter);
    typed(&mut settings, "MY_RELAY_KEY");
    press(&mut settings, KeyCode::Enter);
    press(&mut settings, KeyCode::Char('s'));
    assert!(popup(&settings).is_none(), "{:?}", popup(&settings));
    let text = config(&paths);
    assert!(text.contains("[providers.my-relay]"), "{text}");
    assert!(
        text.contains(r#"base_url = "http://127.0.0.1:9/v1""#),
        "{text}"
    );
    assert!(text.contains(r#"key = { env = "MY_RELAY_KEY" }"#), "{text}");
}

#[test]
fn renaming_a_provider_moves_its_table_and_what_points_at_it() {
    let scratch = Scratch::new("rename");
    let paths = scratch.paths();
    std::fs::write(paths.config_file(), OLD).unwrap();
    let mut settings = open(paths.clone());
    press(&mut settings, KeyCode::Char('j'));
    press(&mut settings, KeyCode::Enter);
    // 已有的停在显示名称，往上一格是 ID。
    press(&mut settings, KeyCode::Char('k'));
    press(&mut settings, KeyCode::Enter);
    for _ in 0.."relay".len() {
        press(&mut settings, KeyCode::Backspace);
    }
    typed(&mut settings, "station");
    press(&mut settings, KeyCode::Enter);
    press(&mut settings, KeyCode::Char('s'));
    assert!(popup(&settings).is_none(), "{:?}", popup(&settings));
    let after = written(&paths);
    assert!(!after.keys().any(|key| key.starts_with("providers.relay.")));
    assert_eq!(after.get("providers.station.name"), Some(&text("中转")));
    assert_eq!(
        after.get("providers.station.models.m-old.window"),
        Some(&Value::Int(64000))
    );
}

#[test]
fn deleting_asks_first() {
    let scratch = Scratch::new("delete");
    let paths = scratch.paths();
    std::fs::write(paths.config_file(), OLD).unwrap();
    let mut settings = open(paths.clone());
    press(&mut settings, KeyCode::Char('j'));
    press(&mut settings, KeyCode::Char('d'));
    let Some(Popup::Confirm(confirm)) = popup(&settings) else {
        panic!("先问");
    };
    assert_eq!(confirm.sel, 1, "停在「取消」");
    assert!(
        confirm.lines.is_empty(),
        "默认模型不是它的，没人用它（LM-8 砍了模型池，不再说「池 daily」）：{:?}",
        confirm.lines
    );
    press(&mut settings, KeyCode::Char('h'));
    press(&mut settings, KeyCode::Enter);
    let after = written(&paths);
    assert!(!after.keys().any(|key| key.starts_with("providers.relay.")));
    assert!(after.contains_key("providers.deepseek.key"));
}

#[test]
fn the_default_model_is_saved_from_its_window() {
    let scratch = Scratch::new("defaults");
    let paths = scratch.paths();
    std::fs::write(paths.config_file(), OLD).unwrap();
    let mut settings = open(paths.clone());
    press(&mut settings, KeyCode::Char('.'));
    press(&mut settings, KeyCode::Enter);
    let Some(Popup::Pick(_)) = popup(&settings) else {
        panic!("默认模型页 Enter 开选模型的窗");
    };
    press(&mut settings, KeyCode::Char('/'));
    typed(&mut settings, "m-old");
    press(&mut settings, KeyCode::Enter);
    press(&mut settings, KeyCode::Enter);
    assert!(config(&paths).contains(r#"chat = "relay/m-old""#));
}
