//! 拆成片段和整份写一个字节都不差（施工 V-38）。

use std::collections::BTreeMap;

use serde::Serialize;

use super::*;

#[derive(Serialize)]
struct Url {
    url: Inline,
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Part {
    Text { text: String },
    ImageUrl { image_url: Url },
    Source { data: Inline, media_type: String },
}

#[test]
fn an_inline_blob_is_cut_into_its_own_fragment() {
    let picture = b"\x89PNG\r\n\x1a\nabcdefg".to_vec();
    let hash = ContentHash::of(&picture);
    let size = u64::try_from(picture.len()).unwrap();
    let parts = vec![
        Part::Text {
            text: "看 \"这\" 张\n".to_string(),
        },
        Part::ImageUrl {
            image_url: Url {
                url: Inline::data_url("image/png", &hash, size),
            },
        },
        Part::Source {
            data: Inline::bare(&hash, size),
            media_type: "image/\"png\"".to_string(),
        },
    ];
    let mut builder = Builder::default();
    builder.push(b'[');
    builder.json(&parts);
    builder.push(b']');
    let fragments = builder.finish();
    let blobs: BTreeMap<ContentHash, Vec<u8>> = [(hash.clone(), picture.clone())].into();
    let body = fragments.to_bytes(&blobs).unwrap();
    let encoded = base64::encode(&picture);
    let expected = format!(
        "[[{{\"type\":\"text\",\"text\":\"看 \\\"这\\\" 张\\n\"}},\
         {{\"type\":\"image_url\",\"image_url\":{{\"url\":\"data:image/png;base64,{encoded}\"}}}},\
         {{\"type\":\"source\",\"data\":\"{encoded}\",\"media_type\":\"image/\\\"png\\\"\"}}]]"
    );
    assert_eq!(String::from_utf8(body.clone()).unwrap(), expected);
    assert_eq!(fragments.len(), u64::try_from(body.len()).unwrap());
    let kinds: Vec<bool> = fragments
        .parts()
        .iter()
        .map(|part| matches!(part, Fragment::Base64 { .. }))
        .collect();
    assert_eq!(kinds, [false, true, false, true, false]);
}

#[test]
fn positions_count_the_base64_that_is_not_there_yet() {
    let hash = ContentHash::of(b"abcd");
    let mut builder = Builder::default();
    builder.extend_from_slice(b"ab");
    builder.json(&Inline::bare(&hash, 4));
    // `"` + 8 个字的 base64 + `"`。
    assert_eq!(builder.len(), 2 + 1 + 8 + 1);
    assert_eq!(builder.pop(), Some(b'"'));
    assert_eq!(builder.len(), 11);
}

#[test]
fn a_blob_of_another_size_is_missing() {
    let hash = ContentHash::of(b"abc");
    let mut builder = Builder::default();
    builder.json(&Inline::bare(&hash, 4));
    let fragments = builder.finish();
    let blobs: BTreeMap<ContentHash, Vec<u8>> = [(hash.clone(), b"abc".to_vec())].into();
    assert_eq!(
        fragments.to_bytes(&blobs),
        Err(EncodeError::MissingBlob(hash))
    );
}

#[test]
fn whole_text_is_one_fragment() {
    let fragments = Fragments::from(b"{}".to_vec());
    assert_eq!(fragments.parts(), [Fragment::Text(b"{}".to_vec())]);
    assert_eq!(fragments.len(), 2);
    assert!(Fragments::from(Vec::new()).is_empty());
}

/// 驱动留着的共用（速度线四第 2 条）：接上去不拷，拿的是同一份；要改结尾的拷进来；拼起来的字节和全拷的一样。
#[test]
fn shared_pieces_are_referenced_not_copied_and_give_the_same_bytes() {
    let kept = Fragments::from(br#"{"role":"user","content":"hi"}"#.to_vec()).shared();
    let [Fragment::Shared(kept_text)] = kept.parts() else {
        panic!("换成了共用的：{kept:?}")
    };
    let mut shared = Builder::default();
    shared.extend_from_slice(b"[");
    shared.append(&kept);
    shared.push(b',');
    shared.append(&kept);
    shared.push(b']');
    let shared = shared.finish();
    let referenced: Vec<&Arc<[u8]>> = shared
        .parts()
        .iter()
        .filter_map(|part| match part {
            Fragment::Shared(text) => Some(text),
            _ => None,
        })
        .collect();
    assert_eq!(referenced.len(), 2);
    assert!(
        referenced.iter().all(|text| Arc::ptr_eq(text, kept_text)),
        "拿的是留着的那一份"
    );
    let mut copied = Builder::default();
    copied.extend_from_slice(b"[");
    copied.append_copy(&kept);
    copied.push(b',');
    copied.append_copy(&kept);
    copied.push(b']');
    let copied = copied.finish();
    assert_eq!(copied.parts().len(), 1, "拷进来的连成一段");
    let blobs: BTreeMap<ContentHash, Vec<u8>> = BTreeMap::new();
    assert_eq!(shared.to_bytes(&blobs), copied.to_bytes(&blobs));
    assert!(shared == copied, "照拼起来的字节比，一样");
    assert_eq!(shared.len(), copied.len());
}
