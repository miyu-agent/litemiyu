//! 几个请求同时发（子代理和主会话、提前压的旁路请求，瘦身 六）：只留 HTTP/1.1 以后一条连接上一次只走一个请求，同一个
//! 客户端同时发的各开各的连接，慢的不挡快的；都回完以后连接池只留一条闲着的，下一个请求接着用它。
//!
//! 假服务器在这里另写：`testkit` 的那个回完就关连接，数不出连接有没有复用。这一个照 HTTP/1.1 的长连接回，一个连接上
//! 一个接一个地读请求、回，数它接了几个连接、对方关了几个。路径里带 `slow` 的等一会儿才回响应头。

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use lm_engine::http::{Attempt, Client, Endpoint, Outcome, Proxy, client, send};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Notify;

use crate::support::{BODY, driver, sample};

/// 慢的那个多久才回响应头。
const SLOW: Duration = Duration::from_secs(2);

/// 跑着的长连接假服务器。
struct KeepAlive {
    base_url: String,
    /// 接了几个连接。
    accepted: Arc<AtomicUsize>,
    /// 对方关了几个连接。
    closed: Arc<AtomicUsize>,
    /// 收到了一个请求（读完请求体、还没回）就说一声。
    got: Arc<Notify>,
}

impl KeepAlive {
    async fn start() -> KeepAlive {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("绑得上本机回环");
        let port = listener.local_addr().expect("有地址").port();
        let accepted = Arc::new(AtomicUsize::new(0));
        let closed = Arc::new(AtomicUsize::new(0));
        let got = Arc::new(Notify::new());
        let (count, done, heard) = (Arc::clone(&accepted), Arc::clone(&closed), Arc::clone(&got));
        tokio::spawn(async move {
            while let Ok((socket, _)) = listener.accept().await {
                count.fetch_add(1, Ordering::SeqCst);
                let (done, heard) = (Arc::clone(&done), Arc::clone(&heard));
                tokio::spawn(async move {
                    serve(socket, &heard).await;
                    done.fetch_add(1, Ordering::SeqCst);
                });
            }
        });
        KeepAlive {
            base_url: format!("http://127.0.0.1:{port}/v1"),
            accepted,
            closed,
            got,
        }
    }
}

/// 一个连接：一个接一个地读请求、回，直到对方关掉。
async fn serve(mut socket: TcpStream, got: &Notify) {
    let answer = sample("openai-text");
    let mut buffer = Vec::new();
    loop {
        let Some(head_end) = read_until_head(&mut socket, &mut buffer).await else {
            return;
        };
        let head = String::from_utf8_lossy(&buffer[..head_end]).to_lowercase();
        let length = head
            .lines()
            .find_map(|line| line.strip_prefix("content-length:"))
            .and_then(|value| value.trim().parse::<usize>().ok())
            .unwrap_or(0);
        while buffer.len() < head_end + length {
            let mut chunk = [0u8; 8192];
            match socket.read(&mut chunk).await {
                Ok(0) | Err(_) => return,
                Ok(n) => buffer.extend_from_slice(&chunk[..n]),
            }
        }
        let slow = head
            .lines()
            .next()
            .is_some_and(|line| line.contains("slow"));
        buffer.drain(..head_end + length);
        got.notify_one();
        if slow {
            tokio::time::sleep(SLOW).await;
        }
        let reply = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: {}\r\n\r\n",
            answer.len()
        );
        if socket.write_all(reply.as_bytes()).await.is_err()
            || socket.write_all(&answer).await.is_err()
        {
            return;
        }
    }
}

/// 读到请求头的末尾，交回请求头（连同空行）有多长；对方关了的没有。
async fn read_until_head(socket: &mut TcpStream, buffer: &mut Vec<u8>) -> Option<usize> {
    loop {
        if let Some(at) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            return Some(at + 4);
        }
        let mut chunk = [0u8; 8192];
        match socket.read(&mut chunk).await {
            Ok(0) | Err(_) => return None,
            Ok(n) => buffer.extend_from_slice(&chunk[..n]),
        }
    }
}

/// 照 `client` 发一次到 `path`，交回收场和用了多久。
async fn ask(client: &Client, base_url: &str, path: &str) -> (Outcome, Duration) {
    let endpoint = Endpoint::new(base_url, "sk-test");
    let driver = driver();
    let start = Instant::now();
    let outcome = send(
        Attempt {
            client,
            endpoint: &endpoint,
            driver: &driver,
            body: BODY.to_vec().into(),
            path,
            idle: Duration::from_secs(20),
        },
        std::future::pending(),
        |_| {},
    )
    .await;
    (outcome, start.elapsed())
}

fn ok(outcome: &Outcome) -> bool {
    matches!(outcome, Outcome::Ended { error: None, .. })
}

#[tokio::test]
async fn requests_sent_together_each_get_a_connection_and_the_slow_one_holds_up_nobody() {
    let server = KeepAlive::start().await;
    let client = client(Proxy::Off).expect("造得出客户端");
    let slow = {
        let (client, base_url) = (client.clone(), server.base_url.clone());
        tokio::spawn(async move { ask(&client, &base_url, "/slow/chat/completions").await })
    };
    // 慢的那个到了对方手里、还没回，再发两个快的。
    tokio::time::timeout(Duration::from_secs(5), server.got.notified())
        .await
        .expect("慢的那个五秒内到了");
    let (first, second) = tokio::join!(
        ask(&client, &server.base_url, "/chat/completions"),
        ask(&client, &server.base_url, "/chat/completions"),
    );
    for (outcome, took) in [&first, &second] {
        assert!(ok(outcome), "{outcome:?}");
        assert!(*took < SLOW, "快的没被慢的挡住：用了 {took:?}");
    }
    assert!(!slow.is_finished(), "快的回完时慢的还在等");
    assert_eq!(
        server.accepted.load(Ordering::SeqCst),
        3,
        "同时在路上的三个请求各开一个连接"
    );
    let (outcome, took) = slow.await.expect("慢的那个没崩");
    assert!(ok(&outcome), "{outcome:?}");
    assert!(took >= SLOW, "{took:?}");

    // 都回完了：一个地址闲着的只留一条（`http::client` 的 `pool_max_idle_per_host(1)`），多的两条客户端关掉；下一个请求
    // 接着用留下的那一条，不另开。
    let waited = tokio::time::timeout(Duration::from_secs(5), async {
        while server.closed.load(Ordering::SeqCst) < 2 {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await;
    assert!(waited.is_ok(), "闲下来五秒内多的两条关掉了");
    let (outcome, _) = ask(&client, &server.base_url, "/chat/completions").await;
    assert!(ok(&outcome), "{outcome:?}");
    assert_eq!(
        server.accepted.load(Ordering::SeqCst),
        3,
        "接着用留下的那一条"
    );
    assert_eq!(server.closed.load(Ordering::SeqCst), 2);
}
