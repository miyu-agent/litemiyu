//! 闲下来收成槽、从日志读回来（红线 5，`session/actor/shelf.rs`）：载入完当场收成槽，人再说话时从日志读回来，请求里一段不少；
//! 读回失败（日志的一行坏了、读盘出错、被截短）这一轮一个请求都不发，照「没发出去」收场，原话写明读不回、为什么，会话照旧跑；
//! 日志好了，下一句照常，请求里连读不回的那一轮人说的话也在。

use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use lm_engine::session::Pushed;
use lm_engine::session::testkit::{Play, Script};
use lm_engine::tool::testkit::{Act, Fake};
use lm_engine::tool::{Catalog, Tool};
use lm_kernel::kernel::block::Block;
use lm_kernel::kernel::event::{Body, ModelCalled};
use lm_kernel::kernel::id::SessionId;
use lm_kernel::kernel::request::{Message, Request};
use lm_kernel::kernel::tool::Access;

use crate::support::*;

/// 一件 `echo`：参数原样回。
fn tools() -> Catalog {
    let echo = Fake::new("echo", Access::Read, Act::Echo);
    Catalog::new([echo as Arc<dyn Tool>]).expect("合写法")
}

/// 造一个会话说一轮（调一次 `echo` 再说完），停下，再载入：载入完当场收成了槽。交回载入的、订阅、会话编号。
async fn loaded(
    home: &Home,
    script: &Script,
) -> (
    lm_engine::session::Handle,
    lm_engine::session::Subscription,
    SessionId,
) {
    let tools = tools();
    let made = home.create_with(script, &tools).await;
    let mut pushes = watch(&made).await;
    ask(&made, "cmd-1", say("第一句")).await.expect("会话在跑");
    until_turn_ends(&mut pushes).await;
    let id = made.id().clone();
    stop(&made).await;
    let handle = home.load_with(&id, script, &tools).await;
    let pushes = watch(&handle).await;
    (handle, pushes, id)
}

/// 会话唯一的那一段日志。
fn segment(home: &Home, id: &SessionId) -> PathBuf {
    let dir = home.root.session(id);
    let mut segments: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("会话目录读得了")
        .map(|entry| entry.expect("读得了").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "jsonl"))
        .collect();
    assert_eq!(segments.len(), 1, "{segments:?}");
    segments.remove(0)
}

/// 推送里这一轮的 `model.called`。
fn called(pushed: &[Arc<Pushed>]) -> Vec<ModelCalled> {
    pushed
        .iter()
        .filter_map(|pushed| match &**pushed {
            Pushed::Events(events) => Some(events.iter()),
            Pushed::Transient(_) => None,
        })
        .flatten()
        .filter_map(|event| match &event.body {
            Body::ModelCalled(called) => Some((**called).clone()),
            _ => None,
        })
        .collect()
}

/// 这一轮读不回来：一个请求都没发（剧本没收到新的），记下的 `model.called` 没发出去、原话写明读不回，`because` 在里面。
fn refused(pushed: &[Arc<Pushed>], script: &Script, before: usize, because: &str) {
    assert_eq!(script.requests().len(), before, "读不回来就不发请求");
    let called = called(pushed);
    assert_eq!(called.len(), 1, "{called:?}");
    let error = called[0].error.as_ref().expect("出错收场");
    assert!(called[0].endpoint.is_none(), "没发出去：{called:?}");
    assert!(
        error.message.contains("从日志读不回来") && error.message.contains(because),
        "{}",
        error.message
    );
}

/// 请求里人说的话和工具结果的字，照先后。
fn said(request: &Request) -> Vec<String> {
    request
        .messages
        .iter()
        .filter_map(|message| match message {
            Message::User { blocks } | Message::Tool { blocks, .. } => Some(blocks.iter()),
            Message::Assistant { .. } => None,
        })
        .flatten()
        .filter_map(|block| match block {
            Block::Text(text) => Some(text.text.clone()),
            _ => None,
        })
        .collect()
}

/// 下一句照常：请求里第一轮人说的、工具结果、读不回的那一轮人说的、这一句都在，照先后。
async fn recovers(
    handle: &lm_engine::session::Handle,
    pushes: &mut lm_engine::session::Subscription,
    script: &Script,
) {
    let before = script.requests().len();
    ask(handle, "cmd-3", say("第三句")).await.expect("会话在跑");
    until_turn_ends(pushes).await;
    let requests = script.requests();
    assert_eq!(requests.len(), before + 1, "这一次发了");
    let said = said(&requests[before].1);
    let at = |needle: &str| {
        said.iter()
            .position(|text| text.contains(needle))
            .unwrap_or_else(|| panic!("请求里没有「{needle}」：{said:?}"))
    };
    assert!(at("第一句") < at("look") && at("look") < at("第二句") && at("第二句") < at("第三句"));
}

/// 剧本：第一轮调一次 `echo` 再说完；第三句说一句。读不回的那一轮不该用到剧本。
fn script() -> Script {
    Script::new([
        Play::calls(&[("echo", r#"{"look":"here"}"#)]),
        Play::Says("好。"),
        Play::Says("第三轮。"),
    ])
}

#[tokio::test]
async fn a_loaded_session_reads_its_history_back_when_spoken_to() {
    let home = Home::new();
    let script = script();
    let (handle, mut pushes, _) = loaded(&home, &script).await;
    ask(&handle, "cmd-2", say("第二句"))
        .await
        .expect("会话在跑");
    until_turn_ends(&mut pushes).await;
    let requests = script.requests();
    assert_eq!(requests.len(), 3);
    let said = said(&requests[2].1);
    assert!(said.iter().any(|text| text.contains("第一句")), "{said:?}");
    assert!(said.iter().any(|text| text.contains("look")), "{said:?}");
    stop(&handle).await;
}

#[tokio::test]
async fn a_broken_line_sends_nothing_says_why_and_the_next_one_goes_through() {
    let home = Home::new();
    let script = Script::new([
        Play::calls(&[("echo", r#"{"look":"here"}"#)]),
        Play::Says("好。"),
        Play::Says("第三轮。"),
    ]);
    let (handle, mut pushes, id) = loaded(&home, &script).await;
    let path = segment(&home, &id);
    let original = std::fs::read(&path).expect("读得了");
    // 第二行（人说的第一句）换成一样长的乱码：偏移不变，那一行读不成。
    let line = original
        .split(|byte| *byte == b'\n')
        .nth(1)
        .expect("有第二行")
        .len();
    let start = original
        .iter()
        .position(|byte| *byte == b'\n')
        .expect("有换行")
        + 1;
    let mut broken = original.clone();
    broken[start..start + line].fill(b'#');
    overwrite(&path, &broken, 0);
    ask(&handle, "cmd-2", say("第二句"))
        .await
        .expect("会话在跑");
    let pushed = until_turn_ends(&mut pushes).await;
    refused(&pushed, &script, 2, "line 2 is broken");
    // 日志修好（只改回那一行，后来追加的照留），下一句照常。
    overwrite(&path, &original[start..start + line], start);
    recovers(&handle, &mut pushes, &script).await;
    stop(&handle).await;
}

#[tokio::test]
async fn an_unreadable_log_sends_nothing_says_why_and_the_next_one_goes_through() {
    let home = Home::new();
    // root 读得了 0o000 的文件，这一条在 root 下不成立（同 `fs/find/tests.rs`）。
    if std::fs::metadata(&home.scratch.0).expect("有").uid() == 0 {
        return;
    }
    let script = script();
    let (handle, mut pushes, id) = loaded(&home, &script).await;
    let path = segment(&home, &id);
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o000)).expect("改得了权限");
    ask(&handle, "cmd-2", say("第二句"))
        .await
        .expect("会话在跑");
    let pushed = until_turn_ends(&mut pushes).await;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).expect("改得回来");
    refused(&pushed, &script, 2, "Permission denied");
    recovers(&handle, &mut pushes, &script).await;
    stop(&handle).await;
}

#[tokio::test]
async fn a_cut_log_sends_nothing_says_why_and_the_session_goes_on() {
    let home = Home::new();
    let script = script();
    let (handle, mut pushes, id) = loaded(&home, &script).await;
    let path = segment(&home, &id);
    let original = std::fs::read(&path).expect("读得了");
    let lines = original.iter().filter(|byte| **byte == b'\n').count();
    // 截掉最后两整行：有效历史里的最后两条读不回来。
    let keep = original
        .iter()
        .enumerate()
        .filter(|(_, byte)| **byte == b'\n')
        .nth(lines - 3)
        .map(|(at, _)| at + 1)
        .expect("有那么多行");
    std::fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .and_then(|file| file.set_len(keep as u64))
        .expect("截得了");
    ask(&handle, "cmd-2", say("第二句"))
        .await
        .expect("会话在跑");
    let pushed = until_turn_ends(&mut pushes).await;
    refused(&pushed, &script, 2, "to put back");
    // 会话照旧跑：再说一句照样收下、照样不发。
    ask(&handle, "cmd-3", say("第三句"))
        .await
        .expect("会话在跑");
    let pushed = until_turn_ends(&mut pushes).await;
    assert_eq!(script.requests().len(), 2, "还是读不回来，不发");
    assert_eq!(called(&pushed).len(), 1);
    stop(&handle).await;
}

/// 从第 `at` 个字节起写进 `bytes`，不动别的。
fn overwrite(path: &Path, bytes: &[u8], at: usize) {
    use std::io::{Seek, SeekFrom, Write};
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .expect("开得了");
    file.seek(SeekFrom::Start(at as u64)).expect("挪得了");
    file.write_all(bytes).expect("写得进");
}
