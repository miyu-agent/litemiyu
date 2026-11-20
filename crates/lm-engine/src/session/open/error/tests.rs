//! 读不了的会话照原因分开说（Miyu 施工 YC-2、YC-6，照 `miyu-endpoint/src/refusal/unread/tests.rs` 改写）：这一刻读写出错的不再
//! 说成损坏；日志中间坏了一行的说坏在第几行、只能看坏行以前的。

use std::io;
use std::path::PathBuf;

use lm_kernel::kernel::id::ContentHash;

use super::*;

const HASH: &str = "sha256:97f5f58cebf9e368ddcc668976ce5da07ceb80c7be52ac6d4edcf2ac8a639894";

fn broken(line: usize) -> OpenError {
    OpenError::Broken {
        segment: PathBuf::from("000000000001.jsonl"),
        line,
        why: "not JSON".to_string(),
    }
}

fn too_many_files() -> io::Error {
    io::Error::other("Too many open files")
}

#[test]
fn loading_tells_a_passing_error_from_a_broken_session() {
    assert!(LoadError::Log(OpenError::Io(too_many_files())).passing());
    assert!(LoadError::Blob(BlobError::Io(too_many_files())).passing());
    assert!(!LoadError::Log(broken(93)).passing());
    assert!(!LoadError::NotCreated.passing());
    let hash = ContentHash::parse(HASH).expect("合写法");
    assert!(!LoadError::Blob(BlobError::Missing(hash)).passing());
    assert!(!LoadError::Log(OpenError::Missing(PathBuf::from("x"))).passing());
    let said = LoadError::Log(OpenError::Io(too_many_files())).to_string();
    assert!(
        said.starts_with("Session can't be read right now. Try again later."),
        "{said}"
    );
    assert!(said.contains("Too many open files"), "{said}");
}

#[test]
fn a_log_broken_after_the_first_event_is_read_only_at_that_line() {
    let error = LoadError::Log(broken(93));
    let at = error.broken_at().expect("坏在第 93 行");
    assert_eq!(
        (at.segment.as_str(), at.line, at.seq.get()),
        ("000000000001.jsonl", 93, 93)
    );
    let said = error.to_string();
    assert!(
        said.starts_with("The session log is broken at line 93;"),
        "{said}"
    );
    // 坏在第一条的：没有能看的，照损坏说。
    let first = LoadError::Log(broken(1));
    assert_eq!(first.broken_at(), None);
    assert!(
        first.to_string().starts_with("session log not opened:"),
        "{first}"
    );
    assert_eq!(LoadError::NotCreated.broken_at(), None);
}
