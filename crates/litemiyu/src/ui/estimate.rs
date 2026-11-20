//! 放掉一段时没排过的条目估几行（红线 5，`app/window.rs`）：不排版，照字数和宽估，空档的行数、滚动条照它；读回来排好了换成真的。

use unicode_width::UnicodeWidthStr;

use crate::transcript::{Entry, Kind};

/// 第一条以外每条前面的空行也算在内：一条估几行、看得见的字有多宽（换了宽重估用）。
pub fn estimate(entry: &Entry, width: u16) -> (u32, u32) {
    let width = usize::from(width.max(1));
    let (rows, chars) = match entry.kind {
        // 收起的时间线、收尾行：一行。
        Kind::Steps | Kind::Done => (1, 0),
        _ => entry.text.lines().fold((0, 0), |(rows, chars), line| {
            let cols = UnicodeWidthStr::width(line);
            (rows + cols.div_ceil(width).max(1), chars + cols)
        }),
    };
    let rows = u32::try_from(rows + 1).unwrap_or(u32::MAX);
    (rows, u32::try_from(chars).unwrap_or(u32::MAX))
}

/// 照 `was` 宽量的（或估的）`rows` 行、`chars` 宽的字，换到 `now` 宽大约几行：字那一截照新宽重折，别的不变。
pub fn rewrap(rows: u32, chars: u32, was: u16, now: u16) -> u32 {
    if was == now || was == 0 || now == 0 {
        return rows;
    }
    let old = chars.div_ceil(u32::from(was));
    let new = chars.div_ceil(u32::from(now));
    (rows + new).saturating_sub(old).max(1)
}
