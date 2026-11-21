//! 替看不了图的模型看图（LM-36，`session/route/sight.rs`，`docs/00-设计.md` 第六节第 12 条「图片」）：真路由、假服务器。会话的
//! 模型配成只收字，贴一张图说一句：配了 `models.vision` 的，先把图交给它，它的转述记成 `image.described`、跟着那句话交给会话的
//! 模型；没配的、配的那一家不存在的，不发转述的请求，会话的模型收到的是一句带文件名的占位，这一轮照常说完。看图的模型从不退回
//! 会话的模型：会话的模型看不了图才要别人替它看。LM-36 以前路由没有这一段，配了也从来不发给它（照没配办）。

use std::time::Duration;

use lm_engine::http::testkit::{Piece, Reply, Server};
use lm_engine::session::{Attach, AttachSource, Handle};
use lm_kernel::kernel::block::{Block, Text};
use lm_kernel::kernel::event::Body;
use lm_kernel::kernel::session::Command;

use crate::support::routing::{configs, routes};
use crate::support::*;

/// 转述的那一句。
const SEEN: &str = "A grey cat sleeps on a red mat.";

/// 照 OpenAI 兼容的流说一句 `text`。
fn says(text: &str) -> Reply {
    let chunk = serde_json::json!({"choices": [{"index": 0, "delta": {"content": text}}]});
    let done = serde_json::json!({"choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]});
    let body = format!("data: {chunk}\n\ndata: {done}\n\ndata: [DONE]\n\n");
    Reply::stream(vec![Piece::Bytes(body.into_bytes())])
}

/// 一家 `a` 在 `base_url`：会话的模型 `m` 只收字，`v` 收字和图；`vision` 是 `models.vision` 那一行（空的不写）。
fn config(base_url: &str, vision: &str) -> String {
    format!(
        "[providers.a]\ndriver = \"openai-chat\"\nbase_url = \"{base_url}\"\n\n\
         [providers.a.models.\"m\"]\ninputs = [\"text\"]\n\n\
         [providers.a.models.\"v\"]\ninputs = [\"text\", \"image\"]\n\n\
         [models]\nchat = \"a/m\"\n{vision}\n"
    )
}

/// 一张 800 × 600 的 PNG 的开头（量宽高只看它）。
fn png() -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR".to_vec();
    bytes.extend_from_slice(&800u32.to_be_bytes());
    bytes.extend_from_slice(&600u32.to_be_bytes());
    bytes.extend_from_slice(&[8, 6, 0, 0, 0, 0, 0, 0, 0]);
    bytes
}

/// 贴一张图 `cat.png`，说「看看这张」，等这一轮说完。
async fn show(home: &Home, handle: &Handle) {
    let staged = home.scratch.0.join("cat.png");
    std::fs::write(&staged, png()).expect("写得了暂存的图");
    let item = Attach {
        name: "cat.png".to_string(),
        source: AttachSource::File(staged),
        keep: true,
    };
    let mut blocks = within("附图", handle.attach(vec![item]))
        .await
        .expect("会话在跑")
        .expect("附得上")
        .blocks;
    blocks.insert(
        0,
        Block::Text(Text {
            text: "看看这张".to_string(),
        }),
    );
    let mut pushes = watch(handle).await;
    let send = Command::Send {
        blocks,
        urgent: false,
    };
    ask(handle, "cmd-1", send).await.expect("会话在跑");
    until_turn_ends(&mut pushes).await;
}

/// 请求体里的模型名。
fn model(body: &[u8]) -> String {
    let value: serde_json::Value = serde_json::from_slice(body).expect("请求体是 JSON");
    value["model"].as_str().unwrap_or_default().to_string()
}

/// 日志里的转述。
fn described(home: &Home, handle: &Handle) -> Vec<String> {
    home.log(handle.id())
        .into_iter()
        .filter_map(|event| match event.body {
            Body::ImageDescribed(described) => Some(described.text.clone()),
            _ => None,
        })
        .collect()
}

fn routes_plain() -> lm_engine::session::Routes {
    routes(serde_json::json!({}), Duration::from_secs(60))
}

#[tokio::test]
async fn the_vision_model_describes_the_picture_for_a_model_that_cannot_see() {
    let server = Server::start(vec![says(SEEN), says("是一只猫。")]).await;
    let mut home = Home::new();
    home.configs = configs(&config(&server.base_url, "vision = \"a/v\""), &[]);
    let handle = home.create(&routes_plain()).await;
    show(&home, &handle).await;
    let received = server.received();
    assert_eq!(received.len(), 2, "先转述、再问会话的模型");
    assert_eq!(model(&received[0].body), "v", "转述发给看图的模型");
    assert!(
        String::from_utf8_lossy(&received[0].body).contains("image_url"),
        "图交给它看"
    );
    assert_eq!(model(&received[1].body), "m");
    let main = String::from_utf8_lossy(&received[1].body).into_owned();
    assert!(main.contains(SEEN), "会话的模型收到转述：{main}");
    assert!(!main.contains("image_url"), "图本身不发给看不了图的模型");
    assert_eq!(described(&home, &handle), [SEEN]);
    stop(&handle).await;
}

#[tokio::test]
async fn without_a_usable_vision_model_the_picture_becomes_a_placeholder() {
    for vision in ["", "vision = \"nope/v\""] {
        let server = Server::start(vec![says("看不到图。")]).await;
        let mut home = Home::new();
        home.configs = configs(&config(&server.base_url, vision), &[]);
        let handle = home.create(&routes_plain()).await;
        show(&home, &handle).await;
        let received = server.received();
        assert_eq!(
            received.len(),
            1,
            "{vision:?}：不发转述，也不退回会话的模型去看"
        );
        assert_eq!(model(&received[0].body), "m");
        let main = String::from_utf8_lossy(&received[0].body).into_owned();
        assert!(
            main.contains("cat.png"),
            "{vision:?}：换成带文件名的占位：{main}"
        );
        assert!(!main.contains("image_url"));
        assert!(described(&home, &handle).is_empty());
        stop(&handle).await;
    }
}
