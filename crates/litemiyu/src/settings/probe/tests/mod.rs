//! 配置界面里试一家通不通：全连本机现起的假服务器（一个连接一个线程，不连外网）。每一种结果怎么认在引擎的测试里
//! （`lm-engine/src/providers/probe/tests.rs`）；这里测界面那一半：后台试、状态行的一句、`Esc` 停、key 不上屏。列不出模型时
//! 拿配置里的模型发一句的几种在 `fallback.rs`。

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use lm_engine::store::paths::Paths;

use crate::settings::{Outcome as Leave, Settings, Tone, View};

/// 一眼看得出是假的 key。
const FAKE: &str = "sk-FAKE-probe-key-0000-not-real";

/// 怎么回。
#[derive(Clone, Copy)]
enum Reply {
    /// 照原样写这些字节，然后关掉。
    Bytes(&'static [u8]),
    /// 一直不回，等对面关。
    Never,
}

/// 起一个只收一个连接的假服务器：交回地址，和收到的请求头（读完头就交）。
fn serve(reply: Reply) -> (String, mpsc::Receiver<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let (sender, heard) = mpsc::channel();
    std::thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else {
            return;
        };
        let head = read_head(&mut stream);
        sender.send(head).expect("测试拿着收的那一头");
        match reply {
            Reply::Bytes(bytes) => {
                // 对面先关了的（叫停）不用写完。
                if stream.write_all(bytes).is_err() {}
            }
            // 对面停了、超时了会关连接，读到 0 就走。
            Reply::Never => while matches!(stream.read(&mut [0u8; 64]), Ok(n) if n > 0) {},
        }
    });
    (base, heard)
}

/// 读到空行为止。
fn read_head(stream: &mut TcpStream) -> String {
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") && matches!(stream.read(&mut byte), Ok(1)) {
        head.push(byte[0]);
    }
    String::from_utf8_lossy(&head).into_owned()
}

/// 配置界面里按 `r`：状态行先写正在试（界面接着能按键），收到结果写一句；`Esc` 停掉。
#[test]
fn the_settings_page_tests_in_the_background_and_esc_stops_it() {
    let dir = std::env::temp_dir().join(format!("litemiyu-probe-{}", std::process::id()));
    let paths = Paths::under(&dir);
    std::fs::create_dir_all(&paths.config).unwrap();
    let (base, _heard) = serve(Reply::Bytes(
        b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\n\r\n",
    ));
    std::fs::write(
        paths.config_file(),
        format!("[providers.relay]\ndriver = \"openai-chat\"\nbase_url = \"{base}\"\nkey = {{ secret = \"relay\" }}\n"),
    )
    .unwrap();
    std::fs::write(paths.secrets_file(), format!("relay = \"{FAKE}\"\n")).unwrap();
    let mut settings = Settings::at(paths.clone(), "zh", Leave::Back).unwrap();
    settings.show_models();
    let key = |code| KeyEvent::new(code, KeyModifiers::NONE);
    settings.key(key(KeyCode::Char('r')));
    assert_eq!(
        settings.status.clone().unwrap(),
        ("正在测试 relay…".to_string(), Tone::Busy)
    );
    // 试着的时候照样能挪光标：切到模型栏。
    settings.key(key(KeyCode::Char('l')));
    assert_eq!(settings.models.as_ref().map(|m| m.nav.col), Some(1));
    let deadline = Instant::now() + Duration::from_secs(10);
    while !settings.poll_probe() {
        assert!(Instant::now() < deadline, "十秒没试完");
        std::thread::sleep(Duration::from_millis(5));
    }
    let (said, tone) = settings.status.clone().unwrap();
    assert_eq!(said, "relay 连接失败：密钥无效或没有权限（HTTP 401）");
    assert_eq!(tone, Tone::Bad);
    // 一直不回的：Esc 停，状态行说停了，留在供应商页。
    let (silent, _heard) = serve(Reply::Never);
    std::fs::write(
        paths.config_file(),
        format!("[providers.relay]\ndriver = \"openai-chat\"\nbase_url = \"{silent}\"\n"),
    )
    .unwrap();
    let mut settings = Settings::at(paths, "zh", Leave::Back).unwrap();
    settings.show_models();
    settings.key(key(KeyCode::Char('r')));
    assert!(settings.probing.is_some());
    assert_eq!(settings.key(key(KeyCode::Esc)), Leave::Stay);
    assert!(settings.probing.is_none());
    assert_eq!(settings.status.clone().unwrap().0, "已停止测试 relay");
    assert!(
        matches!(settings.view, View::Page { .. }),
        "Esc 停的是试，不是回主菜单"
    );
    let shown = format!("{:?}", settings.status);
    assert!(!shown.contains(FAKE));
    if std::fs::remove_dir_all(&dir).is_err() {}
}

mod fallback;
