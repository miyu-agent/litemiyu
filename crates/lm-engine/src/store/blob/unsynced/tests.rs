//! 存了、还没同步的 blob（速度线三）：登记照先后编号，同步到哪一个就拿掉到哪一个；不同步地存，名字下的内容是整份的，
//! 断电留下的短文件照新的重写。

use std::fs;

use lm_kernel::kernel::id::ContentHash;

use super::*;
use crate::store::blob::Blobs;

/// 测试用的临时目录，用完删掉。
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir =
            std::env::temp_dir().join(format!("litemiyu-unsynced-{name}-{}", std::process::id()));
        drop(fs::remove_dir_all(&dir));
        fs::create_dir_all(&dir).expect("建得了临时目录");
        Scratch(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.0));
    }
}

#[test]
fn what_was_added_is_synced_in_order_and_taken_off_up_to_where_it_got() {
    let mut unsynced = Unsynced::default();
    assert_eq!(unsynced.latest(), None, "一个都没登记过");
    assert!(!unsynced.waiting(u64::MAX));
    unsynced.add(PathBuf::from("a"));
    unsynced.add(PathBuf::from("b"));
    assert_eq!(unsynced.latest(), Some(1));
    // 这一刻要写的事件引用到第 1 个为止；之后又存了一个。
    unsynced.add(PathBuf::from("c"));
    assert!(unsynced.waiting(1));
    assert_eq!(
        unsynced.upto(1),
        Some((1, vec![PathBuf::from("a"), PathBuf::from("b")])),
        "只交到第 1 个"
    );
    unsynced.synced(1);
    assert!(!unsynced.waiting(1), "到第 1 个的都同步了");
    assert!(unsynced.waiting(2), "第 2 个还没");
    assert_eq!(unsynced.upto(u64::MAX), Some((2, vec![PathBuf::from("c")])));
    unsynced.synced(2);
    assert!(!unsynced.waiting(u64::MAX));
    assert_eq!(unsynced.latest(), Some(2), "编号接着数，不回头");
}

#[test]
fn stored_without_a_sync_the_name_holds_the_whole_content_and_syncs_later() {
    let scratch = Scratch::new("put");
    let (blobs, unsynced) = Blobs::new(scratch.0.join("blobs")).deferring();
    let content = vec![b'x'; 64 * 1024];
    let hash = blobs.keep(&content).expect("存得下");
    assert_eq!(hash, ContentHash::of(&content));
    assert_eq!(blobs.get(&hash).expect("读得出"), content, "名字下是整份的");
    let waiting = unsynced.lock().expect("没 panic").upto(u64::MAX);
    let Some((last, paths)) = waiting else {
        panic!("登记了要同步的")
    };
    assert_eq!(paths, [blobs.path(&hash)]);
    sync_files(&paths).expect("同步得了");
    unsynced.lock().expect("没 panic").synced(last);
    assert!(!unsynced.lock().expect("没 panic").waiting(u64::MAX));
}

#[test]
fn a_short_file_left_under_the_name_is_written_again() {
    let scratch = Scratch::new("short");
    let (blobs, _unsynced) = Blobs::new(scratch.0.join("blobs")).deferring();
    let content = vec![b'y'; 40 * 1024];
    let hash = ContentHash::of(&content);
    // 断电时改了名、数据没落盘：名字下是一个短文件。
    let path = blobs.path(&hash);
    fs::create_dir_all(path.parent().expect("有上一级")).expect("建得了");
    fs::write(&path, b"").expect("写得了");
    assert_eq!(blobs.keep(&content).expect("存得下"), hash);
    assert_eq!(fs::read(&path).expect("读得出"), content, "照新的重写了");
}

#[test]
fn without_deferring_it_is_synced_on_the_spot_as_before() {
    let scratch = Scratch::new("plain");
    let blobs = Blobs::new(scratch.0.join("blobs"));
    let hash = blobs.keep(b"plain").expect("存得下");
    assert_eq!(blobs.get(&hash).expect("读得出"), b"plain");
}
