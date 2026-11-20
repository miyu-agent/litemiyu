//! 应急模式进界面以前：给了什么、缺什么，配置读不了照样起（命令行怎么读在 `cli/tests.rs`）。环境一律是测试给的一份，数据那几处在临时目录里。

use std::collections::HashMap;

use super::{API, API_KEY, Args, BASE_URL, MODEL, NotReady, prepare};

/// 假 key。
const KEY: &str = "sk-FAKE-rescue-not-real";

/// 命令行读好的（`--rescue` 和三个带值的，照 `cli` 读好以后交来的样子）。
fn args(list: &[&str]) -> Args {
    let mut values: HashMap<&str, String> = HashMap::new();
    let mut rest = list.iter();
    while let Some(arg) = rest.next() {
        if let Some(name) = arg.strip_prefix("--").filter(|name| *name != "rescue") {
            values.insert(name, rest.next().expect("带值").to_string());
        }
    }
    Args::given(
        list.contains(&"--rescue"),
        values.remove("base-url"),
        values.remove("model"),
        values.remove("api"),
    )
}

/// 一份环境：给的这几样，`LITEMIYU_HOME` 指到 `home`。
fn env(
    home: &std::path::Path,
    pairs: &[(&str, &str)],
) -> impl Fn(&str) -> Option<String> + Clone + Send + Sync + 'static {
    let mut map: HashMap<String, String> = pairs
        .iter()
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect();
    map.insert("LITEMIYU_HOME".into(), home.to_string_lossy().into_owned());
    move |name: &str| map.get(name).cloned()
}

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("litemiyu-rescue-{name}-{}", std::process::id()));
    drop(std::fs::remove_dir_all(&dir));
    dir
}

#[test]
fn nothing_given_and_no_config_says_what_is_missing() {
    let home = scratch("nothing");
    let missing = prepare(&args(&["--rescue"]), env(&home, &[]));
    assert_eq!(
        missing.err(),
        Some(NotReady::Missing(vec![BASE_URL, MODEL], None))
    );
    let only_url = prepare(
        &args(&["--rescue"]),
        env(&home, &[(BASE_URL, "http://127.0.0.1:9/v1")]),
    );
    assert_eq!(only_url.err(), Some(NotReady::Missing(vec![MODEL], None)));
    let only_key = prepare(&args(&["--rescue"]), env(&home, &[(API_KEY, KEY)]));
    assert_eq!(
        only_key.err(),
        Some(NotReady::Missing(vec![BASE_URL, MODEL], None))
    );
    assert!(!home.exists(), "一个目录都不建");
}

#[test]
fn the_command_line_beats_the_environment_and_a_key_is_optional() {
    let home = scratch("given");
    let given = env(
        &home,
        &[
            (BASE_URL, "http://wrong.invalid"),
            (MODEL, "m-env"),
            (API, "anthropic"),
        ],
    );
    let read = args(&["--rescue", "--base-url", "http://127.0.0.1:9/v1"]);
    let prepared = prepare(&read, given).expect("给齐了");
    assert!(prepared.keyless, "没给 key");
    let values = prepared.rescued.loaded.resolved.values();
    assert_eq!(
        lm_kernel::models::provider::chat(&values).as_deref(),
        Some("rescue/m-env")
    );
    assert!(!home.exists());
}

#[test]
fn a_broken_config_still_starts_and_a_bad_value_is_named() {
    let home = scratch("broken");
    let config = home.join("config");
    std::fs::create_dir_all(&config).expect("建得了");
    std::fs::write(config.join("config.toml"), "[[[").expect("写得进");
    let given = env(
        &home,
        &[
            (BASE_URL, "http://127.0.0.1:9/v1"),
            (MODEL, "m"),
            (API_KEY, KEY),
        ],
    );
    let prepared = prepare(&args(&["--rescue"]), given).expect("配置坏了照样起");
    let why = prepared.rescued.unreadable.expect("说了配置读不了");
    assert!(why.contains("config.toml"), "{why}");
    assert!(!prepared.keyless);

    let alone = prepare(&args(&["--rescue"]), env(&home, &[]));
    assert!(
        matches!(&alone, Err(NotReady::Missing(_, Some(why))) if why.contains("config.toml")),
        "没给、配置又坏了：照实说两样"
    );
    let bad = prepare(
        &args(&["--rescue", "--api", "gpt"]),
        env(&home, &[(BASE_URL, "http://127.0.0.1:9/v1"), (MODEL, "m")]),
    );
    assert_eq!(bad.err(), Some(NotReady::Bad(API)));
    let bad = prepare(
        &args(&["--rescue", "--base-url", "nowhere"]),
        env(&home, &[(MODEL, "m")]),
    );
    assert_eq!(bad.err(), Some(NotReady::Bad(BASE_URL)));
    std::fs::remove_dir_all(&home).expect("删得掉");
}
