//! 头附的图片接在会话上整条走通（LM-28，`session/handle/attach.rs`）：贴的图存成 blob、另拷一份进会话目录，跟着那句话每次
//! 请求都交给模型；停下重开以后还在；撤销拿走、恢复带回；压缩以后留在尾巴里的还在、压掉的不再发；应急模式照样交给模型，
//! 场地里一个文件都不多。fork 子代理带着主会话的图在 `fork.rs`。

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use lm_engine::session::testkit::{Play, Script};
use lm_engine::session::{Attach, AttachSource, Create, Handle, create_rescue, new_id};
use lm_engine::store::blob::Blobs;
use lm_engine::tool::Catalog;
use lm_kernel::kernel::block::{Block, Image, Text};
use lm_kernel::kernel::facts::Environment;
use lm_kernel::kernel::request::{Message, Request};
use lm_kernel::kernel::session::Command;

use crate::support::jobs::talk;
use crate::support::*;

/// 一张 `width` × `height` 的 PNG 的开头（量宽高只看它）。
fn png(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR".to_vec();
    bytes.extend_from_slice(&width.to_be_bytes());
    bytes.extend_from_slice(&height.to_be_bytes());
    bytes.extend_from_slice(&[8, 6, 0, 0, 0, 0, 0, 0, 0]);
    bytes
}

/// 请求里人这边的消息带的图片，照先后。
fn pictures(request: &Request) -> Vec<Image> {
    request
        .messages
        .iter()
        .filter_map(|message| match message {
            Message::User { blocks } => Some(blocks),
            _ => None,
        })
        .flatten()
        .filter_map(|block| match block {
            Block::Image(image) => Some(image.clone()),
            _ => None,
        })
        .collect()
}

/// 把 `bytes` 当剪贴板贴的图（暂存在场地里的一个文件）附上，交回图片块。
async fn pasted(home: &Home, handle: &Handle, name: &str, bytes: &[u8]) -> Vec<Block> {
    let staged = home.scratch.0.join(name);
    std::fs::write(&staged, bytes).expect("写得了暂存的图");
    let item = Attach {
        name: name.to_string(),
        source: AttachSource::File(staged),
        keep: true,
    };
    within("附图", handle.attach(vec![item]))
        .await
        .expect("会话在跑")
        .expect("附得上")
        .blocks
}

/// 说一句带着 `blocks`（图片块）的话，等这一轮说完。
async fn show(handle: &Handle, id: &str, words: &str, mut blocks: Vec<Block>) {
    blocks.insert(
        0,
        Block::Text(Text {
            text: words.to_string(),
        }),
    );
    let mut pushes = watch(handle).await;
    let send = Command::Send {
        blocks,
        urgent: false,
    };
    ask(handle, id, send).await.expect("会话在跑");
    until_turn_ends(&mut pushes).await;
}

fn main_requests(script: &Script) -> Vec<Request> {
    script.requests().into_iter().map(|(_, r)| r).collect()
}

#[tokio::test]
async fn a_pasted_picture_goes_with_every_request_and_is_still_there_after_reopening() {
    let home = Home::new();
    let script = Script::new([
        Play::Says("看到了"),
        Play::Says("还在"),
        Play::Says("重开以后也在"),
    ]);
    let handle = home.create(&script).await;
    let shot = png(800, 600);
    let blocks = pasted(&home, &handle, "0a1b.png", &shot).await;
    show(&handle, "cmd-1", "看 [图片 1]", blocks).await;
    talk(&handle, "cmd-2", "第二句").await;
    let session = handle.id().clone();
    // 暂存的那一份删了（界面退出时删）：会话留着自己的。
    std::fs::remove_file(home.scratch.0.join("0a1b.png")).expect("删得了");
    stop(&handle).await;
    drop(handle);
    let reopened = home.load(&session, &script).await;
    talk(&reopened, "cmd-3", "第三句").await;

    let requests = main_requests(&script);
    assert_eq!(requests.len(), 3);
    let kept = std::fs::canonicalize(
        home.root
            .session(&session)
            .join("attachments")
            .join("0a1b.png"),
    )
    .expect("会话目录里留着一份");
    for request in &requests {
        let found = pictures(request);
        assert_eq!(found.len(), 1, "每一次请求都带着那张图");
        let image = &found[0];
        assert_eq!((image.width, image.height), (800, 600));
        assert_eq!(image.media_type.as_str(), "image/png");
        assert_eq!(image.name.as_ref().map(|n| n.as_str()), Some("0a1b.png"));
        assert_eq!(image.path.as_ref().map(|p| p.as_str()), kept.to_str());
        let stored = Blobs::new(home.root.blobs()).get(&image.blob);
        assert_eq!(stored.expect("blob 取得出"), shot);
    }
    assert_eq!(std::fs::read(&kept).expect("读得了"), shot);
}

#[tokio::test]
async fn undo_takes_the_picture_away_and_restoring_brings_it_back() {
    let home = Home::new();
    let script = Script::new([
        Play::Says("看到了"),
        Play::Says("没有图的一轮"),
        Play::Says("恢复以后"),
    ]);
    let handle = home.create(&script).await;
    let blocks = pasted(&home, &handle, "shot.png", &png(40, 30)).await;
    show(&handle, "cmd-1", "看 [图片 1]", blocks).await;
    // 撤掉带图的那一轮：下一次请求里没有它。
    let undone = ask(&handle, "cmd-2", Command::Revert { turn: None }).await;
    assert!(matches!(
        undone,
        Ok(lm_kernel::kernel::session::Outcome::Accepted { .. })
    ));
    // 恢复：带图的那一轮回来了。
    let restored = ask(&handle, "cmd-3", Command::Unrevert).await;
    assert!(matches!(
        restored,
        Ok(lm_kernel::kernel::session::Outcome::Accepted { .. })
    ));
    talk(&handle, "cmd-4", "第二句").await;
    // 再撤两轮里后面那一轮不动图；撤到头就没有图了。
    let first_turn = home
        .log(handle.id())
        .iter()
        .find(|event| matches!(event.body, lm_kernel::kernel::event::Body::TurnStarted(_)))
        .map(|event| lm_kernel::kernel::id::TurnId::new(event.seq))
        .expect("开过一轮");
    let all = Command::Revert {
        turn: Some(first_turn),
    };
    ask(&handle, "cmd-5", all).await.expect("会话在跑");
    talk(&handle, "cmd-6", "从头来").await;
    let requests = main_requests(&script);
    assert_eq!(requests.len(), 3);
    assert_eq!(pictures(&requests[0]).len(), 1);
    assert_eq!(pictures(&requests[1]).len(), 1, "恢复以后那张图还在上文里");
    assert_eq!(pictures(&requests[2]).len(), 0, "撤掉了带图的那一轮");
}

/// 是压缩的摘要请求（同 `compaction.rs`）。
fn summarizing(request: &Request) -> bool {
    matches!(
        request.messages.last(),
        Some(Message::User { blocks })
            if matches!(blocks.last(), Some(Block::Text(text)) if text.text.starts_with("Respond with text only."))
    )
}

fn answer(request: &Request) -> Play {
    match summarizing(request) {
        true => Play::Says("<summary>SUMMARY</summary>"),
        false => Play::Says("好"),
    }
}

#[tokio::test]
async fn after_compaction_the_kept_picture_still_goes_out_and_requests_stay_whole() {
    let home = Home::new();
    let script = Script::new([]).answers(answer).recording();
    let handle = home.create(&script).await;
    // 第一句说七万个字：超了出厂的尾巴，手动压得掉它（同 `rebuild.rs`）；它带着图甲。第二句短，带着图乙，留在尾巴里。
    let first = format!("第一句 [图片 1] {}", "x".repeat(70_000));
    let a = pasted(&home, &handle, "a.png", &png(11, 11)).await;
    show(&handle, "cmd-1", &first, a).await;
    let b = pasted(&home, &handle, "b.png", &png(22, 22)).await;
    show(&handle, "cmd-2", "第二句 [图片 2]", b).await;
    let mut pushes = watch(&handle).await;
    let compact = Command::Compact { instructions: None };
    ask(&handle, "cmd-3", compact).await.expect("会话在跑");
    until_turn_ends(&mut pushes).await;
    assert!(
        home.log(handle.id()).iter().any(|event| matches!(
            event.body,
            lm_kernel::kernel::event::Body::ContextCompacted(_)
        )),
        "压了"
    );
    talk(&handle, "cmd-4", "第三句").await;
    let requests = main_requests(&script);
    let summary = requests
        .iter()
        .find(|request| summarizing(request))
        .expect("发过摘要请求");
    let widths = |request: &Request| -> Vec<u32> {
        pictures(request).iter().map(|image| image.width).collect()
    };
    assert_eq!(widths(summary), [11], "摘要请求带着要压掉的那一段里的图");
    let last = requests.last().expect("请求了");
    assert_eq!(widths(last), [22], "压掉的那一段里的图不再发，尾巴里的照发");
    // 重开以后照样。
    let session = handle.id().clone();
    stop(&handle).await;
    drop(handle);
    let reopened = home.load(&session, &script).await;
    talk(&reopened, "cmd-5", "第四句").await;
    let last = main_requests(&script).pop().expect("请求了");
    assert_eq!(widths(&last), [22]);
}

/// 场地里的每一项（不跟链接）。
fn tree(root: &Path) -> BTreeSet<PathBuf> {
    let mut found = BTreeSet::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                stack.push(path.clone());
            }
            found.insert(path);
        }
    }
    found
}

#[tokio::test]
async fn rescue_mode_sends_the_picture_and_writes_nothing() {
    let home = Home::new();
    let script = Script::new([Play::Says("看到了"), Play::Says("还在")]);
    let created = create_rescue(Create {
        root: &home.root,
        id: new_id(now()),
        attended: false,
        oneshot: false,
        environment: Environment {
            cwd: home.scratch.0.to_string_lossy().into_owned(),
            ..environment()
        },
        command: id("cmd-0"),
        by: alice(),
        models: &script,
        tools: Arc::new(Catalog::default()),
        home: Some(&home.home),
        configs: home.configs.clone(),
        model: None,
        no_tools: false,
        preset: None,
    });
    let handle = within("造会话", created).await.expect("造得出会话");
    let before = tree(&home.scratch.0);
    let shot: Arc<[u8]> = png(64, 48).into();
    let item = Attach {
        name: "shot.png".to_string(),
        source: AttachSource::Held(Arc::clone(&shot)),
        keep: true,
    };
    let blocks = within("附图", handle.attach(vec![item]))
        .await
        .expect("会话在跑")
        .expect("附得上")
        .blocks;
    show(&handle, "cmd-1", "看 [图片 1]", blocks).await;
    talk(&handle, "cmd-2", "第二句").await;
    for request in main_requests(&script) {
        let found = pictures(&request);
        assert_eq!(found.len(), 1);
        assert_eq!((found[0].width, found[0].height), (64, 48));
        assert_eq!(found[0].path, None, "没有落盘的文件，不带路径");
    }
    assert_eq!(
        tree(&home.scratch.0),
        before,
        "场地里一个文件、一个目录都不多"
    );
}
