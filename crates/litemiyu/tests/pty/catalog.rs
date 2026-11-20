//! 模型目录第一次要用到时才取（LM-9：程序不带目录）：起来时不连网；开会话说第一句话时在后台取一次，存进缓存目录；取到以后
//! 下一轮的底栏照目录写上窗口和花费；无人值守的一次运行（`litemiyu ask`）不取。目录、模型都是本机的假服务器（各一个），供应商
//! 写了 `catalog` 才算目录帮得上（都在本机的不取，`wants_catalog`）。

use std::io::{Read, Write};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use crate::cli::{Home, Input, serve, stream};
use crate::replies::says;
use crate::support::{Options, Session, served};

/// 只有一家 `p`、一个模型 `m1` 的目录：窗口 4000，有价格。
const CATALOG: &str = r#"{"p":{"id":"p","name":"P","models":{"m1":{"limit":{"context":4000,"output":1000},"cost":{"input":1,"output":2}}}}}"#;

/// 假供应商对上目录里的 `p`，目录照 `catalog` 取。
fn config(base_url: &str, catalog: &str) -> String {
    format!(
        "[providers.fake]\ndriver = \"openai-chat\"\nbase_url = \"{base_url}\"\ncatalog = \"p\"\nkey = {{ env = \"LITEMIYU_TEST_KEY\" }}\n\n\
         [providers.fake.models.\"m1\"]\n\n[models]\nchat = \"fake/m1\"\n\n[models.catalog]\nurl = \"{catalog}\"\n"
    )
}

/// 本机的假目录服务器：什么请求都回 `body`（JSON），数着来了几次。交回地址和次数。
fn serve_catalog(body: &'static str) -> (String, Arc<AtomicUsize>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("绑得上本机回环");
    let port = listener.local_addr().expect("有地址").port();
    let count = Arc::new(AtomicUsize::new(0));
    let counted = Arc::clone(&count);
    std::thread::spawn(move || {
        for socket in listener.incoming() {
            let Ok(mut socket) = socket else {
                return;
            };
            let mut got = Vec::new();
            let mut chunk = [0u8; 4096];
            while !got.windows(4).any(|w| w == b"\r\n\r\n") {
                match socket.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => got.extend_from_slice(&chunk[..n]),
                }
            }
            counted.fetch_add(1, Ordering::SeqCst);
            let reply = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            if socket.write_all(reply.as_bytes()).is_err() {}
        }
    });
    (format!("http://127.0.0.1:{port}/api.json"), count)
}

/// 底栏那一行：输入框下边框（`╰`）下面那一行。
fn footer(session: &Session) -> String {
    let screen = session.screen();
    let lines: Vec<&str> = screen.lines().collect();
    lines
        .iter()
        .rposition(|l| l.contains('╰'))
        .and_then(|at| lines.get(at + 1))
        .map(|l| (*l).to_string())
        .unwrap_or_default()
}

#[test]
fn the_catalog_is_fetched_after_the_first_message_and_fills_in_the_footer() {
    let server = served(vec![says(&["第一句的回答"]), says(&["第二句的回答"])]);
    let (url, fetched) = serve_catalog(CATALOG);
    let options = Options {
        env: vec![("LITEMIYU_CATALOG_UPDATE", "true")],
        ..Options::default()
    };
    let session = Session::launch_with(Some(&config(&server.base_url, &url)), &[], &[], &options);
    session.wait_for("❯", "输入框");
    std::thread::sleep(Duration::from_millis(1500));
    assert!(server.requests().is_empty(), "起来时不连网");
    assert_eq!(fetched.load(Ordering::SeqCst), 0, "起来时不取目录");
    let cache = lm_engine::store::paths::Paths::under(session.data()).model_cache();
    session.type_text("头一句话");
    session.press_enter();
    session.wait_for("第一句的回答", "答完");
    let deadline = Instant::now() + Duration::from_secs(30);
    while fetched.load(Ordering::SeqCst) == 0 {
        assert!(Instant::now() < deadline, "说了话以后去取了目录");
        std::thread::sleep(Duration::from_millis(50));
    }
    session.wait_until("目录写进了缓存", |_| {
        cache.join("models-dev.json").is_file()
    });
    session.type_text("第二句话");
    session.press_enter();
    session.wait_for("第二句的回答", "答完");
    session.wait_until("下一轮底栏写上窗口和花费", |s| {
        let line = footer(s);
        line.contains("/4k") && line.contains('$')
    });
    assert_eq!(fetched.load(Ordering::SeqCst), 1, "只取了一次");
}

#[test]
fn asking_once_does_not_fetch_the_catalog() {
    let server = serve(vec![stream(says(&["答"])), stream(says(&["标题"]))]);
    let (url, fetched) = serve_catalog(CATALOG);
    let home = Home::new(None);
    home.write_config(&config(&server.base_url, &url));
    let mut command = home.command(&["ask", "你好"]);
    command.env("LITEMIYU_CATALOG_UPDATE", "true");
    let out = crate::cli::run(&mut command, Input::Null);
    assert_eq!(out.code, Some(0), "{out:?}");
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(fetched.load(Ordering::SeqCst), 0, "没去取目录");
    let cache = lm_engine::store::paths::Paths::under(&home.data).model_cache();
    assert!(!cache.join("models-dev.json").exists(), "没取目录");
}
