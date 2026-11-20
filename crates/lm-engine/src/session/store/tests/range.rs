//! 读回照序号定位（施工 V-37 下）：撤掉压缩时读回第 `from` 条起的（`Store::events_from`），交回的、报的错都和原来一段段读、
//! 整个解完再挑的（照抄留在这里的 [`old_from`]）一样。`from` 以前的不解、不自检（2026-11-03 项目主人定）。Miyu 这里还比分叉
//! 出来的会话的底子（子代理 fork，L5 搬）。

use std::fs;
use std::path::{Path, PathBuf};

use crate::store::log::{SEGMENT_LIMIT, SessionLog, read_segments};
use lm_kernel::kernel::event::Event;
use lm_kernel::kernel::id::Seq;

use super::super::Store;

/// 数据根里的临时目录（会话日志只在数据根里开，施工 TMP-1）：用完删掉。
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let path =
            std::env::temp_dir().join(format!("miyu-session-range-{name}-{}", std::process::id()));
        drop(fs::remove_dir_all(&path));
        fs::create_dir_all(&path).unwrap();
        Scratch(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        drop(fs::remove_dir_all(&self.0));
    }
}

/// 第 `seq` 条的一行（带换行），正文长短照序号变。
fn line(seq: u64) -> String {
    let text = "字".repeat(usize::try_from(seq % 11).unwrap_or(0));
    format!(
        "{{\"seq\":{seq},\"at\":\"2026-11-03T07:00:00.000Z\",\"kind\":\"message.user\",\"by\":{{\"kind\":\"person\"}},\"body\":{{\"blocks\":[{{\"type\":\"text\",\"text\":\"第 {seq} {text}\"}}]}}}}\n"
    )
}

fn lines(from: u64, to: u64) -> String {
    (from..=to).map(line).collect()
}

fn write(dir: &Path, segments: &[(u64, String)]) {
    fs::create_dir_all(dir).unwrap();
    for (first, text) in segments {
        fs::write(dir.join(format!("{first:012}.jsonl")), text).unwrap();
    }
}

fn seq(n: u64) -> Seq {
    Seq::new(n).unwrap()
}

/// 原来的 `SessionLog` 的 `Store::events_from`（施工 6-9 起，V-37 下以前），照抄。
fn old_from(dir: &Path, from: Seq) -> Result<Vec<Event>, String> {
    let mut kept = Vec::new();
    read_segments(dir, |segment| {
        kept.extend(segment.into_iter().filter(|event| event.seq >= from));
        true
    })
    .map_err(|error| error.to_string())?;
    Ok(kept)
}

/// 两份交回的一样：事件逐个相等，报的错写出来一样。
fn alike<E: std::fmt::Display>(
    new: &Result<Vec<Event>, E>,
    old: &Result<Vec<Event>, E>,
    what: &str,
) {
    match (new, old) {
        (Ok(new), Ok(old)) => assert_eq!(new, old, "{what}"),
        (Err(new), Err(old)) => assert_eq!(new.to_string(), old.to_string(), "{what}"),
        (new, old) => panic!(
            "{what}：新的 {:?}，原来 {:?}",
            new.as_ref().map(Vec::len).map_err(ToString::to_string),
            old.as_ref().map(Vec::len).map_err(ToString::to_string)
        ),
    }
}

/// 三段夹一段空的，最后一段末尾有半行（打开时截掉）：读回第 `from` 条起的，每一处都和原来一样。
#[test]
fn reading_back_from_every_seq_matches_the_whole_walk() {
    let scratch = Scratch::new("from");
    let dir = scratch.0.join("s");
    let mut last = lines(61, 90);
    last.push_str(r#"{"seq":91,"at""#);
    write(
        &dir,
        &[
            (1, lines(1, 30)),
            (31, lines(31, 60)),
            (45, String::new()),
            (61, last),
        ],
    );
    let (log, _) = SessionLog::open(&dir, SEGMENT_LIMIT).unwrap();
    for from in 1..=93 {
        let new = Store::events_from(&log, seq(from));
        alike(&new, &old_from(&dir, seq(from)), &format!("从 {from} 起"));
    }
    // 分叉出来的会话（第一段从第 9 条起）：从分叉点以前读起的照样一样。
    let forked = scratch.0.join("f");
    write(&forked, &[(9, lines(9, 30)), (31, lines(31, 50))]);
    let (log, _) = SessionLog::open(&forked, SEGMENT_LIMIT).unwrap();
    for from in 1..=52 {
        let new = Store::events_from(&log, seq(from));
        alike(
            &new,
            &old_from(&forked, seq(from)),
            &format!("分叉的从 {from} 起"),
        );
    }
}

/// 坏的一行在读回的那一截里：报的错和原来一字不差；在 `from` 以前的不解、不报（原来要报，项目主人定的差别）。
#[test]
fn a_broken_line_before_from_is_skipped_and_one_inside_is_told_alike() {
    let scratch = Scratch::new("broken");
    let dir = scratch.0.join("s");
    write(&dir, &[(1, lines(1, 30)), (31, lines(31, 60))]);
    let (log, _) = SessionLog::open(&dir, SEGMENT_LIMIT).unwrap();
    // 开着以后第一段被外人改坏了一行（开的时候整份查过，坏了的开不了）。
    let first = lines(1, 30).replacen(&line(12), "{\"seq\":12,\n", 1);
    write(&dir, &[(1, first)]);
    for from in [1, 5, 11, 12] {
        let new = Store::events_from(&log, seq(from));
        alike(&new, &old_from(&dir, seq(from)), &format!("从 {from} 起"));
        assert_eq!(
            new.expect_err("坏的那一行在里面"),
            "000000000001.jsonl line 12 is broken"
        );
    }
    for from in [13, 20, 31, 45, 60] {
        assert!(old_from(&dir, seq(from)).is_err(), "原来整份解，都报");
        let got = Store::events_from(&log, seq(from)).unwrap();
        let seqs: Vec<u64> = got.iter().map(|event| event.seq.get()).collect();
        assert_eq!(seqs, (from..=60).collect::<Vec<_>>(), "从 {from} 起");
    }
}
