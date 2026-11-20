//! 配置服务：读最终值和来源、改几项只动那几行、存密钥（0600、名字错开）、记日志。

use std::os::unix::fs::PermissionsExt;

use lm_kernel::config::Value;
use lm_kernel::config::secret::{Reference, Secret};

use super::{Edit, SetError, Settings, free_name};
use crate::store::paths::Paths;
use crate::store::test_support::Scratch;

/// 临时目录下的四处，写好配置（`None` 的不写）。
fn settings(scratch: &Scratch, config: Option<&str>) -> Settings {
    let paths = Paths::under(scratch.path());
    std::fs::create_dir_all(&paths.config).expect("建得了配置那一处");
    if let Some(text) = config {
        std::fs::write(paths.config_file(), text).expect("写得进");
    }
    Settings::new(paths, |name| {
        (name == "LITEMIYU_LOG").then(|| "debug".to_string())
    })
}

fn text(value: &str) -> Value {
    Value::Text(value.to_string().into())
}

fn config_text(settings: &Settings) -> String {
    std::fs::read_to_string(settings.paths().config_file()).expect("读得了")
}

#[test]
fn nothing_written_reads_as_defaults_and_no_file() {
    let scratch = Scratch::new();
    let read = settings(&scratch, None).read().expect("读得了");
    assert!(!read.exists);
    assert!(read.written.is_empty());
    let (value, origin) = read.resolved.get("ui.language").expect("有默认值");
    assert_eq!(value, &text("auto"));
    assert_eq!(origin.layer_name(), "default");
    assert!(read.secrets.is_empty());
    assert_eq!(read.broken, None);
}

#[test]
fn written_values_come_from_the_file_with_their_line() {
    let scratch = Scratch::new();
    let read = settings(&scratch, Some("# 我的配置\n[ui]\nlanguage = \"en\"\n"))
        .read()
        .expect("读得了");
    assert!(read.exists);
    assert_eq!(read.written.get("ui.language"), Some(&text("en")));
    let (_, origin) = read.resolved.get("ui.language").expect("写了");
    assert_eq!(origin.layer_name(), "system");
}

#[test]
fn a_broken_file_is_reported_and_cannot_be_edited() {
    let scratch = Scratch::new();
    let settings = settings(&scratch, Some("[ui\nlanguage = 1\n"));
    let read = settings.read().expect("读得了");
    assert_eq!(read.broken.map(|(line, _)| line), Some(1));
    let edited = settings.set(&[Edit::Set("ui.language".into(), text("zh"))]);
    assert!(matches!(edited, Err(SetError::Broken(_))), "{edited:?}");
    assert_eq!(
        config_text(&settings),
        "[ui\nlanguage = 1\n",
        "一个字节没动"
    );
}

/// 设计第五节第 13 条的验收：改一项，文件里只多出这一项，注释和先后不变。
#[test]
fn setting_one_item_adds_only_that_line_and_keeps_the_rest() {
    let scratch = Scratch::new();
    let before = "# 我的配置\n\n[models]\n# 主力\nchat = \"a/b\"\n\n[providers.a]\nbase_url = \"http://127.0.0.1:1/v1\"\n";
    let settings = settings(&scratch, Some(before));
    let changed = settings
        .set(&[Edit::Set("sessions.default".into(), text("bare"))])
        .expect("改得了");
    assert_eq!(changed, vec!["sessions.default".to_string()]);
    let after = config_text(&settings);
    assert!(after.starts_with(before), "原来的字原样在前面：\n{after}");
    assert_eq!(
        &after[before.len()..],
        "\n[sessions]\ndefault = \"bare\"\n",
        "只多出这一项"
    );
}

#[test]
fn changing_and_unsetting_touch_only_their_lines() {
    let scratch = Scratch::new();
    let before = "# 头\n[ui]\nlanguage = \"en\" # 行尾的注释\n\n[sessions]\ndefault = \"bare\"\n";
    let settings = settings(&scratch, Some(before));
    settings
        .set(&[
            Edit::Set("ui.language".into(), text("zh")),
            Edit::Unset("sessions.default".into()),
        ])
        .expect("改得了");
    let after = config_text(&settings);
    assert!(
        after.starts_with("# 头\n[ui]\nlanguage = \"zh\""),
        "{after}"
    );
    assert!(!after.contains("default = "), "删掉了：\n{after}");
}

#[test]
fn the_same_value_writes_nothing() {
    let scratch = Scratch::new();
    let settings = settings(&scratch, Some("[ui]\nlanguage = \"en\"\n"));
    let changed = settings
        .set(&[
            Edit::Set("ui.language".into(), text("en")),
            Edit::Unset("sessions.default".into()),
        ])
        .expect("改得了");
    assert!(changed.is_empty());
    assert!(!settings.paths().journal().exists(), "没改就不记");
}

#[test]
fn a_new_file_holds_only_the_changed_items() {
    let scratch = Scratch::new();
    let settings = settings(&scratch, None);
    settings
        .set(&[
            Edit::Set(
                "providers.fake.base_url".into(),
                text("http://127.0.0.1:1/v1"),
            ),
            Edit::Set(
                "providers.fake.key".into(),
                Value::Secret(Reference::Secret("fake".into())),
            ),
            Edit::Set("models.chat".into(), text("fake/m1")),
        ])
        .expect("改得了");
    let after = config_text(&settings);
    assert!(after.contains("[providers.fake]\n"), "{after}");
    assert!(after.contains("key = { secret = \"fake\" }"), "{after}");
    assert!(after.contains("chat = \"fake/m1\""), "{after}");
    let read = settings.read().expect("读得了");
    assert_eq!(read.written.len(), 3);
}

#[test]
fn every_item_of_one_call_is_checked_before_anything_is_written() {
    let scratch = Scratch::new();
    let settings = settings(&scratch, Some("[ui]\nlanguage = \"en\"\n"));
    let edited = settings.set(&[
        Edit::Set("ui.language".into(), text("zh")),
        Edit::Set("ui.language_typo".into(), text("zh")),
    ]);
    assert!(matches!(edited, Err(SetError::UnknownKey(ref key)) if key == "ui.language_typo"));
    let edited = settings.set(&[Edit::Set("ui.language".into(), text("ja"))]);
    assert!(matches!(edited, Err(SetError::Invalid(ref key, _)) if key == "ui.language"));
    assert_eq!(config_text(&settings), "[ui]\nlanguage = \"en\"\n");
}

#[test]
fn a_change_is_recorded_in_the_journal() {
    let scratch = Scratch::new();
    let settings = settings(&scratch, None);
    settings
        .set(&[Edit::Set("ui.language".into(), text("zh"))])
        .expect("改得了");
    let journal = std::fs::read_to_string(settings.paths().journal()).expect("记了");
    assert!(journal.contains("\"kind\":\"config.changed\""), "{journal}");
    assert!(journal.contains("\"key\":\"ui.language\""), "{journal}");
    assert!(journal.contains("\"new\":\"zh\""), "{journal}");
}

#[test]
fn secrets_are_private_and_named_apart() {
    let scratch = Scratch::new();
    let settings = settings(&scratch, None);
    let key = Secret::new("sk-one").expect("合写法");
    assert_eq!(settings.store_secret("fake", &key).expect("存得了"), "fake");
    let other = Secret::new("sk-two").expect("合写法");
    assert_eq!(
        settings.store_secret("fake", &other).expect("存得了"),
        "fake-2"
    );
    let path = settings.paths().secrets_file();
    let mode = std::fs::metadata(&path).expect("在").permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
    let read = settings.read().expect("读得了");
    assert_eq!(
        read.secrets.into_iter().collect::<Vec<_>>(),
        vec!["fake".to_string(), "fake-2".to_string()]
    );
    let journal = std::fs::read_to_string(settings.paths().journal()).expect("记了");
    assert!(journal.contains("secrets.fake-2"), "{journal}");
    assert!(!journal.contains("sk-"), "日志里没有明文：{journal}");
}

#[test]
fn a_bad_secret_name_is_refused() {
    let scratch = Scratch::new();
    let settings = settings(&scratch, None);
    let key = Secret::new("sk-one").expect("合写法");
    let stored = settings.store_secret("不合写法 的名字", &key);
    assert!(
        matches!(stored, Err(SetError::Invalid(_, "bad_format"))),
        "{stored:?}"
    );
    assert!(!settings.paths().secrets_file().exists());
}

#[test]
fn free_names_count_up_until_one_is_free() {
    assert_eq!(free_name("a", |_| false).as_deref(), Some("a"));
    assert_eq!(
        free_name("a", |name| name == "a" || name == "a-2").as_deref(),
        Some("a-3")
    );
}
