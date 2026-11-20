//! 命令行的表、读参数、帮助页、字。

use std::collections::BTreeSet;

use unicode_width::UnicodeWidthStr;

use super::parse::{self, Misuse};
use super::spec::{Cmd, nearest};
use super::words::Words;
use super::{ROOT, help};

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(ToString::to_string).collect()
}

/// 表里每一条命令连同全名。
fn every() -> Vec<(String, &'static Cmd)> {
    fn walk(path: String, cmd: &'static Cmd, into: &mut Vec<(String, &'static Cmd)>) {
        into.push((path.clone(), cmd));
        for sub in cmd.subs {
            walk(format!("{path} {}", sub.name), sub, into);
        }
    }
    let mut all = Vec::new();
    walk("litemiyu".to_string(), &ROOT, &mut all);
    all
}

#[test]
fn the_top_level_reads_values_both_ways_and_refuses_what_it_does_not_know() {
    let read = parse::read(
        &ROOT,
        &args(&["--rescue", "--model=m", "--base-url", "http://x", "--bare"]),
    )
    .expect("写得对");
    assert!(read.has("rescue") && read.has("bare"));
    assert_eq!(read.value("model"), Some("m"));
    assert_eq!(read.value("base-url"), Some("http://x"));
    assert!(
        parse::read(&ROOT, &args(&["-h"]))
            .expect("写得对")
            .has("help")
    );
    assert_eq!(
        parse::read(&ROOT, &args(&["--rescue", "--api"])).map(|_| ()),
        Err(Misuse::NoValue("api"))
    );
    for (list, arg) in [
        (&["--verison"][..], "--verison"),
        (&["--bare", "--nope"][..], "--nope"),
        (&["--bare=1"][..], "--bare=1"),
        (&["--rescue=yes"][..], "--rescue=yes"),
        (&["-x"][..], "-x"),
        (&["-h=1"][..], "-h=1"),
    ] {
        assert!(
            matches!(parse::read(&ROOT, &args(list)), Err(Misuse::UnknownOption(got, _)) if got == arg),
            "{list:?}"
        );
    }
    let read = parse::read(&ROOT, &args(&["--model", "--version"])).expect("写得对");
    assert!(!read.has("version"), "跟在要值的后面是值");
    assert_eq!(
        parse::read(&ROOT, &args(&["--verison"])).map(|_| ()),
        Err(Misuse::UnknownOption(
            "--verison".into(),
            Some("--version".into())
        )),
        "带上最接近的"
    );
}

#[test]
fn words_after_a_double_dash_and_a_lone_dash_are_words() {
    let ask = ROOT.sub("ask").expect("有 ask");
    let read = parse::read(ask, &args(&["-c", "--", "-s", "-", "x"])).expect("写得对");
    assert!(read.has("continue") && !read.has("session"));
    assert_eq!(read.words, args(&["-s", "-", "x"]));
    let read = parse::read(ask, &args(&["-sabc", "hi"])).expect("写得对");
    assert_eq!(read.value("session"), Some("abc"));
}

#[test]
fn the_nearest_name_is_suggested_only_when_close() {
    let names = ["ask", "config", "help"];
    assert_eq!(nearest("aks", names), Some("ask"));
    assert_eq!(nearest("confg", names), Some("config"));
    assert_eq!(nearest("conf", names), Some("config"), "开头");
    assert_eq!(nearest("zzzzzz", names), None);
}

#[test]
fn durations_and_counts_read_like_gnu_timeout() {
    use std::time::Duration;
    assert_eq!(parse::duration("30"), Some(Duration::from_secs(30)));
    assert_eq!(parse::duration("30s"), Some(Duration::from_secs(30)));
    assert_eq!(parse::duration("10m"), Some(Duration::from_secs(600)));
    assert_eq!(parse::duration("1h"), Some(Duration::from_secs(3600)));
    for bad in ["0", "-1", "1.5", "5d", "", "s", "m5"] {
        assert_eq!(parse::duration(bad), None, "{bad}");
    }
    assert_eq!(parse::positive("3"), Some(3));
    for bad in ["0", "-3", "x", ""] {
        assert_eq!(parse::positive(bad), None, "{bad}");
    }
}

#[test]
fn exclusive_and_new_only_options_are_refused_together() {
    let ask = ROOT.sub("ask").expect("有 ask");
    let read = parse::read(ask, &args(&["-c", "-s", "x"])).expect("写得对");
    assert_eq!(
        parse::exclusive(&read, &[&["session", "continue"]]),
        Err(Misuse::Conflict("session", "continue"))
    );
    let read = parse::read(ask, &args(&["-c", "--bare"])).expect("写得对");
    assert_eq!(
        parse::only_new(&read, &["cd", "bare"]),
        Err(Misuse::Conflict("bare", "continue"))
    );
}

#[test]
fn every_help_page_fits_80_columns_lists_every_option_and_ends_with_a_newline() {
    for code in ["zh", "en"] {
        let words = Words::load(code);
        for (path, cmd) in every() {
            let page = help::page(&words, &path, cmd);
            assert!(
                page.ends_with('\n') && !page.ends_with("\n\n"),
                "{code} {path}"
            );
            for line in page.lines() {
                assert!(line.width() <= 80, "{code} {path} 太宽：{line}");
            }
            for opt in cmd.opts {
                assert!(
                    page.contains(&format!("--{}", opt.long)),
                    "{code} {path} 少了 --{}",
                    opt.long
                );
            }
            for sub in cmd.subs {
                assert!(
                    page.contains(&format!("  {}", sub.name)),
                    "{code} {path} 少了 {}",
                    sub.name
                );
            }
        }
    }
}

/// 代码里照字面取的编号（`get("…")`、`say("…"`）：扫 `src/cli/` 下的源码。
fn literal_keys() -> BTreeSet<String> {
    fn walk(dir: &std::path::Path, into: &mut BTreeSet<String>) {
        for entry in std::fs::read_dir(dir).expect("读得了源码目录").flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, into);
            } else if path.extension().is_some_and(|ext| ext == "rs") && !path.ends_with("tests.rs")
            {
                let text = std::fs::read_to_string(&path).expect("读得了源码");
                for opener in ["get(\"", "say(\"", "say(\n                \""] {
                    for piece in text.split(opener).skip(1) {
                        if let Some(key) = piece.split('"').next() {
                            into.insert(key.to_string());
                        }
                    }
                }
            }
        }
    }
    let mut keys = BTreeSet::new();
    walk(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/cli"),
        &mut keys,
    );
    keys
}

#[test]
fn every_key_in_the_table_and_the_code_is_in_both_languages() {
    let mut keys = literal_keys();
    for (_, cmd) in every() {
        for key in [cmd.usage, cmd.summary, cmd.about, cmd.extra] {
            if !key.is_empty() {
                keys.insert(key.to_string());
            }
        }
        for opt in cmd.opts.iter().chain(std::iter::once(&super::spec::HELP)) {
            keys.insert(opt.help.to_string());
            keys.extend(opt.value.map(str::to_string));
        }
    }
    // 拼出来的编号：结束的原因、出错的分类。
    for reason in ["interrupted", "timed_out", "max_steps"] {
        keys.insert(format!("end.{reason}"));
    }
    for class in [
        "retryable",
        "rate_limited",
        "context_too_long",
        "auth",
        "quota",
        "content_policy",
        "bad_stream",
        "empty_reply",
        "output_limit",
        "bad_summary",
        "compaction_paused",
        "cooling",
        "stalled",
        "no_model",
        "other",
    ] {
        keys.insert(format!("class.{class}"));
    }
    assert!(keys.len() > 50, "扫到了源码里的编号：{}", keys.len());
    for code in ["zh", "en"] {
        let words = Words::load(code);
        assert_eq!(words.code(), code);
        for key in &keys {
            assert_ne!(words.get(key), key.as_str(), "{code} 没有 {key}");
        }
    }
}

/// `src/cli/` 下源码（不算测试）里写着的每一个字串字面量（引号里只有字母、数字、`_`、`.` 的）：编号不只照 `get`、`say` 取，
/// 也有写在表里、`Misuse::Invalid` 这种地方的。
fn literals() -> BTreeSet<String> {
    fn walk(dir: &std::path::Path, into: &mut BTreeSet<String>) {
        for entry in std::fs::read_dir(dir).expect("读得了源码目录").flatten() {
            let path = entry.path();
            if path.is_dir() {
                walk(&path, into);
            } else if path.extension().is_some_and(|ext| ext == "rs") && !path.ends_with("tests.rs")
            {
                let text = std::fs::read_to_string(&path).expect("读得了源码");
                for piece in text.split('"').skip(1).step_by(2) {
                    if !piece.is_empty()
                        && piece
                            .chars()
                            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.'))
                    {
                        into.insert(piece.to_string());
                    }
                }
            }
        }
    }
    let mut found = BTreeSet::new();
    walk(
        &std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/cli"),
        &mut found,
    );
    found
}

/// 字表里没有用不到的句子（LM-3 删子命令时照这条清的）：两种语言的编号一样，每个都在源码里写着（拼出来的 `end.*`、
/// `class.*` 两类除外，上一条测试照名单查它们在不在）。
#[test]
fn every_key_in_both_languages_is_used() {
    let written = literals();
    let keys = |json: &str| -> BTreeSet<String> {
        let value: serde_json::Value = serde_json::from_str(json).expect("读得懂");
        value["cli"]
            .as_object()
            .expect("有 cli 那一格")
            .keys()
            .cloned()
            .collect()
    };
    let zh = keys(tui_json!("cli/zh.json"));
    let en = keys(tui_json!("cli/en.json"));
    assert_eq!(zh, en, "两种语言的编号一样");
    let unused: Vec<&String> = zh
        .iter()
        .filter(|key| !key.starts_with("end.") && !key.starts_with("class."))
        .filter(|key| !written.contains(*key))
        .collect();
    assert!(unused.is_empty(), "没人用的：{unused:?}");
}
