//! 配置界面要用到时才取模型目录（LM-9：程序不带目录）：开头的前几步不取、走到接模型那一步才取；取到了常用的几家补上、光标
//! 还在原来那一行；取不到的状态行说原因和「自定义…」，自定义照样能选；「供应商和模型」进页时取，开着的目录窗跟着换；缓存里
//! 新的不再取；关着 `update` 的不取。目录服务器在本机（`127.0.0.1`），从不连 models.dev。

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use lm_engine::store::paths::Paths;

use crate::settings::models::Popup;
use crate::settings::models::catalog::Row as PickRow;
use crate::settings::opening::{Row, Step};
use crate::settings::{Outcome, Settings, Tone};

/// 真目录裁出来的一份（`lm-kernel/testdata`）：有 DeepSeek。
const TRIMMED: &str = include_str!("../../../../lm-kernel/testdata/models/models-dev-trimmed.json");

/// 临时的数据目录，用完删掉。
struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!(
            "litemiyu-catalog-fetch-{}-{name}",
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

/// 本机的假目录服务器：每个请求回 `body`（JSON），数着来了几次。交回地址和次数。
fn serve(body: &'static str) -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/api.json", listener.local_addr().unwrap());
    let count = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&count);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else {
                return;
            };
            let mut head = Vec::new();
            let mut chunk = [0u8; 4096];
            while !head.windows(4).any(|w| w == b"\r\n\r\n") {
                match stream.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => head.extend_from_slice(&chunk[..n]),
                }
            }
            counted.fetch_add(1, Ordering::SeqCst);
            let reply = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            if stream.write_all(reply.as_bytes()).is_err() {}
        }
    });
    (url, count)
}

/// 一个没人听的端口：先占再放。
fn closed() -> String {
    let port = TcpListener::bind("127.0.0.1:0")
        .and_then(|listener| listener.local_addr())
        .unwrap()
        .port();
    format!("http://127.0.0.1:{port}/api.json")
}

fn press(settings: &mut Settings, code: KeyCode) -> Outcome {
    let outcome = settings.handle(&Event::Key(KeyEvent::new(code, KeyModifiers::NONE)));
    settings.flush_writes();
    outcome
}

/// 配置：界面语言钉成中文，目录照 `url` 取（`update` 照 `update`）。
fn config(paths: &Paths, url: &str, update: bool) {
    std::fs::write(
        paths.config_file(),
        format!(
            "[ui]\nlanguage = \"zh\"\n\n[models.catalog]\nurl = \"{url}\"\nupdate = {update}\n"
        ),
    )
    .unwrap();
}

/// 照真的数据起的那样：要用到目录时去取。
fn settings(paths: &Paths) -> Settings {
    let mut settings = Settings::at(paths.clone(), "zh", Outcome::Back).unwrap();
    settings.auto_catalog = true;
    settings
}

/// 等后台取完（最多十秒，照节拍收，同主循环）。
fn fetched(settings: &mut Settings) {
    let until = Instant::now() + Duration::from_secs(10);
    while settings.fetching_catalog() {
        assert!(Instant::now() < until, "十秒没取完");
        settings.poll_catalog();
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// 开头里接模型那一步的几家（不算「其他」「更多」「自定义」）。
fn listed(settings: &Settings) -> Vec<String> {
    settings
        .opening
        .as_ref()
        .unwrap()
        .model
        .rows
        .iter()
        .filter_map(|row| match row {
            Row::Provider(p) => Some(p.catalog.clone()),
            _ => None,
        })
        .collect()
}

/// 开头里接模型那一步画的那一句。
fn note(settings: &Settings) -> (String, Tone) {
    settings
        .catalog_note()
        .cloned()
        .unwrap_or((String::new(), Tone::Note))
}

fn status(settings: &Settings) -> (String, Tone) {
    settings
        .status
        .clone()
        .unwrap_or((String::new(), Tone::Note))
}

#[test]
fn the_opening_fetches_only_at_the_model_step_and_fills_the_list_in() {
    let scratch = Scratch::new("opening");
    let paths = scratch.paths();
    let (url, count) = serve(TRIMMED);
    config(&paths, &url, true);
    let mut settings = settings(&paths);
    settings.start_opening();
    for _ in 0..2 {
        press(&mut settings, KeyCode::Enter);
    }
    assert_eq!(count.load(Ordering::SeqCst), 0, "前几步不取");
    assert!(!settings.fetching_catalog());
    press(&mut settings, KeyCode::Enter);
    assert_eq!(settings.opening.as_ref().unwrap().step, Step::Model);
    assert!(listed(&settings).is_empty(), "还没有目录：一家都不列");
    assert_eq!(
        note(&settings),
        ("正在获取模型目录…".to_string(), Tone::Busy)
    );
    // 光标挪到「自定义…」上：取到了以后还停在它上面。
    let rows = settings.opening.as_ref().unwrap().model.rows.clone();
    let custom = rows.iter().position(|row| *row == Row::Custom).unwrap();
    while settings.opening.as_ref().unwrap().model.cursor < custom {
        press(&mut settings, KeyCode::Char('j'));
    }
    fetched(&mut settings);
    assert_eq!(count.load(Ordering::SeqCst), 1);
    assert!(
        listed(&settings).contains(&"deepseek".to_string()),
        "常用的几家补上了"
    );
    let model = &settings.opening.as_ref().unwrap().model;
    assert_eq!(model.rows[model.cursor], Row::Custom, "光标还在「自定义…」");
    assert_eq!(note(&settings), ("已获取模型目录".to_string(), Tone::Good));
    assert!(paths.model_cache().join("models-dev.json").is_file());
    // 再进一次开头：缓存里是新的，不再取。
    let mut again = self::settings(&paths);
    again.start_opening();
    for _ in 0..3 {
        press(&mut again, KeyCode::Enter);
    }
    assert!(!again.fetching_catalog());
    assert!(listed(&again).contains(&"deepseek".to_string()), "照缓存列");
    assert_eq!(count.load(Ordering::SeqCst), 1);
}

#[test]
fn an_unreachable_catalog_says_why_and_custom_is_still_there() {
    let scratch = Scratch::new("unreachable");
    let paths = scratch.paths();
    config(&paths, &closed(), true);
    let mut settings = settings(&paths);
    settings.start_opening();
    let started = Instant::now();
    for _ in 0..3 {
        press(&mut settings, KeyCode::Enter);
    }
    fetched(&mut settings);
    assert!(started.elapsed() < Duration::from_secs(6), "很快认输");
    let (said, tone) = note(&settings);
    assert_eq!(tone, Tone::Bad);
    // 原因是短的一句（LM-9补），只带原话最里面那一层，不整句搬 reqwest 的「error sending request: client error (Connect): …」。
    assert!(
        said.starts_with("未能获取模型目录：无法连接到服务器（")
            && said.ends_with("）。可以选择「自定义…」，手动填写地址和接口"),
        "{said}"
    );
    assert!(!said.contains("error sending request"), "{said}");
    let rows = &settings.opening.as_ref().unwrap().model.rows;
    assert!(rows.contains(&Row::Custom) && rows.contains(&Row::More));
    assert!(listed(&settings).is_empty());
    // 半分钟里再走一次不再取（不每按一下都连一次）。
    press(&mut settings, KeyCode::Esc);
    press(&mut settings, KeyCode::Enter);
    assert!(!settings.fetching_catalog());
}

#[test]
fn the_models_page_fetches_and_an_open_picker_takes_the_new_providers() {
    let scratch = Scratch::new("page");
    let paths = scratch.paths();
    let (url, count) = serve(TRIMMED);
    config(&paths, &url, true);
    let mut settings = settings(&paths);
    settings.show_models();
    assert!(settings.fetching_catalog(), "进页就取");
    assert_eq!(
        status(&settings),
        ("正在获取模型目录…".to_string(), Tone::Busy)
    );
    // 目录窗开着、光标在「自定义…」上。
    press(&mut settings, KeyCode::Char('a'));
    let pick = |settings: &Settings| match settings.models.as_ref().and_then(|m| m.popup.as_ref()) {
        Some(Popup::Catalog(pick)) => pick.clone(),
        other => panic!("目录窗开着：{other:?}"),
    };
    let empty = pick(&settings);
    assert!(
        !empty
            .rows()
            .iter()
            .any(|row| matches!(row, PickRow::Provider(_))),
        "还没有目录：一家都不列"
    );
    let custom = empty
        .rows()
        .iter()
        .position(|row| *row == PickRow::Custom)
        .unwrap();
    while pick(&settings).sel < custom {
        press(&mut settings, KeyCode::Char('j'));
    }
    fetched(&mut settings);
    assert_eq!(count.load(Ordering::SeqCst), 1);
    let filled = pick(&settings);
    assert!(
        filled
            .rows()
            .iter()
            .any(|row| matches!(row, PickRow::Provider(p) if p.id == "deepseek")),
        "常用的几家补上了"
    );
    assert_eq!(
        filled.rows()[filled.sel],
        PickRow::Custom,
        "光标还在「自定义…」"
    );
    assert!(
        settings
            .models
            .as_ref()
            .and_then(|m| m.catalog.as_ref())
            .is_some_and(|catalog| !catalog.is_empty())
    );
    assert_eq!(
        status(&settings),
        ("已获取模型目录".to_string(), Tone::Good)
    );
    assert!(settings.changed, "回对话时核心照缓存换上");
}

#[test]
fn update_off_or_tests_built_settings_never_fetch() {
    let scratch = Scratch::new("off");
    let paths = scratch.paths();
    let (url, count) = serve(TRIMMED);
    config(&paths, &url, false);
    let mut settings = settings(&paths);
    settings.show_models();
    assert!(!settings.fetching_catalog(), "关着 update");
    config(&paths, &url, true);
    let mut quiet = Settings::at(paths.clone(), "zh", Outcome::Back).unwrap();
    quiet.show_models();
    assert!(!quiet.fetching_catalog(), "测试照 Settings::at 造的不取");
    assert_eq!(count.load(Ordering::SeqCst), 0);
}

/// 回的不是目录（LM-9补）：说「服务器返回的内容不是模型目录」，中文、英文各一句，不搬解析器的原话。
#[test]
fn a_body_that_is_not_a_catalog_is_said_shortly_in_both_languages() {
    for (language, expected) in [
        (
            "zh",
            "未能获取模型目录：服务器返回的内容不是模型目录。可以选择「自定义…」，手动填写地址和接口",
        ),
        (
            "en",
            "Couldn't fetch the model catalog: the server did not return a model catalog. You can choose “Custom…” and enter the address and API yourself",
        ),
    ] {
        let scratch = Scratch::new(&format!("garbled-{language}"));
        let paths = scratch.paths();
        let (url, count) = serve("<html>not a catalog</html>");
        std::fs::write(
            paths.config_file(),
            format!("[ui]\nlanguage = \"{language}\"\n\n[models.catalog]\nurl = \"{url}\"\n"),
        )
        .unwrap();
        let mut settings = Settings::at(paths.clone(), language, Outcome::Back).unwrap();
        settings.auto_catalog = true;
        settings.show_models();
        fetched(&mut settings);
        assert_eq!(count.load(Ordering::SeqCst), 1);
        assert_eq!(status(&settings), (expected.to_string(), Tone::Bad));
        assert!(
            !paths.model_cache().join("models-dev.json").exists(),
            "不收"
        );
    }
}

/// 光标没动过的（LM-9补）：目录取到、几家刷出来以后光标落在第一家，不停在「更多供应商…」上；开头里接模型那一步、「添加供应商」的
/// 目录窗都是。
#[test]
fn an_untouched_cursor_lands_on_the_first_provider_once_the_catalog_arrives() {
    let scratch = Scratch::new("first");
    let paths = scratch.paths();
    let (url, _) = serve(TRIMMED);
    config(&paths, &url, true);
    let mut settings = settings(&paths);
    settings.start_opening();
    for _ in 0..3 {
        press(&mut settings, KeyCode::Enter);
    }
    let model = &settings.opening.as_ref().unwrap().model;
    assert_eq!(
        model.rows[model.cursor],
        Row::More,
        "没有目录时停在「更多供应商…」"
    );
    fetched(&mut settings);
    let model = &settings.opening.as_ref().unwrap().model;
    let first = model
        .rows
        .iter()
        .position(|row| matches!(row, Row::Provider(p) if p.supported))
        .unwrap();
    assert_eq!(
        model.cursor, first,
        "落在第一家：{:?}",
        model.rows[model.cursor]
    );

    let scratch = Scratch::new("first-page");
    let paths = scratch.paths();
    let (url, _) = serve(TRIMMED);
    config(&paths, &url, true);
    let mut settings = self::settings(&paths);
    settings.show_models();
    press(&mut settings, KeyCode::Char('a'));
    fetched(&mut settings);
    match settings.models.as_ref().and_then(|m| m.popup.as_ref()) {
        Some(Popup::Catalog(pick)) => {
            assert!(
                matches!(pick.rows()[pick.sel], PickRow::Provider(_)),
                "落在第一家：{:?}",
                pick.rows()[pick.sel]
            );
            assert_eq!(
                pick.sel,
                pick.rows().iter().position(PickRow::stops).unwrap()
            );
        }
        other => panic!("目录窗开着：{other:?}"),
    }
}
