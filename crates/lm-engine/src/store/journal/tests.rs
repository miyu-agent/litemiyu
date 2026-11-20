//! 日志 `journal.jsonl`：`seq` 从 1 数起、接着数；打开时截掉最后那半行；一行一条，外壳照事件的写法、没有 `turn`；最后一行
//! 手改坏了的不往后写。

use std::fs;

use lm_kernel::kernel::id::{CommandId, EventKind};
use lm_kernel::kernel::origin::{By, Person};
use lm_kernel::kernel::raw::RawJson;
use lm_kernel::kernel::time::Timestamp;

use super::*;
use crate::store::test_support::Scratch;

/// 一条：第 `n` 次改。
fn entry(n: u32) -> Entry {
    Entry {
        at: Timestamp::parse("2026-10-01T08:00:00.000Z").unwrap(),
        by: By::Person(Person::default()),
        cause: Some(CommandId::parse(&format!("config-9f2c4e1a7b3d5f60-{n}")).unwrap()),
        kind: EventKind::parse("config.changed").unwrap(),
        body: serde_json::from_str::<RawJson>(&format!("{{\"n\":{n}}}")).unwrap(),
    }
}

fn lines(path: &std::path::Path) -> Vec<String> {
    fs::read_to_string(path)
        .unwrap()
        .lines()
        .map(str::to_string)
        .collect()
}

#[test]
fn entries_are_counted_from_one_and_written_like_events() {
    let temp = Scratch::new();
    let path = temp.path().join("sub").join(FILE);
    assert_eq!(append(&path, entry(1)).unwrap().get(), 1);
    assert_eq!(append(&path, entry(2)).unwrap().get(), 2);
    assert_eq!(
        lines(&path),
        [
            "{\"seq\":1,\"at\":\"2026-10-01T08:00:00.000Z\",\"kind\":\"config.changed\",\"by\":{\"kind\":\"person\"},\"cause\":\"config-9f2c4e1a7b3d5f60-1\",\"body\":{\"n\":1}}",
            "{\"seq\":2,\"at\":\"2026-10-01T08:00:00.000Z\",\"kind\":\"config.changed\",\"by\":{\"kind\":\"person\"},\"cause\":\"config-9f2c4e1a7b3d5f60-2\",\"body\":{\"n\":2}}",
        ]
    );
    let seen = Event::from_line(&lines(&path)[0]).unwrap();
    assert!(
        matches!(seen.body, Body::Unknown { .. }),
        "内核照不认识的种类留着"
    );
}

#[test]
fn a_half_written_last_line_is_cut_and_the_count_goes_on() {
    let temp = Scratch::new();
    let path = temp.path().join(FILE);
    append(&path, entry(1)).unwrap();
    let mut bytes = fs::read(&path).unwrap();
    bytes.extend_from_slice(b"{\"seq\":2,\"at\":");
    fs::write(&path, &bytes).unwrap();
    assert_eq!(append(&path, entry(3)).unwrap().get(), 2, "半行不算");
    let lines = lines(&path);
    assert_eq!(lines.len(), 2);
    assert_eq!(
        lines[1],
        "{\"seq\":2,\"at\":\"2026-10-01T08:00:00.000Z\",\"kind\":\"config.changed\",\"by\":{\"kind\":\"person\"},\"cause\":\"config-9f2c4e1a7b3d5f60-3\",\"body\":{\"n\":3}}",
        "半行截掉了，新的一行完整"
    );
}

#[test]
fn someone_elses_entries_are_counted_on_from() {
    let temp = Scratch::new();
    fs::create_dir_all(temp.path()).unwrap();
    let path = temp.path().join(FILE);
    fs::write(
        &path,
        "{\"seq\":7,\"at\":\"2026-10-01T08:00:00.000Z\",\"kind\":\"trust.changed\",\"by\":{\"kind\":\"kernel\"},\"body\":{}}\n",
    )
    .unwrap();
    assert_eq!(append(&path, entry(1)).unwrap().get(), 8);
}

#[test]
fn a_broken_last_line_stops_the_write() {
    let temp = Scratch::new();
    fs::create_dir_all(temp.path()).unwrap();
    let path = temp.path().join(FILE);
    fs::write(&path, "not json\n").unwrap();
    let error = append(&path, entry(1)).unwrap_err();
    assert!(error.to_string().contains("line 1"), "{error}");
    assert_eq!(fs::read_to_string(&path).unwrap(), "not json\n", "没往后写");
}

/// 审计 2026-11-18 #061（journal 那一半）：配置、密钥改动的日志只给自己读写（0600），同输入历史（`input/saved.rs`）、配置文件里
/// 有密钥的那种写法（`config_file.rs` 的 `Mode::Private`）。原来照系统默认建，umask 022 下是 0644。所在的状态目录新建时是
/// 0700（`durable.rs`），这一条是防目录是人自己建的、放宽了的。
#[test]
fn the_journal_is_created_readable_only_by_its_owner() {
    use std::os::unix::fs::PermissionsExt;
    let temp = Scratch::new();
    let path = temp.path().join(FILE);
    append(&path, entry(1)).unwrap();
    let mode = fs::metadata(&path).unwrap().permissions().mode();
    assert_eq!(mode & 0o077, 0, "别人读不了、写不了：{:o}", mode & 0o777);
}

/// 审计 2026-11-18 #061：已有的、别人读得到写得了的（旧版本照 0644 建的）追加时收紧成只给自己。
#[test]
fn an_existing_journal_open_to_others_is_tightened() {
    use std::os::unix::fs::PermissionsExt;
    let temp = Scratch::new();
    let path = temp.path().join(FILE);
    append(&path, entry(1)).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o666)).unwrap();
    append(&path, entry(2)).unwrap();
    let mode = fs::metadata(&path).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600, "收紧了：{:o}", mode & 0o777);
}
