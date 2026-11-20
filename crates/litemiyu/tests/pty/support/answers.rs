//! 照请求现回的假服务器（从 `support.rs` 挪出来：那一份长过了 500 行）。

use std::io::{Read, Write};

/// 照请求现回的假服务器（几个会话的请求谁先到不一定时用，同 lm-engine 测试的 `Script::answers`）：一个连接一个线程，读完
/// 请求体交给 `answer` 定回哪一份 SSE 的流。一直跑到测试进程退出；请求体不记。
pub fn served_by(answer: fn(&str) -> String) -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("绑得上本机回环");
    let port = listener.local_addr().expect("有地址").port();
    std::thread::spawn(move || {
        for socket in listener.incoming() {
            let Ok(socket) = socket else {
                return;
            };
            std::thread::spawn(move || reply_to(socket, answer));
        }
    });
    format!("http://127.0.0.1:{port}/v1")
}

/// 读一个请求（头里的 `Content-Length` 那么长的体），回 `answer` 定的那一份，关连接。
fn reply_to(mut socket: std::net::TcpStream, answer: fn(&str) -> String) {
    let mut got = Vec::new();
    let mut chunk = [0u8; 8192];
    let (head_end, length) = loop {
        let Ok(n) = socket.read(&mut chunk) else {
            return;
        };
        if n == 0 {
            return;
        }
        got.extend_from_slice(&chunk[..n]);
        if let Some(at) = got.windows(4).position(|w| w == b"\r\n\r\n") {
            let head = String::from_utf8_lossy(&got[..at]).to_ascii_lowercase();
            let length = head
                .lines()
                .find_map(|line| line.strip_prefix("content-length:"))
                .and_then(|v| v.trim().parse::<usize>().ok())
                .unwrap_or(0);
            break (at + 4, length);
        }
    };
    while got.len() < head_end + length {
        match socket.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(n) => got.extend_from_slice(&chunk[..n]),
        }
    }
    let body = String::from_utf8_lossy(&got[head_end..]).into_owned();
    let reply = format!(
        "HTTP/1.1 200 X\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n{}",
        answer(&body)
    );
    if socket.write_all(reply.as_bytes()).is_err() {}
}
