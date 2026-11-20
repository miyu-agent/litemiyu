//! 试一家通不通，往坏里测：全连本机现起的假服务器（一个连接一个线程，不连外网）。服务器不回（超时、能停）、401、404、乱码、
//! 回一半断开、地址写错、连不上；每一种认成一种结果；key 进了请求头，不进结果。列不出模型时拿配置里的模型发一句的几种、
//! 界面上的那一句在配置界面的测试里（`crates/litemiyu/src/settings/probe/tests/`）。

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use lm_kernel::config::secret::Secret;
use lm_kernel::models::provider::Driver;
use tokio::sync::oneshot;

use super::{Outcome, Target, run};

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

/// 照地址、key 造一家（没有可试的模型）。
fn target(base: &str, key: Option<&str>) -> Target {
    Target {
        driver: Driver::OpenAiChat,
        base_url: base.to_string(),
        key: key.map(|key| Secret::new(key).unwrap()),
        trial: None,
        chosen: false,
    }
}

/// 试一次，不叫停。
fn probe(base: &str, wait: Duration) -> Outcome {
    let (_keep, cancelled) = oneshot::channel();
    let outcome = run(&target(base, Some(FAKE)), wait, cancelled).expect("没叫停");
    assert!(!format!("{outcome:?}").contains(FAKE), "结果里没有 key");
    if let Outcome::Listed(n) = outcome.0 {
        assert_eq!(outcome.1.len(), n, "列出几个交回几个模型名");
    } else {
        assert!(outcome.1.is_empty(), "没列出来的不交模型名");
    }
    outcome.0
}

const LIST: &[u8] = b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 34\r\n\r\n{\"data\":[{\"id\":\"m1\"},{\"id\":\"m2\"}]}";

#[test]
fn a_listing_works_and_the_key_goes_only_into_the_header() {
    let (base, heard) = serve(Reply::Bytes(LIST));
    let (_keep, cancelled) = oneshot::channel();
    let got = run(
        &target(&base, Some(FAKE)),
        Duration::from_secs(10),
        cancelled,
    );
    assert_eq!(
        got,
        Some((Outcome::Listed(2), vec!["m1".to_string(), "m2".to_string()]))
    );
    let head = heard.recv().unwrap();
    assert!(head.starts_with("GET /v1/models HTTP/1.1"), "{head}");
    let wanted = format!("authorization: bearer {}", FAKE.to_ascii_lowercase());
    assert!(head.to_ascii_lowercase().contains(&wanted), "{head}");
}

#[test]
fn each_failure_is_told_apart() {
    let cases: [(&[u8], Outcome); 5] = [
        (
            b"HTTP/1.1 401 Unauthorized\r\nContent-Length: 0\r\n\r\n",
            Outcome::Auth(401),
        ),
        (
            b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n",
            Outcome::NotFound,
        ),
        (
            b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\n\r\n",
            Outcome::Status(502),
        ),
        (
            b"HTTP/1.1 200 OK\r\nContent-Length: 9\r\n\r\n\xff\xfe<html>!",
            Outcome::Garbled,
        ),
        (
            b"HTTP/1.1 200 OK\r\nContent-Length: 1000\r\n\r\n{\"data\":[",
            Outcome::Cut,
        ),
    ];
    for (reply, wanted) in cases {
        let (base, _heard) = serve(Reply::Bytes(reply));
        assert_eq!(probe(&base, Duration::from_secs(10)), wanted);
    }
    // 回的不是 HTTP：认成别的，带原话。
    let (base, _heard) = serve(Reply::Bytes(b"garbage\r\n\r\n"));
    assert!(matches!(
        probe(&base, Duration::from_secs(10)),
        Outcome::Other(_)
    ));
}

#[test]
fn a_wrong_or_dead_address_is_told() {
    assert_eq!(
        probe("127.0.0.1:9/v1", Duration::from_secs(5)),
        Outcome::BadUrl
    );
    assert_eq!(probe("http:///v1", Duration::from_secs(5)), Outcome::BadUrl);
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    match probe(
        &format!("http://127.0.0.1:{port}/v1"),
        Duration::from_secs(5),
    ) {
        Outcome::Connect(why) => assert!(why.contains("refused"), "{why}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_silent_server_times_out_and_can_be_stopped() {
    let (base, _heard) = serve(Reply::Never);
    let started = Instant::now();
    assert_eq!(
        probe(&base, Duration::from_millis(300)),
        Outcome::Timeout(1)
    );
    assert!(started.elapsed() < Duration::from_secs(5));
    // 叫停：不等时限，交回 None。
    let (base, heard) = serve(Reply::Never);
    let (stop, cancelled) = oneshot::channel();
    let thread =
        std::thread::spawn(move || run(&target(&base, None), Duration::from_secs(600), cancelled));
    heard.recv().unwrap();
    stop.send(()).unwrap();
    assert_eq!(thread.join().unwrap(), None);
}
