//! 请求体的哈希接着上一次算（速度线四第 1 条）：请求体的 SHA-256 记进 `model.called` 的 `request`，跟着送出去的字节一路算
//! （`lm-engine` 的 `http/body.rs`）。主请求一步步往后长，每一次和上一次大半一样，原来每一次从第一个字节算起（1 MB 一次
//! 1.15 ms）。这里留着上一次的请求体（片段清单，字的那几段多半和驱动留着的共用，不另占）和算到每隔 [`SPACING`] 个字节时
//! 哈希的中间状态；这一次照「和上一次一样的那一截」末尾以前最近的那一个状态接着算。算出来和从头算的逐字节一样：SHA-256
//! 照字节一个个喂，从同样的状态喂同样的字节，结果一样。

use std::fmt;

use crate::drivers::Fragments;
use crate::kernel::id::Hasher;

/// 每隔多少个字节留一个中间状态：从一样的那一截的末尾往回最多重算这么多。1 MB 的请求留 64 个，每个一百多字节。
pub const SPACING: u64 = 16 * 1024;

/// 一个中间状态：前多少个字节算过了、那一刻的状态。
pub type Point = (u64, Hasher);

/// 上一次请求体的哈希算到哪：请求体本身和中间状态（照先后，「前多少个字节算过了」和那一刻的状态）。
#[derive(Default)]
pub struct Hashed {
    last: Fragments,
    points: Vec<Point>,
}

impl fmt::Debug for Hashed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Hashed")
            .field("last", &self.last.len())
            .field("points", &self.points.len())
            .finish()
    }
}

impl Hashed {
    /// 这一次的请求体 `next` 从哪接着算：和上一次一样的那一截末尾以前最近的那个状态（前多少个字节算过了、那一刻的状态），
    /// 连同到那里为止的中间状态（这一次照样用得上，接着往后留）。一样的不到第一个状态的，从头算：`None`，什么都不留。
    pub fn resume(&self, next: &Fragments) -> (Option<Point>, Vec<Point>) {
        let same = self.last.common_len(next);
        let kept: Vec<Point> = self
            .points
            .iter()
            .take_while(|(at, _)| *at <= same)
            .cloned()
            .collect();
        (kept.last().cloned(), kept)
    }

    /// 记下这一次的请求体和算到的中间状态，下一次照它接着算。
    pub fn remember(&mut self, body: Fragments, points: Vec<Point>) {
        self.last = body;
        self.points = points;
    }
}

#[cfg(test)]
mod tests;
