//! 读到最后、知道末尾在哪（占用线三，`read_range_ending`）：离末尾近的从末尾往回数，交出去的和从开头数的一样；跳过的那一截
//! 照旧不查；末尾记得不对（比文件长、少了几行、多了几行）的照从开头数，交的一样、不交错的。

use std::fs;

use super::{lines, picked, ranged, replace_line, seq, write};
use crate::store::log::{Flow, Mark, OpenError, read_range_ending};
use crate::store::test_support::Scratch;

/// `read_range_ending` 交出去的序号。
fn ending(dir: &std::path::Path, from: u64, end: &Mark) -> Result<Vec<u64>, OpenError> {
    let mut got = Vec::new();
    read_range_ending(dir, seq(from), end, |events| {
        assert!(!events.is_empty(), "不交空的一块");
        got.extend(events.iter().map(|event| event.seq.get()));
        Flow::Go
    })?;
    Ok(got)
}

/// 一段 `first..=last` 写成的末尾。
fn end_of(first: u64, last: u64) -> Mark {
    Mark {
        segment: first,
        bytes: lines(first, last).len() as u64,
        next: seq(last + 1),
    }
}

#[test]
fn every_place_reads_like_reading_from_the_start() {
    let scratch = Scratch::new();
    let dir = scratch.path().join("s");
    // 最后一段比一次往回读的一块（64 KiB）长得多：往回数要跨好几块。
    write(&dir, &[(1, lines(1, 40)), (41, lines(41, 3000))]);
    let end = end_of(41, 3000);
    for from in (1..=3001)
        .step_by(7)
        .chain([2998, 2999, 3000, 3001, 41, 42, 1520])
    {
        assert_eq!(
            ending(&dir, from, &end).unwrap(),
            ranged(&dir, from, None).unwrap(),
            "从 {from}"
        );
        assert_eq!(ending(&dir, from, &end).unwrap(), picked(&dir, from, None));
    }
}

#[test]
fn the_skipped_part_is_not_checked_from_the_end_either() {
    let scratch = Scratch::new();
    let dir = scratch.path().join("s");
    // 第 5 行坏了：从末尾往回数到第 90 条，前面的不解、不查（同从开头数）。
    write(&dir, &[(1, replace_line(&lines(1, 100), 5, "坏了"))]);
    assert_eq!(
        ending(&dir, 90, &end_of(1, 100)).unwrap(),
        (90..=100).collect::<Vec<_>>()
    );
}

#[test]
fn a_wrong_end_falls_back_to_counting_from_the_start() {
    let scratch = Scratch::new();
    let dir = scratch.path().join("s");
    let mut last = lines(1, 100);
    // 末尾一个没写完的半行：记着的末尾在它前面。
    last.extend_from_slice(br#"{"seq":101,"at""#);
    write(&dir, &[(1, last)]);
    let right = end_of(1, 100);
    assert_eq!(
        ending(&dir, 95, &right).unwrap(),
        (95..=100).collect::<Vec<_>>()
    );
    let wrong = [
        // 比文件还长。
        Mark {
            bytes: right.bytes + 4096,
            ..right
        },
        // 记着的少了两行、多了两行：往回数到的那一行不是第 95 条。
        Mark {
            next: seq(99),
            ..right
        },
        Mark {
            next: seq(103),
            ..right
        },
        // 记着的是别的一段。
        Mark {
            segment: 7,
            ..right
        },
    ];
    for end in wrong {
        assert_eq!(
            ending(&dir, 95, &end).unwrap(),
            (95..=100).collect::<Vec<_>>(),
            "{end:?}"
        );
    }
}

#[test]
fn nothing_past_the_end_and_a_missing_session_is_reported() {
    let scratch = Scratch::new();
    let dir = scratch.path().join("s");
    write(&dir, &[(1, lines(1, 10))]);
    assert_eq!(ending(&dir, 11, &end_of(1, 10)).unwrap(), Vec::<u64>::new());
    let gone = scratch.path().join("gone");
    fs::create_dir_all(&gone).unwrap();
    assert!(matches!(
        ending(&gone, 3, &end_of(1, 10)),
        Err(OpenError::Missing(_))
    ));
}
