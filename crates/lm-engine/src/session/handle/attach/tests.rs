//! 头附的图片（LM-28）：本机文件带原来的路径；剪贴板贴的另拷一份进会话目录、块指着那一份；应急模式什么都不写、不带路径；
//! 超了上限的、不是图片的、读不了的各说各的原因。

use std::sync::Arc;

use lm_kernel::kernel::block::Block;

use super::{Attach, AttachError, Source, store};
use crate::store::blob::Blobs;
use crate::store::test_support::Scratch;
use crate::tool::picture;

/// 一张 `width` × `height` 的 PNG 的开头（量宽高只看它），后面垫 `pad` 个字节。
fn png(width: u32, height: u32, pad: usize) -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n\x00\x00\x00\x0dIHDR".to_vec();
    bytes.extend_from_slice(&width.to_be_bytes());
    bytes.extend_from_slice(&height.to_be_bytes());
    bytes.extend_from_slice(&[8, 6, 0, 0, 0, 0, 0, 0, 0]);
    bytes.resize(bytes.len() + pad, 0);
    bytes
}

fn image(blocks: Vec<Block>) -> lm_kernel::kernel::block::Image {
    match blocks.into_iter().next() {
        Some(Block::Image(image)) => image,
        other => panic!("要一块图片：{other:?}"),
    }
}

#[test]
fn a_local_file_keeps_its_own_path_and_a_pasted_one_is_copied_into_the_session() {
    let scratch = Scratch::new();
    let root = scratch.path();
    let blobs = Blobs::new(root.join("blobs"));
    let dir = root.join("sessions").join("s1").join("attachments");
    let shot = root.join("晚霞.png");
    std::fs::write(&shot, png(800, 600, 64)).unwrap();
    // 本机的文件：块带名字、宽高、原来的路径，不另拷。
    let local = Attach {
        name: "晚霞.png".into(),
        source: Source::File(shot.clone()),
        keep: false,
    };
    let got = image(store(&blobs, Some(&dir), vec![local]).unwrap());
    assert_eq!(
        got.name.as_ref().map(|name| name.as_str()),
        Some("晚霞.png")
    );
    assert_eq!((got.width, got.height), (800, 600));
    assert_eq!(got.media_type.as_str(), "image/png");
    let real = std::fs::canonicalize(&shot).unwrap();
    assert_eq!(got.path.as_ref().map(|path| path.as_str()), real.to_str());
    assert!(!dir.exists(), "本机的文件不另拷");
    assert_eq!(
        blobs.get(&got.blob).unwrap(),
        png(800, 600, 64),
        "存成了 blob"
    );
    // 剪贴板贴的（暂存的文件过后会删）：另拷一份进会话目录，块指着那一份。
    let pasted = Attach {
        name: "0123.png".into(),
        source: Source::File(shot.clone()),
        keep: true,
    };
    let got = image(store(&blobs, Some(&dir), vec![pasted]).unwrap());
    let kept = std::fs::canonicalize(dir.join("0123.png")).unwrap();
    assert_eq!(got.path.as_ref().map(|path| path.as_str()), kept.to_str());
    assert_eq!(std::fs::read(&kept).unwrap(), png(800, 600, 64));
}

#[test]
fn rescue_mode_writes_nothing_and_carries_no_path() {
    let scratch = Scratch::new();
    let blobs = Blobs::in_memory();
    let held: Arc<[u8]> = png(40, 30, 8).into();
    let item = Attach {
        name: "shot.png".into(),
        source: Source::Held(Arc::clone(&held)),
        keep: true,
    };
    let got = image(store(&blobs, None, vec![item]).unwrap());
    assert_eq!(got.path, None, "没有落盘的文件，不带路径");
    assert_eq!(blobs.get(&got.blob).unwrap(), &held[..]);
    assert_eq!(
        std::fs::read_dir(scratch.path()).unwrap().count(),
        0,
        "什么都没写"
    );
}

#[test]
fn each_refusal_says_which_one_and_why() {
    let scratch = Scratch::new();
    let root = scratch.path();
    let blobs = Blobs::new(root.join("blobs"));
    let file = |name: &str, bytes: &[u8]| {
        let path = root.join(name);
        std::fs::write(&path, bytes).unwrap();
        Attach {
            name: name.into(),
            source: Source::File(path),
            keep: false,
        }
    };
    let refused = |item: Attach| store(&blobs, None, vec![item]).unwrap_err();
    // 字节超了：不整份读进来。
    let big = png(10, 10, usize::try_from(picture::MAX_BYTES).unwrap());
    assert_eq!(
        refused(file("big.png", &big)),
        AttachError::TooBig("big.png".into())
    );
    // 一边超了。
    assert_eq!(
        refused(file("wide.png", &png(picture::MAX_SIDE + 1, 10, 0))),
        AttachError::TooBig("wide.png".into())
    );
    // 扩展名是图、内容不是：照内容认。
    assert_eq!(
        refused(file("notes.png", b"just text")),
        AttachError::NotImage("notes.png".into())
    );
    // 文件没了。
    let gone = Attach {
        name: "gone.png".into(),
        source: Source::File(root.join("gone.png")),
        keep: false,
    };
    let error = refused(gone);
    assert_eq!(error, AttachError::Unreadable("gone.png".into()));
    assert_eq!(
        (error.code(), error.name()),
        ("attachment_unreadable", "gone.png")
    );
    // 有一张附不上的整批不算。
    let good = file("ok.png", &png(8, 6, 0));
    let bad = file("bad.png", b"x");
    assert!(store(&blobs, None, vec![good, bad]).is_err());
}
