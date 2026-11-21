//! 第一次运行的开头（配置 六：照 Miyu 引导重写）：几步的先后、`Esc`、语言当场换字、操作方式和图标写进配置、接模型那一步
//! （自定义的一家照填的试、选模型、排给写线程存、存完才往下走；试不通的「仍然保存」；点名模型的真发一句）、配色方案、完成那一屏。
//! 假服务器全在本机（`127.0.0.1`），key 是假的。

use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::{Duration, Instant};

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use lm_engine::store::paths::Paths;

use super::{Phase, Row, Slot, Step, ThemeSlot};
use crate::settings::{Outcome, Settings};

/// 一眼看得出是假的 key。
const FAKE: &str = "sk-FAKE-opening-0000-not-real";

/// 临时的数据目录，用完删掉。
struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir =
            std::env::temp_dir().join(format!("litemiyu-opening-{}-{name}", std::process::id()));
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

/// 配好了一家、主对话的模型是它。
const CONFIGURED: &str = "[providers.fake]\ndriver = \"openai-chat\"\nbase_url = \"http://127.0.0.1:9/v1\"\n\n[models]\nchat = \"fake/m1\"\n";

fn key(settings: &mut Settings, key: KeyEvent) -> Outcome {
    let outcome = settings.handle(&Event::Key(key));
    // 写盘在写线程里：按完等排着的写完，下面照盘上的查。
    settings.flush_writes();
    outcome
}

fn press(settings: &mut Settings, code: KeyCode) -> Outcome {
    key(settings, KeyEvent::new(code, KeyModifiers::NONE))
}

fn typed(settings: &mut Settings, text: &str) {
    for c in text.chars() {
        press(settings, KeyCode::Char(c));
    }
}

fn opened(scratch: &Scratch, config: Option<&str>) -> (Settings, Paths) {
    let paths = scratch.paths();
    // 界面语言钉成中文：不写的话是「跟随系统」，过语言那一步时照跑测试那台机器的 `LANG` 换字，英文环境下面的断言全红
    // （2026-11-19 头一趟 CI 红在这）。
    let config = format!("{}\n[ui]\nlanguage = \"zh\"\n", config.unwrap_or_default());
    std::fs::write(paths.config_file(), config).unwrap();
    let mut settings = Settings::at(paths.clone(), "zh", Outcome::Back).unwrap();
    settings.start_opening();
    (settings, paths)
}

fn step(settings: &Settings) -> Step {
    settings.opening.as_ref().unwrap().step
}

fn phase(settings: &Settings) -> &Phase {
    &settings.opening.as_ref().unwrap().model.phase
}

/// 起一个本机的假服务器：每个连接读完请求、照 `reply(请求头)` 回一份就关。交回 `http://…/v1`。
fn serve(reply: fn(&str) -> String) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else {
                return;
            };
            let mut head = Vec::new();
            let mut byte = [0u8; 1];
            while !head.ends_with(b"\r\n\r\n") && matches!(stream.read(&mut byte), Ok(1)) {
                head.push(byte[0]);
            }
            let head = String::from_utf8_lossy(&head).into_owned();
            if stream.write_all(reply(&head).as_bytes()).is_err() {}
        }
    });
    base
}

fn http(status: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}

/// 等在试的那一家试完（最多十秒，照节拍收，同主循环）。
fn tested(settings: &mut Settings) {
    let until = Instant::now() + Duration::from_secs(10);
    while matches!(phase(settings), Phase::Testing(_)) {
        assert!(Instant::now() < until, "十秒没试完");
        settings.poll_probe();
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// 从语言一步走到接模型那一步（三下回车：跟随系统、Vim、图标）。
fn to_model(settings: &mut Settings) {
    for _ in 0..3 {
        press(settings, KeyCode::Enter);
    }
    assert_eq!(step(settings), Step::Model);
}

/// 照 Miyu 的先后：语言、操作方式、图标、模型、配色方案、完成；第一步 `Esc` 退出，记号留着。
#[test]
fn the_steps_go_in_miyus_order_and_esc_on_the_first_quits_leaving_the_mark() {
    let scratch = Scratch::new("order");
    let (mut settings, paths) = opened(&scratch, None);
    let opening = settings.opening.as_ref().unwrap();
    assert_eq!(opening.step, Step::Language);
    assert_eq!(opening.language.rows.len(), 3);
    assert_eq!(
        opening.language.rows[1..],
        ["中文".to_string(), "English".to_string()]
    );
    assert!(opening.language.rows[0].starts_with("跟随系统（"));
    assert_eq!(opening.keys.selected, 1, "操作方式出厂停在 Vim");
    assert!(super::super::startup::opening_mark(&paths).exists());
    assert_eq!(press(&mut settings, KeyCode::Esc), Outcome::Quit);
    assert!(super::super::startup::opening_mark(&paths).exists());
}

/// 语言一步选 English：写进配置，开头的字当场换成英文；操作方式、图标照选的写。
#[test]
fn english_switches_the_words_at_once_and_keys_and_icons_are_written() {
    let scratch = Scratch::new("language");
    let (mut settings, paths) = opened(&scratch, None);
    press(&mut settings, KeyCode::Down);
    press(&mut settings, KeyCode::Down);
    press(&mut settings, KeyCode::Enter);
    assert_eq!(step(&settings), Step::Keys);
    assert_eq!(
        settings.opening_texts().controls.title,
        "How do you like to move around?"
    );
    assert_eq!(settings.switched.as_deref(), Some("en"));
    press(&mut settings, KeyCode::Char('k'));
    press(&mut settings, KeyCode::Enter);
    assert_eq!(step(&settings), Step::Icons);
    press(&mut settings, KeyCode::Char('j'));
    press(&mut settings, KeyCode::Enter);
    assert_eq!(step(&settings), Step::Model);
    let config = std::fs::read_to_string(paths.config_file()).unwrap();
    assert!(config.contains("language = \"en\""), "{config}");
    assert!(config.contains("keys = \"arrows\""), "{config}");
    assert!(config.contains("icons = \"plain\""), "{config}");
    // Esc 一步步回去，回到语言一步再 Esc 退出。
    press(&mut settings, KeyCode::Esc);
    assert_eq!(step(&settings), Step::Icons);
    press(&mut settings, KeyCode::Esc);
    press(&mut settings, KeyCode::Esc);
    assert_eq!(step(&settings), Step::Language);
}

/// 已经有主对话模型：「已配置：fake/m1」「使用」「更换」；使用到配色方案，更换回选一家。
#[test]
fn a_configured_chat_model_asks_use_it_or_choose_another() {
    let scratch = Scratch::new("ready");
    let (mut settings, _paths) = opened(&scratch, Some(CONFIGURED));
    to_model(&mut settings);
    assert!(matches!(phase(&settings), Phase::Ready(true)));
    assert_eq!(
        settings.opening.as_ref().unwrap().model.chat.as_deref(),
        Some("fake/m1")
    );
    press(&mut settings, KeyCode::Enter);
    assert_eq!(step(&settings), Step::Theme);
    press(&mut settings, KeyCode::Esc);
    assert_eq!(step(&settings), Step::Model);
    press(&mut settings, KeyCode::Down);
    press(&mut settings, KeyCode::Enter);
    assert!(matches!(phase(&settings), Phase::List));
    let rows = &settings.opening.as_ref().unwrap().model.rows;
    assert!(rows.contains(&Row::More) && rows.contains(&Row::Custom));
    assert!(!rows.contains(&Row::Import), "机器上没有 Miyu 不列导入");
    press(&mut settings, KeyCode::Esc);
    assert!(
        matches!(phase(&settings), Phase::Ready(true)),
        "有主对话模型的 Esc 回「已配置」"
    );
}

/// 到「自定义…」填一家：选一家那一屏一直往下到底就是它（`Row::Custom`）。
fn custom_form(settings: &mut Settings, base: &str) {
    for _ in 0..40 {
        press(settings, KeyCode::Down);
    }
    press(settings, KeyCode::Enter);
    let Phase::Form(form) = phase(settings) else {
        panic!("{:?}", phase(settings));
    };
    assert_eq!(
        form.slots(),
        [
            Slot::Id,
            Slot::Name,
            Slot::Url,
            Slot::Driver,
            Slot::Key,
            Slot::Test
        ]
    );
    // ID 空着（照地址自动起）、显示名称空着；地址。
    press(settings, KeyCode::Down);
    press(settings, KeyCode::Down);
    press(settings, KeyCode::Enter);
    settings.handle(&Event::Paste(base.to_string()));
    press(settings, KeyCode::Enter);
    // 接口照出厂的 OpenAI 兼容；密钥。
    press(settings, KeyCode::Down);
    press(settings, KeyCode::Down);
    press(settings, KeyCode::Enter);
    typed(settings, FAKE);
    press(settings, KeyCode::Enter);
    press(settings, KeyCode::Down);
}

/// 自定义的一家：试的时候不写盘、key 只进请求头；列出模型选一个，排给写线程存（一家、key 进密钥文件、主对话的模型），存完才
/// 往下走；列到的模型名跟着存下。
#[test]
fn a_custom_provider_is_tested_before_anything_is_written_then_picked_and_saved() {
    let base = serve(|head| {
        assert!(head.contains(&format!("Bearer {FAKE}")), "key 进了请求头");
        http("200 OK", r#"{"data":[{"id":"m-a"},{"id":"m-b"}]}"#)
    });
    let scratch = Scratch::new("custom");
    let (mut settings, paths) = opened(&scratch, None);
    to_model(&mut settings);
    custom_form(&mut settings, &base);
    press(&mut settings, KeyCode::Enter);
    assert!(matches!(phase(&settings), Phase::Testing(_)), "在试");
    tested(&mut settings);
    assert!(
        !std::fs::read_to_string(paths.config_file()).is_ok_and(|c| c.contains("providers")),
        "试的时候不写盘"
    );
    let Phase::Models(models) = phase(&settings) else {
        panic!(
            "{:?} {:?}",
            phase(&settings),
            settings.opening.as_ref().unwrap().model.error
        );
    };
    assert_eq!(models.names, ["m-a", "m-b"]);
    assert_eq!(models.listed, 2);
    press(&mut settings, KeyCode::Down);
    press(&mut settings, KeyCode::Enter);
    assert_eq!(step(&settings), Step::Theme, "写完了往下走");
    let config = std::fs::read_to_string(paths.config_file()).unwrap();
    assert!(config.contains("[providers.local]"), "{config}");
    assert!(
        config.contains(&format!("base_url = \"{base}\"")),
        "{config}"
    );
    assert!(config.contains("key = { secret = \"local\" }"), "{config}");
    assert!(config.contains("chat = \"local/m-b\""), "{config}");
    assert!(!config.contains(FAKE), "明文不进配置");
    let secrets = std::fs::read_to_string(paths.secrets_file()).unwrap();
    assert!(secrets.contains(FAKE));
    let stored = lm_engine::providers::lists::stored(&paths);
    assert_eq!(
        stored.get("local").map(Vec::len),
        Some(2),
        "列到的模型名存下"
    );
    // 配色方案：下一步到完成，摘要只写模型，回车删记号回对话。
    press(&mut settings, KeyCode::Down);
    press(&mut settings, KeyCode::Down);
    assert_eq!(settings.opening.as_ref().unwrap().theme, ThemeSlot::Next);
    press(&mut settings, KeyCode::Enter);
    assert_eq!(step(&settings), Step::Done);
    let opening = settings.opening.take().unwrap();
    assert_eq!(settings.opening_summary(&opening), "模型 local/m-b");
    settings.opening = Some(opening);
    assert_eq!(press(&mut settings, KeyCode::Enter), Outcome::Back);
    assert!(!super::super::startup::opening_mark(&paths).exists());
}

/// 试不通：红字「连接失败（获取模型列表）：…」，多一行「仍然保存」；没模型名的先要一个，填了照填的存。
#[test]
fn a_failed_test_says_why_and_save_anyway_asks_for_a_model_name() {
    let base = serve(|_| http("401 Unauthorized", r#"{"error":{"message":"bad key"}}"#));
    let scratch = Scratch::new("failed");
    let (mut settings, paths) = opened(&scratch, None);
    to_model(&mut settings);
    custom_form(&mut settings, &base);
    press(&mut settings, KeyCode::Enter);
    tested(&mut settings);
    let model = &settings.opening.as_ref().unwrap().model;
    assert_eq!(
        model.error.as_deref(),
        Some("连接失败（获取模型列表）：密钥无效或没有权限（HTTP 401）")
    );
    let Phase::Form(form) = &model.phase else {
        panic!("{:?}", model.phase);
    };
    assert!(form.slots().ends_with(&[Slot::Test, Slot::Anyway]));
    assert_eq!(form.slot(), Some(Slot::Key), "光标回到密钥");
    // 到「仍然保存」：没模型名的多一格模型名、黄字。
    press(&mut settings, KeyCode::Down);
    press(&mut settings, KeyCode::Down);
    press(&mut settings, KeyCode::Enter);
    let model = &settings.opening.as_ref().unwrap().model;
    assert_eq!(model.warn.as_deref(), Some("请填写模型名后再保存"));
    let Phase::Form(form) = &model.phase else {
        panic!();
    };
    assert_eq!(form.slot(), Some(Slot::Model));
    press(&mut settings, KeyCode::Enter);
    typed(&mut settings, "m-typed");
    press(&mut settings, KeyCode::Enter);
    press(&mut settings, KeyCode::Down);
    press(&mut settings, KeyCode::Down);
    press(&mut settings, KeyCode::Enter);
    assert_eq!(step(&settings), Step::Theme);
    let config = std::fs::read_to_string(paths.config_file()).unwrap();
    assert!(config.contains("chat = \"local/m-typed\""), "{config}");
}

/// 列不出模型：多一格模型名（黄字）；填了再试，拿它真发一句（项目主人 2026-11-14），发不通的照原话说是发送请求没通。
#[test]
fn no_list_asks_for_a_model_name_and_the_named_model_is_really_sent_to() {
    let base = serve(|head| {
        if head.starts_with("GET") {
            http("404 Not Found", "{}")
        } else {
            http(
                "404 Not Found",
                r#"{"error":{"message":"model not found"}}"#,
            )
        }
    });
    let scratch = Scratch::new("named");
    let (mut settings, _paths) = opened(&scratch, None);
    to_model(&mut settings);
    custom_form(&mut settings, &base);
    press(&mut settings, KeyCode::Enter);
    tested(&mut settings);
    let model = &settings.opening.as_ref().unwrap().model;
    assert_eq!(
        model.warn.as_deref(),
        Some("未获取到模型列表，请填写模型名后重试")
    );
    let Phase::Form(form) = &model.phase else {
        panic!("{:?}", model.phase);
    };
    assert_eq!(form.slot(), Some(Slot::Model));
    press(&mut settings, KeyCode::Enter);
    typed(&mut settings, "m-named");
    press(&mut settings, KeyCode::Enter);
    press(&mut settings, KeyCode::Down);
    press(&mut settings, KeyCode::Enter);
    tested(&mut settings);
    let model = &settings.opening.as_ref().unwrap().model;
    let error = model.error.clone().unwrap_or_default();
    assert!(error.starts_with("连接失败（发送请求）："), "{error}");
    assert!(error.contains("404"), "{error}");
}

/// 配色方案：铺底色照 Miyu 开「开、关」的选择窗，明暗同；挪着就预览，选好了先记下、照它换样子，到「下一步」才一起存（Miyu
/// `oobe/theme.rs` 的 `save`）。
#[test]
fn the_theme_step_opens_choosers_for_the_mode_and_the_ground() {
    let scratch = Scratch::new("theme");
    let (mut settings, paths) = opened(&scratch, Some(CONFIGURED));
    to_model(&mut settings);
    press(&mut settings, KeyCode::Enter);
    assert_eq!(step(&settings), Step::Theme);
    press(&mut settings, KeyCode::Down);
    press(&mut settings, KeyCode::Enter);
    assert!(settings.popup.is_some(), "铺底色开选择窗");
    press(&mut settings, KeyCode::Down);
    assert_eq!(
        settings.previewing(),
        Some((
            "tui.theme_background".into(),
            lm_kernel::config::Value::Bool(false)
        )),
        "挪着就预览"
    );
    press(&mut settings, KeyCode::Enter);
    let config = std::fs::read_to_string(paths.config_file()).unwrap();
    assert!(
        !config.contains("theme_background"),
        "选好了不写盘：{config}"
    );
    settings.restyle();
    assert_eq!(
        settings.looks("tui.theme_background"),
        Some(lm_kernel::config::Value::Bool(false)),
        "选好了照它换样子"
    );
    press(&mut settings, KeyCode::Up);
    press(&mut settings, KeyCode::Enter);
    press(&mut settings, KeyCode::Down);
    press(&mut settings, KeyCode::Enter);
    let config = std::fs::read_to_string(paths.config_file()).unwrap();
    assert!(!config.contains("theme_mode"), "{config}");
    // 回上一步：照配置的值换回去；再进来选的还在（Miyu 只在这一步照选的）。
    press(&mut settings, KeyCode::Esc);
    assert_eq!(step(&settings), Step::Model);
    settings.restyle();
    assert_ne!(
        settings.looks("tui.theme_background"),
        Some(lm_kernel::config::Value::Bool(false)),
        "回上一步照配置的值"
    );
    press(&mut settings, KeyCode::Enter);
    assert_eq!(step(&settings), Step::Theme);
    settings.restyle();
    assert_eq!(
        settings.looks("tui.theme_mode"),
        Some(lm_kernel::config::Value::Text("dark".into()))
    );
    press(&mut settings, KeyCode::Down);
    press(&mut settings, KeyCode::Down);
    press(&mut settings, KeyCode::Enter);
    assert_eq!(step(&settings), Step::Done, "写完才往下走");
    let config = std::fs::read_to_string(paths.config_file()).unwrap();
    assert!(config.contains("theme_background = false"), "{config}");
    assert!(config.contains("theme_mode = \"dark\""), "{config}");
    assert!(settings.status.is_none(), "配色那一步不说「已保存」");
}

/// 配色方案什么都没选的「下一步」不写盘（配置文件照旧只写人选了的）。
#[test]
fn the_theme_step_writes_nothing_when_nothing_was_picked() {
    let scratch = Scratch::new("theme-none");
    let (mut settings, paths) = opened(&scratch, Some(CONFIGURED));
    to_model(&mut settings);
    press(&mut settings, KeyCode::Enter);
    for _ in 0..2 {
        press(&mut settings, KeyCode::Down);
    }
    press(&mut settings, KeyCode::Enter);
    assert_eq!(step(&settings), Step::Done);
    let config = std::fs::read_to_string(paths.config_file()).unwrap();
    assert!(!config.contains("theme_"), "{config}");
}

/// 开头里 `Ctrl+C` 退出程序（同 Miyu 的引导）。
#[test]
fn ctrl_c_in_the_opening_quits() {
    let scratch = Scratch::new("ctrlc");
    let (mut settings, _paths) = opened(&scratch, None);
    let ctrl = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
    assert_eq!(key(&mut settings, ctrl), Outcome::Quit);
}

/// 列不出模型时要不要人给模型名，逐种照 Miyu 核心 `miyu-session/src/route/probe.rs` 的 `need_model`（配置 七）：列到了一个都没有、
/// 列不到又不是认证错、又没有能拿来发一句的模型（连不上、等到时限的照这一回有没有能发的分）要；认证错、推不出这一家、发了那一句没成的
/// 不要，照没通办。
#[test]
fn a_model_name_is_asked_for_exactly_where_miyu_core_says_need_model() {
    use lm_engine::providers::probe::Outcome;
    let scratch = Scratch::new("need-model");
    let (mut settings, _paths) = opened(&scratch, None);
    let cases = [
        (Outcome::Listed(0), false, true),
        (Outcome::NotFound, false, true),
        (Outcome::Status(500), false, true),
        (Outcome::Garbled, false, true),
        (Outcome::Cut, false, true),
        (Outcome::Other("tls".into()), false, true),
        (Outcome::Connect("refused".into()), false, true),
        (Outcome::Timeout(30), false, true),
        (Outcome::Connect("refused".into()), true, false),
        (Outcome::Timeout(30), true, false),
        (Outcome::Auth(401), false, false),
        (Outcome::Auth(403), true, false),
        (Outcome::Config("no key".into()), false, false),
        (
            Outcome::Request {
                model: "m".into(),
                why: "bad".into(),
            },
            true,
            false,
        ),
        (Outcome::BadUrl, false, false),
    ];
    for (outcome, sendable, asks) in cases {
        let opening = settings.opening.as_mut().unwrap();
        opening.step = Step::Model;
        opening.model.phase = Phase::Testing(super::Form::default());
        opening.model.sendable = sendable;
        opening.model.warn = None;
        assert!(settings.opening_tested(&outcome, &[]), "{outcome:?}");
        let Phase::Form(form) = phase(&settings) else {
            panic!("{outcome:?}：回到填的那一屏，得到 {:?}", phase(&settings));
        };
        assert_eq!(
            form.needs_model, asks,
            "{outcome:?}（有能发的模型：{sendable}）要不要模型名"
        );
        assert_eq!(
            form.failed, !asks,
            "{outcome:?}：要模型名的不算没通，别的多一行「仍然保存」"
        );
    }
}

/// 画出来的一整屏（120×40），一行一行。
fn drawn(settings: &Settings) -> String {
    let mut buffer = ratatui::buffer::Buffer::empty(ratatui::layout::Rect::new(0, 0, 120, 40));
    crate::ui::draw_settings(&mut crate::ui::Frame::new(&mut buffer), settings);
    (0..40)
        .map(|y| {
            // 宽字后面那一格是空的：跳过，同终端上看到的。
            let mut line = String::new();
            let mut skip = false;
            for x in 0..120 {
                let symbol = buffer[(x, y)].symbol();
                if !std::mem::take(&mut skip) {
                    line.push_str(symbol);
                    skip = unicode_width::UnicodeWidthStr::width(symbol) > 1;
                }
            }
            line
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// 第二十六趟（验收时报的）：接模型那一步试通了（「✓ 连接成功，找到 N 个模型」），后台列模型那一句「X 连接成功，已获取
/// N 个模型」不再写在下面（同一个意思；Miyu 引导这一屏只有上面那一句）；出错的照写；回到选一家那一屏照旧写（那一屏没有上面那句）。
#[test]
fn a_connected_model_step_does_not_say_the_background_list_again() {
    let base = serve(|_| http("200 OK", r#"{"data":[{"id":"m-a"},{"id":"m-b"}]}"#));
    let scratch = Scratch::new("said-once");
    let (mut settings, _paths) = opened(&scratch, None);
    to_model(&mut settings);
    custom_form(&mut settings, &base);
    press(&mut settings, KeyCode::Enter);
    tested(&mut settings);
    assert!(matches!(phase(&settings), Phase::Models(_)));
    // 后台列到了（打开开头时拉的、导入完拉的，到得晚的也一样）：状态行记着这一句。
    let listed = "local 连接成功，已获取 2 个模型".to_string();
    settings.status = Some((listed.clone(), crate::settings::Tone::Good));
    let screen = drawn(&settings);
    assert!(screen.contains("✓ 连接成功，找到 2 个模型"), "{screen}");
    assert!(!screen.contains(&listed), "同一个意思不写两遍：\n{screen}");
    settings.status = Some((
        "正在获取 local 的模型列表…".into(),
        crate::settings::Tone::Busy,
    ));
    assert!(!drawn(&settings).contains("正在获取"), "在忙的那一句也不写");
    settings.status = Some(("保存失败：盘满了".into(), crate::settings::Tone::Bad));
    assert!(drawn(&settings).contains("保存失败：盘满了"), "出错的照写");
    // Esc 回选一家那一屏：上面没有连接成功那一句，状态行照写。
    settings.status = Some((listed.clone(), crate::settings::Tone::Good));
    press(&mut settings, KeyCode::Esc);
    assert!(matches!(phase(&settings), Phase::List));
    assert!(drawn(&settings).contains(&listed), "选一家那一屏照写");
}
