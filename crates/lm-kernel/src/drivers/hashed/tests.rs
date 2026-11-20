//! 请求体的哈希接着上一次算（速度线四第 1 条）：一样的那一截量得对；照留着的中间状态接着算出来的，和从头算的逐字节一样。
//! 随机造一串请求体：往后接、改中间、改开头、截短、换掉一段 blob、整份换掉，一个个比。

use std::collections::BTreeMap;
use std::sync::Arc;

use super::*;
use crate::drivers::Fragment;
use crate::kernel::id::ContentHash;

/// 照 [`SPACING`] 喂、留中间状态，同发请求那一头（`lm-engine` 的 `http/body.rs`）：从 `from` 接着喂 `bytes` 的第 `from.0`
/// 个字节以后的，交回结果和这一次的中间状态。
fn hash_from(
    bytes: &[u8],
    from: Option<(u64, Hasher)>,
    mut points: Vec<(u64, Hasher)>,
) -> (ContentHash, Vec<(u64, Hasher)>) {
    let (start, mut hasher) = from.unwrap_or_default();
    let mut at = start;
    let mut rest = &bytes[usize::try_from(start).expect("在范围里")..];
    while !rest.is_empty() {
        let next = (at / SPACING + 1) * SPACING;
        let take = usize::try_from(next - at)
            .expect("在范围里")
            .min(rest.len());
        hasher.update(&rest[..take]);
        at += take as u64;
        rest = &rest[take..];
        if at == next {
            points.push((at, hasher.clone()));
        }
    }
    (hasher.finish(), points)
}

/// 一个小小的伪随机数（xorshift），种子固定，每次跑一样。
struct Dice(u64);

impl Dice {
    fn roll(&mut self, below: u64) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 % below.max(1)
    }

    fn bytes(&mut self, len: u64) -> Vec<u8> {
        (0..len).map(|_| b'a' + self.roll(26) as u8).collect()
    }
}

/// 一份请求体：几段字（共用的、不共用的混着）和 blob。
fn body(parts: &[Fragment]) -> Fragments {
    parts.iter().cloned().collect()
}

#[test]
fn the_same_bytes_count_as_the_same_however_they_are_cut() {
    let whole = body(&[Fragment::Text(b"abcdef".to_vec())]);
    let cut = body(&[
        Fragment::Text(b"ab".to_vec()),
        Fragment::Shared(Arc::from(&b"cde"[..])),
        Fragment::Text(b"f".to_vec()),
    ]);
    assert_eq!(whole.common_len(&cut), 6);
    assert!(whole == cut, "照拼起来的字节比");
    let other = body(&[Fragment::Text(b"abcXef".to_vec())]);
    assert_eq!(whole.common_len(&other), 3, "第 4 个字节不一样");
    let blob = ContentHash::of(b"x");
    let with = body(&[
        Fragment::Text(b"ab".to_vec()),
        Fragment::Base64 {
            blob: blob.clone(),
            size: 1,
        },
        Fragment::Text(b"z".to_vec()),
    ]);
    let other_blob = body(&[
        Fragment::Text(b"ab".to_vec()),
        Fragment::Base64 {
            blob: ContentHash::of(b"y"),
            size: 1,
        },
    ]);
    assert_eq!(with.common_len(&other_blob), 2, "blob 换了，从它开始不一样");
    assert_eq!(with.common_len(&with.clone()), with.len());
    assert!(with != other_blob);
}

#[test]
fn hashing_on_from_the_last_request_matches_hashing_from_scratch() {
    let mut dice = Dice(0x5eed_cafe_f00d_1234);
    let mut blobs: BTreeMap<ContentHash, Vec<u8>> = BTreeMap::new();
    let mut parts: Vec<Fragment> =
        vec![Fragment::Text(b"{\"model\":\"m\",\"messages\":[".to_vec())];
    let mut hashed = Hashed::default();
    for round in 0..300 {
        match dice.roll(10) {
            // 往后接一条（多数时候）：共用的字，偶尔带一张图。
            0..=5 => {
                let len = 1 + dice.roll(40 * 1024);
                parts.push(Fragment::Shared(Arc::from(dice.bytes(len))));
                if dice.roll(8) == 0 {
                    let content = {
                        let len = 1 + dice.roll(3000);
                        dice.bytes(len)
                    };
                    let hash = ContentHash::of(&content);
                    let size = content.len() as u64;
                    blobs.insert(hash.clone(), content);
                    parts.push(Fragment::Base64 { blob: hash, size });
                }
            }
            // 改中间的一段（撤掉重发、压缩以后从那里起不一样）。
            6 => {
                let at = dice.roll(parts.len() as u64) as usize;
                parts.truncate(at.max(1));
                let len = 1 + dice.roll(8 * 1024);
                parts.push(Fragment::Shared(Arc::from(dice.bytes(len))));
            }
            // 开头变了（换模型、改输出上限）。
            7 => {
                parts[0] = Fragment::Text({
                    let len = 1 + dice.roll(64);
                    dice.bytes(len)
                })
            }
            // 截短（压缩）。
            8 => parts.truncate(1 + dice.roll(parts.len() as u64) as usize),
            // 整份换掉（改工具面、换 system：从很前面就不一样）。
            _ => {
                parts.truncate(1);
                parts.push(Fragment::Text({
                    let len = 1 + dice.roll(20 * 1024);
                    dice.bytes(len)
                }));
            }
        }
        // 尾巴每一次照这一次的写。
        let mut sent = parts.clone();
        sent.push(Fragment::Text({
            let len = 1 + dice.roll(30);
            dice.bytes(len)
        }));
        let sent = body(&sent);
        let bytes = sent.to_bytes(&blobs).expect("blob 都在");
        let (from, kept) = hashed.resume(&sent);
        if let Some((at, _)) = &from {
            assert!(
                *at <= hashed.last.common_len(&sent),
                "第 {round} 次：从一样的那一截里接着算"
            );
        }
        let (hash, points) = hash_from(&bytes, from, kept);
        assert_eq!(
            hash,
            ContentHash::of(&bytes),
            "第 {round} 次：和从头算的一样"
        );
        hashed.remember(sent, points);
    }
}
