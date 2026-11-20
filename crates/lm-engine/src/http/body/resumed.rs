//! 请求体的哈希接着上一次算（速度线四第 1 条，[`Body::resuming`]）：一串主请求照会话那样一次次发，每一次照留着的中间状态
//! 接着算，送完了的、没送完就要哈希的（响应头早到了），算出来都和整份从头算的逐字节一样。

use std::collections::BTreeMap;
use std::future::poll_fn;
use std::io::Cursor;

use http_body::Body as _;
use lm_kernel::drivers::hashed::Hashed;
use lm_kernel::drivers::{Encoded, Fragment};

use super::*;

/// 内存里的 blob。
#[derive(Default)]
struct Memory(BTreeMap<ContentHash, Vec<u8>>);

impl BlobSource for Memory {
    fn open(&self, blob: &ContentHash, offset: u64) -> io::Result<Box<dyn Read + Send>> {
        let bytes = self
            .0
            .get(blob)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "没有"))?;
        let offset = usize::try_from(offset)
            .unwrap_or(usize::MAX)
            .min(bytes.len());
        Ok(Box::new(Cursor::new(bytes[offset..].to_vec())))
    }
}

/// 一个小小的伪随机数（xorshift），种子固定。
struct Dice(u64);

impl Dice {
    fn roll(&mut self, below: u64) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 % below.max(1)
    }

    fn bytes(&mut self, len: u64) -> Vec<u8> {
        (0..len)
            .map(|_| b'a' + u8::try_from(self.roll(26)).unwrap_or(0))
            .collect()
    }
}

async fn drain(stream: &mut Stream) {
    while let Some(frame) = poll_fn(|cx| Pin::new(&mut *stream).poll_frame(cx)).await {
        frame.expect("送得出");
    }
}

#[tokio::test]
async fn a_run_of_main_requests_hashes_on_and_matches_hashing_from_scratch() {
    let mut dice = Dice(0x0dd_ba11_c0ff_ee42);
    let mut blobs = BTreeMap::new();
    let mut parts = vec![Fragment::Text(b"{\"messages\":[".to_vec())];
    let mut hashed = Hashed::default();
    for round in 0..120 {
        match dice.roll(8) {
            0..=4 => {
                let len = 1 + dice.roll(30 * 1024);
                parts.push(Fragment::Shared(Arc::from(dice.bytes(len))));
            }
            5 => {
                let content = {
                    let len = 1 + dice.roll(70_000);
                    dice.bytes(len)
                };
                let hash = ContentHash::of(&content);
                let size = u64::try_from(content.len()).unwrap_or(0);
                blobs.insert(hash.clone(), content);
                parts.push(Fragment::Base64 { blob: hash, size });
            }
            6 => {
                let keep = 1 + dice.roll(u64::try_from(parts.len()).unwrap_or(1));
                parts.truncate(usize::try_from(keep).unwrap_or(1));
            }
            _ => {
                parts[0] = Fragment::Text({
                    let len = 1 + dice.roll(40);
                    dice.bytes(len)
                })
            }
        }
        let mut sent = parts.clone();
        sent.push(Fragment::Text(b"]}".to_vec()));
        let sent: Fragments = sent.into_iter().collect();
        let whole = Encoded {
            body: sent.clone(),
            messages: Vec::new(),
            path: String::new(),
        }
        .to_bytes(&blobs)
        .expect("blob 都在")
        .body;
        // blob 交给发请求的那一头读：内存里放一份同样的。
        let memory = Arc::new(Memory(blobs.clone()));
        let (from, kept) = hashed.resume(&sent);
        let points = Arc::new(Mutex::new(kept));
        let body = Body::new(sent.clone(), memory).resuming(from, Arc::clone(&points));
        let replay = Arc::new(Replay::new(body));
        // 有一半送完了再要哈希，一半一个字节都没送就要（响应头早到了）。
        if dice.roll(2) == 0 {
            drain(&mut replay.stream()).await;
        }
        assert_eq!(
            replay.hash().await,
            ContentHash::of(&whole),
            "第 {round} 次：接着算的和从头算的一样"
        );
        let points = std::mem::take(&mut *points.lock().expect("没 panic"));
        hashed.remember(sent, points);
    }
}
