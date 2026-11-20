//! 列不出模型时发一句（Miyu `provider.test` 第 4、5 步）：本机假服务器，第一个连接是列模型（回 404），第二个是那一句话。
//! 对话通了、对话 401、对话一直不回（到时限、`Esc` 停）；配置里一个模型都没写、目录里也认不出的照旧只说列模型的错。
//! key 进了请求头，不进状态行。

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use lm_engine::store::paths::Paths;

use super::{FAKE, read_head};
use crate::settings::{Outcome as Leave, Settings, Tone};

// 假服务器一个连接只回一份、回完就关（`serve`）：回的头里照实说 `Connection: close`。不说的话客户端把连接留在池子里，下一句
// 拿它发，赶上这边已经关了就是「Connection reset by peer」（整套一起跑、机器忙时红过：列模型的 404 以后发那一句被重置）。
const NOT_FOUND: &[u8] =
    b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
const DENIED: &[u8] = b"HTTP/1.1 401 Unauthorized\r\nContent-Type: application/json\r\nContent-Length: 40\r\nConnection: close\r\n\r\n{\"error\":{\"message\":\"invalid api key\"}}";
const SAID_OK: &[u8] = b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\ndata: {\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"OK\"},\"finish_reason\":null}]}\n\ndata: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\ndata: [DONE]\n\n";

/// 照先后一个连接回一份：`None` 的一直不回，等对面关。交回地址，和每个连接收到的请求（头和体）。
fn serve(replies: Vec<Option<&'static [u8]>>) -> (String, mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let (sender, heard) = mpsc::channel();
    std::thread::spawn(move || {
        for reply in replies {
            let Ok((mut stream, _)) = listener.accept() else {
                return;
            };
            let head = read_head(&mut stream);
            let length = head
                .lines()
                .find_map(|l| {
                    let (name, value) = l.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().ok())?
                })
                .unwrap_or(0);
            let mut body = vec![0u8; length];
            if stream.read_exact(&mut body).is_err() {
                return;
            }
            let heard_now = format!("{head}{}", String::from_utf8_lossy(&body));
            if sender.send(heard_now).is_err() {
                return;
            }
            match reply {
                Some(bytes) => if stream.write_all(bytes).is_err() {},
                None => while matches!(stream.read(&mut [0u8; 64]), Ok(n) if n > 0) {},
            }
        }
    });
    (base, heard)
}

/// 临时的数据目录：配置里一家 `relay`，key 是假的，写了模型 `models` 这几个，主对话指到它的 `chat`。
struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(name: &str, base: &str, models: &[&str], chat: Option<&str>) -> (Scratch, Paths) {
        let dir =
            std::env::temp_dir().join(format!("litemiyu-fallback-{}-{name}", std::process::id()));
        let paths = Paths::under(&dir);
        std::fs::create_dir_all(&paths.config).unwrap();
        let mut config = format!(
            "[providers.relay]\ndriver = \"openai-chat\"\nbase_url = \"{base}\"\nkey = {{ secret = \"relay\" }}\n"
        );
        for model in models {
            config.push_str(&format!(
                "\n[providers.relay.models.\"{model}\"]\nwindow = 128000\n"
            ));
        }
        if let Some(chat) = chat {
            config.push_str(&format!("\n[models]\nchat = \"relay/{chat}\"\n"));
        }
        std::fs::write(paths.config_file(), config).unwrap();
        std::fs::write(paths.secrets_file(), format!("relay = \"{FAKE}\"\n")).unwrap();
        (Scratch(dir), paths)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if std::fs::remove_dir_all(&self.0).is_err() {}
    }
}

/// 供应商页上按 `r`，等到试完，交回状态行。
fn tried(paths: &Paths, wait: Duration) -> (String, Tone) {
    let mut settings = Settings::at(paths.clone(), "zh", Leave::Back).unwrap();
    settings.wait = wait;
    settings.show_models();
    settings.key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE));
    let deadline = Instant::now() + Duration::from_secs(20);
    while !settings.poll_probe() {
        assert!(Instant::now() < deadline, "二十秒没试完");
        std::thread::sleep(Duration::from_millis(5));
    }
    let status = settings.status.clone().unwrap();
    assert!(!format!("{status:?}").contains(FAKE), "状态行里没有 key");
    status
}

#[test]
fn no_model_list_but_the_chat_answers_counts_as_working() {
    // 配置里只有主对话指到 `relay/m1`（模型那一节什么都没写）：拿它试。
    let (base, heard) = serve(vec![Some(NOT_FOUND), Some(SAID_OK)]);
    let (_scratch, paths) = Scratch::new("works", &base, &[], Some("m1"));
    assert_eq!(
        tried(&paths, Duration::from_secs(10)),
        ("relay 连接成功：m1".to_string(), Tone::Good)
    );
    let listing = heard.recv().unwrap();
    assert!(listing.starts_with("GET /v1/models"), "{listing}");
    let chat = heard.recv().unwrap();
    assert!(chat.starts_with("POST /v1/chat/completions"), "{chat}");
    assert!(
        chat.contains("Reply with OK."),
        "发的是最小的那一句：{chat}"
    );
    assert!(chat.contains(r#""model":"m1""#), "{chat}");
    let wanted = format!("authorization: bearer {}", FAKE.to_ascii_lowercase());
    assert!(
        chat.to_ascii_lowercase().contains(&wanted),
        "key 进了请求头"
    );
}

#[test]
fn no_model_list_and_the_chat_says_unauthorized_is_a_wrong_key() {
    let (base, _heard) = serve(vec![Some(NOT_FOUND), Some(DENIED)]);
    let (_scratch, paths) = Scratch::new("denied", &base, &["m1"], None);
    assert_eq!(
        tried(&paths, Duration::from_secs(10)),
        (
            "relay 连接失败：密钥无效或没有权限（HTTP 401）".to_string(),
            Tone::Bad
        )
    );
}

#[test]
fn no_model_list_and_a_silent_chat_times_out_or_is_stopped() {
    let (base, _heard) = serve(vec![Some(NOT_FOUND), None]);
    let (_scratch, paths) = Scratch::new("silent", &base, &["m1"], None);
    let started = Instant::now();
    assert_eq!(
        tried(&paths, Duration::from_millis(500)),
        (
            "relay 连接失败：1 秒内没有收到完整的响应".to_string(),
            Tone::Bad
        )
    );
    assert!(started.elapsed() < Duration::from_secs(10));
    // Esc 停：等到那一句发出去了（假服务器收到了）再停，状态行说停了。
    let (base, heard) = serve(vec![Some(NOT_FOUND), None]);
    let (_scratch, paths) = Scratch::new("stopped", &base, &["m1"], None);
    let mut settings = Settings::at(paths, "zh", Leave::Back).unwrap();
    settings.show_models();
    settings.key(KeyEvent::new(KeyCode::Char('r'), KeyModifiers::NONE));
    heard.recv().unwrap();
    let chat = heard.recv().unwrap();
    assert!(chat.starts_with("POST "), "{chat}");
    assert_eq!(
        settings.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
        Leave::Stay
    );
    assert!(settings.probing.is_none());
    assert_eq!(settings.status.clone().unwrap().0, "已停止测试 relay");
}

#[test]
fn with_no_model_to_try_the_listing_error_is_told() {
    // 配置里一个模型都没写、目录里也认不出这一家：挑不出模型，照旧说列模型是哪一种错，不发那一句。
    let (base, heard) = serve(vec![Some(NOT_FOUND)]);
    let (_scratch, paths) = Scratch::new("nomodel", &base, &[], None);
    let (said, tone) = tried(&paths, Duration::from_secs(10));
    assert!(said.contains("HTTP 404"), "{said}");
    assert_eq!(tone, Tone::Bad);
    assert_eq!(heard.try_iter().count(), 1, "只列了模型，没发话");
}

/// 配置里这一家一个模型都没写、`models.chat` 也没指到它，可编号是目录认得的一家（`deepseek`）：照目录挑一个模型发那一句
/// （Miyu `provider.test` 第 4 步，界面线第六趟留下没测的那一路）。挑的是哪个模型照目录，这里只认请求里的和状态行里的
/// 是同一个、不是空的。
#[test]
fn no_model_written_picks_one_from_the_catalog_for_a_known_provider() {
    let (base, heard) = serve(vec![Some(NOT_FOUND), Some(SAID_OK)]);
    let dir =
        std::env::temp_dir().join(format!("litemiyu-fallback-{}-catalog", std::process::id()));
    let _scratch = Scratch(dir.clone());
    let paths = Paths::under(&dir);
    std::fs::create_dir_all(&paths.config).unwrap();
    crate::settings::put_catalog(&paths);
    std::fs::write(
        paths.config_file(),
        format!(
            "[providers.deepseek]\ndriver = \"openai-chat\"\nbase_url = \"{base}\"\nkey = {{ secret = \"deepseek\" }}\n"
        ),
    )
    .unwrap();
    std::fs::write(paths.secrets_file(), format!("deepseek = \"{FAKE}\"\n")).unwrap();
    let (status, tone) = tried(&paths, Duration::from_secs(10));
    assert_eq!(tone, Tone::Good, "{status}");
    let listing = heard.recv().unwrap();
    assert!(listing.starts_with("GET /v1/models"), "{listing}");
    let chat = heard.recv().unwrap();
    assert!(
        chat.contains("Reply with OK."),
        "发的是最小的那一句：{chat}"
    );
    let model = chat
        .split(r#""model":""#)
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .expect("请求里写了模型");
    assert!(!model.is_empty(), "{chat}");
    assert_eq!(status, format!("deepseek 连接成功：{model}"));
}

// —— 点名了模型的（配置界面焦点在模型上、开头里选好了主对话的模型）：列得出也一定拿它发一句，主结论是它能不能用（项目主人
// 2026-11-14 定，和 Miyu 不一样）。

const LISTED: &[u8] = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 34\r\nConnection: close\r\n\r\n{\"data\":[{\"id\":\"m1\"},{\"id\":\"m2\"}]}";
const NO_SUCH_MODEL: &[u8] = b"HTTP/1.1 404 Not Found\r\nContent-Type: application/json\r\nContent-Length: 42\r\nConnection: close\r\n\r\n{\"error\":{\"message\":\"model not found: m1\"}}";

/// 点名 `model` 试 `relay`，等到试完，交回状态行；测着的时候界面照常收键（`j` 不卡）。
fn tried_model(paths: &Paths, model: &str) -> (String, Tone) {
    let mut settings = Settings::at(paths.clone(), "zh", Leave::Back).unwrap();
    settings.wait = Duration::from_secs(10);
    settings.show_models();
    settings.probe("relay", Some(model.to_string()));
    assert!(settings.probing.is_some(), "在后台试，界面不等");
    settings.key(KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE));
    let deadline = Instant::now() + Duration::from_secs(20);
    while !settings.poll_probe() {
        assert!(Instant::now() < deadline, "二十秒没试完");
        std::thread::sleep(Duration::from_millis(5));
    }
    let status = settings.status.clone().unwrap();
    assert!(!format!("{status:?}").contains(FAKE), "状态行里没有 key");
    status
}

#[test]
fn a_chosen_model_that_answers_404_or_401_is_not_usable_even_when_models_list() {
    for (reply, why) in [(NO_SUCH_MODEL, "HTTP 404"), (DENIED, "HTTP 401")] {
        let (base, heard) = serve(vec![Some(LISTED), Some(reply)]);
        let (_scratch, paths) = Scratch::new("chosen-bad", &base, &[], Some("m1"));
        let (said, tone) = tried_model(&paths, "m1");
        assert_eq!(tone, Tone::Bad, "{said}");
        assert!(said.starts_with("relay：m1 不可用："), "{said}");
        assert!(said.contains(why), "{said}");
        assert!(!said.contains("已获取"), "列出几个模型不冒充可用：{said}");
        assert!(heard.recv().unwrap().starts_with("GET /v1/models"));
        let chat = heard.recv().unwrap();
        assert!(chat.starts_with("POST /v1/chat/completions"), "{chat}");
        assert!(chat.contains("\"model\":\"m1\""), "{chat}");
        assert!(!chat.contains("\"tools\""), "不带工具：{chat}");
        assert!(
            chat.contains("\"max_tokens\":16") || chat.contains("\"max_completion_tokens\":16"),
            "最小的输出上限：{chat}"
        );
    }
}

#[test]
fn a_chosen_model_that_answers_is_usable_and_the_listing_is_told_apart() {
    let (base, heard) = serve(vec![Some(LISTED), Some(SAID_OK)]);
    let (_scratch, paths) = Scratch::new("chosen-ok", &base, &[], Some("m1"));
    let (said, tone) = tried_model(&paths, "m1");
    assert_eq!(tone, Tone::Good, "{said}");
    assert!(said.starts_with("relay：m1 可用，响应用时 "), "{said}");
    assert!(said.ends_with(" 秒 · 列出 2 个模型"), "{said}");
    assert!(heard.recv().unwrap().starts_with("GET /v1/models"));
    assert!(
        heard
            .recv()
            .unwrap()
            .starts_with("POST /v1/chat/completions")
    );
}

#[test]
fn testing_a_chosen_model_can_be_stopped_with_esc() {
    let (base, heard) = serve(vec![Some(LISTED), None]);
    let (_scratch, paths) = Scratch::new("chosen-stop", &base, &[], Some("m1"));
    let mut settings = Settings::at(paths.clone(), "zh", Leave::Back).unwrap();
    settings.wait = Duration::from_secs(600);
    settings.show_models();
    settings.probe("relay", Some("m1".to_string()));
    assert!(heard.recv().unwrap().starts_with("GET /v1/models"));
    assert!(
        heard.recv().unwrap().starts_with("POST"),
        "那一句发出去了，一直不回"
    );
    settings.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(settings.probing.is_none());
    assert_eq!(
        settings.status.clone().map(|(_, tone)| tone),
        Some(Tone::Bad)
    );
}
