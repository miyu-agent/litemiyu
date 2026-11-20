//! 改前改后存的 blob 不当场同步（速度线三）：引用它们的事件要等它们同步了才写进日志——后台同步先同步 blob、再同步日志；
//! 要同步的那一批等不了，写以前先把它引用到的同步掉。程序这时被杀，日志里没有引用了没落盘的 blob 的事件。

use std::fs;
use std::path::PathBuf;

use super::held::{Rig, reply, seqs, tool_result};
use super::*;

/// 一个真的文件，当作工具刚存了、还没同步的 blob（同步要打开它）。用完删掉。
struct Stored(PathBuf);

impl Stored {
    fn new(name: &str) -> Stored {
        let dir =
            std::env::temp_dir().join(format!("litemiyu-held-blob-{name}-{}", std::process::id()));
        fs::create_dir_all(&dir).expect("建得了临时目录");
        let path = dir.join("blob");
        fs::write(&path, b"after").expect("写得了");
        Stored(path)
    }
}

impl Drop for Stored {
    fn drop(&mut self) {
        if let Some(dir) = self.0.parent() {
            drop(fs::remove_dir_all(dir));
        }
    }
}

/// 造会话那一条的同步放行、做完：路上没有同步了。
async fn settled() -> Rig {
    let mut rig = Rig::new().await;
    rig.gate.let_through(1);
    let (_, result) = within("同步做完了", rig.actor.synced.recv())
        .await
        .expect("报回来了");
    within("收尾", rig.actor.after_sync(result))
        .await
        .unwrap_or_else(|Stop| panic!("写得进"));
    assert!(!rig.actor.sync.lock().expect("没 panic").durable.busy());
    rig
}

#[tokio::test]
async fn a_result_waits_for_its_blob_and_a_kill_leaves_no_dangling_reference() {
    let mut rig = settled().await;
    let blob = Stored::new("kill");
    rig.blobs.lock().expect("没 panic").add(blob.0.clone());
    let stored = within("工具的结果", rig.actor.append(vec![tool_result(10)])).await;
    assert!(
        matches!(stored, Ok(Some(Input::Stored { .. }))),
        "当场交回落盘了"
    );
    assert_eq!(seqs(&rig.written), [1], "blob 没同步：引用它的结果攒着");
    assert!(
        rig.actor.sync.lock().expect("没 panic").durable.busy(),
        "为 blob 派了一次同步"
    );
    // 被杀：日志里没有引用它的那一条。
    drop(rig.actor);
    rig.gate.let_through(10);
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(seqs(&rig.written), [1]);
}

#[tokio::test]
async fn the_blob_is_synced_before_the_result_that_refers_to_it_is_written() {
    let mut rig = settled().await;
    let blob = Stored::new("order");
    rig.blobs.lock().expect("没 panic").add(blob.0.clone());
    within("工具的结果", rig.actor.append(vec![tool_result(10)]))
        .await
        .unwrap_or_else(|Stop| panic!("写得进"));
    rig.gate.let_through(1);
    let (_, result) = within("同步做完了", rig.actor.synced.recv())
        .await
        .expect("报回来了");
    within("写攒着的", rig.actor.after_sync(result))
        .await
        .unwrap_or_else(|Stop| panic!("写得进"));
    assert_eq!(seqs(&rig.written), [1, 10], "blob 同步了，结果写进去");
    assert_eq!(
        rig.clean.lock().expect("没 panic").last(),
        Some(&true),
        "写结果的那一刻 blob 已经同步了"
    );
}

#[tokio::test]
async fn a_batch_that_cannot_wait_syncs_the_blob_first() {
    let mut rig = Rig::new().await;
    let blob = Stored::new("first");
    rig.blobs.lock().expect("没 panic").add(blob.0.clone());
    within("工具的结果", rig.actor.append(vec![tool_result(10)]))
        .await
        .unwrap_or_else(|Stop| panic!("写得进"));
    assert_eq!(seqs(&rig.written), [1]);
    // 同步还在路上（没放行），带工具调用的回复来了：等不了，先把 blob 同步了再连同攒着的写。
    within("带工具调用的回复", rig.actor.append(vec![reply(11, true)]))
        .await
        .unwrap_or_else(|Stop| panic!("写得进"));
    assert_eq!(seqs(&rig.written), [1, 10, 11]);
    assert_eq!(
        rig.clean.lock().expect("没 panic").last(),
        Some(&true),
        "写的那一刻 blob 已经同步了"
    );
    assert!(!rig.blobs.lock().expect("没 panic").waiting(u64::MAX));
    rig.gate.let_through(10);
}
