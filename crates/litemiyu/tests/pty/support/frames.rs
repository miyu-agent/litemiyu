//! 逐帧看（界面线第十九趟）：程序一帧攒成一份字节、套着同步输出（DEC 2026）的首尾写出去（`src/screen.rs`），照收尾那一截
//! 切开，一帧帧放进一块新的屏幕，每帧画完时的样子都留下来。只看最后的屏幕抓不到一闪而过的那一帧（切会话闪首页）。

use std::time::Instant;

use super::{Session, contains};

/// 一帧的收尾（`src/screen.rs` 的 `END`）。
const END: &[u8] = b"\x1b[?2026l";

impl Session {
    /// 程序写出来的字节现在有多长：之后的帧从这里数（[`Session::frames_since`]）。
    pub fn mark(&self) -> usize {
        self.raw().len()
    }

    /// 从 `mark`（[`Session::mark`] 交的）起写出来的每一帧画完时屏幕上的字，照先后。
    pub fn frames_since(&self, mark: usize) -> Vec<String> {
        self.timed_frames_since(mark)
            .into_iter()
            .map(|(_, frame)| frame)
            .collect()
    }

    /// 同 [`Session::frames_since`]，连同每一帧的收尾是什么时候读到的。
    pub fn timed_frames_since(&self, mark: usize) -> Vec<(Instant, String)> {
        // 先拿字节再拿时刻：拿到的字节每一截都记过时刻。
        let raw = self.raw();
        let stamps = self.stamps.lock().expect("锁得上").clone();
        let read_at = |upto: usize| {
            stamps
                .iter()
                .find(|(len, _)| *len >= upto)
                .map_or_else(Instant::now, |(_, at)| *at)
        };
        let (rows, cols) = self.screen.lock().expect("锁得上").screen().size();
        let mut parser = vt100::Parser::new(rows, cols, 0);
        parser.process(&raw[..mark]);
        let mut frames = Vec::new();
        let mut at = mark;
        while let Some(end) = find(&raw[at..], END) {
            let next = at + end + END.len();
            parser.process(&raw[at..next]);
            frames.push((read_at(next), parser.screen().contents()));
            at = next;
        }
        debug_assert!(!contains(&raw[at..], END));
        frames
    }
}

/// `hay` 里 `needle` 头一回在哪。
fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}
