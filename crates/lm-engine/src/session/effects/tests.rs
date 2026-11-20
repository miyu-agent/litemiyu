//! 效果的测试（施工 4-6 上）：改前改后的小内容写在效果里、大的和不是 UTF-8 的存成 blob（施工 V-18），事件里都有哈希；读的、删的照样换过去；她看过的：读过的
//! 记整份的哈希，写过的记改后的，删了的拿掉，后来的盖前面的；从日志里重建的和一路记下来的一样；存不下来的照样有哈希。

use std::sync::atomic::{AtomicU64, Ordering};

use lm_kernel::kernel::event::{FileTrashed, ToolResult};
use lm_kernel::kernel::id::{CallId, Seq, TurnId};
use lm_kernel::kernel::origin::{By, Tool};
use lm_kernel::kernel::time::Timestamp;

use super::*;

/// 一个用完就删的临时目录。
struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Scratch {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("miyu-effects-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }
}

impl Drop for Scratch {
    #[expect(
        clippy::let_underscore_must_use,
        reason = "删不掉就留在临时目录里，不影响测试"
    )]
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn small_changed_contents_ride_in_the_effect_and_the_rest_pass_through() {
    let scratch = Scratch::new();
    let blobs = Blobs::new(scratch.0.join("blobs"));
    let path = scratch.0.join("a.txt");
    let effects = store(
        &blobs,
        vec![
            crate::tool::Effect::Read {
                path: path.clone(),
                lines: Some([1, 2]),
                hash: ContentHash::of(b"old\n"),
            },
            crate::tool::Effect::Changed {
                path: path.clone(),
                before: Some(b"old\n".to_vec()),
                after: b"new\n".to_vec(),
            },
            crate::tool::Effect::Changed {
                path: scratch.0.join("b.txt"),
                before: None,
                after: b"b\n".to_vec(),
            },
        ],
    );
    let text = path.to_string_lossy().into_owned();
    assert_eq!(
        effects,
        [
            Effect::FileRead(FileRead {
                path: text.clone(),
                lines: Some([1, 2]),
                hash: ContentHash::of(b"old\n"),
                skill: None,
                display_name: Default::default(),
            }),
            Effect::FileChanged(FileChanged {
                path: text,
                before: Some(ContentHash::of(b"old\n")),
                after: ContentHash::of(b"new\n"),
                before_text: Some("old\n".to_string()),
                after_text: Some("new\n".to_string()),
            }),
            Effect::FileChanged(FileChanged {
                path: scratch.0.join("b.txt").to_string_lossy().into_owned(),
                before: None,
                after: ContentHash::of(b"b\n"),
                before_text: None,
                after_text: Some("b\n".to_string()),
            }),
        ]
    );
    // 写进了效果的不另存 blob：连 blob 目录都没建。
    assert!(!scratch.0.join("blobs").exists(), "不该建 blob 目录");
}

/// 改成 `after` 的那一个效果，存进 `blobs`。
fn stored_one(blobs: &Blobs, before: Option<&[u8]>, after: &[u8]) -> FileChanged {
    let effects = store(
        blobs,
        vec![crate::tool::Effect::Changed {
            path: PathBuf::from("/x"),
            before: before.map(<[u8]>::to_vec),
            after: after.to_vec(),
        }],
    );
    match effects.into_iter().next() {
        Some(Effect::FileChanged(changed)) => changed,
        other => panic!("{other:?}"),
    }
}

#[test]
fn contents_up_to_the_limit_ride_along_and_bigger_ones_become_blobs() {
    let scratch = Scratch::new();
    let blobs = Blobs::new(scratch.0.join("blobs"));
    assert_eq!(
        INLINE_MOST,
        32 * 1024,
        "图纸写的是 32 KiB（session/tools.md 第 2 条）"
    );
    let most = "a".repeat(INLINE_MOST);
    let over = "b".repeat(INLINE_MOST + 1);
    let changed = stored_one(&blobs, Some(most.as_bytes()), over.as_bytes());
    assert_eq!(
        changed.before_text.as_deref(),
        Some(most.as_str()),
        "正好到上限的写进去"
    );
    assert_eq!(changed.after_text, None, "多一个字节的不写");
    assert_eq!(changed.before, Some(ContentHash::of(most.as_bytes())));
    assert_eq!(changed.after, ContentHash::of(over.as_bytes()));
    assert_eq!(
        blobs.get(&changed.after).unwrap(),
        over.as_bytes(),
        "大的存成了 blob"
    );
    assert!(
        blobs.get(&ContentHash::of(most.as_bytes())).is_err(),
        "写进去的那一边不存 blob"
    );
}

#[test]
fn contents_that_are_not_utf8_become_blobs() {
    let scratch = Scratch::new();
    let blobs = Blobs::new(scratch.0.join("blobs"));
    let bytes = [0xff, 0xfe, b'a'];
    let changed = stored_one(&blobs, Some(b"plain"), &bytes);
    assert_eq!(changed.before_text.as_deref(), Some("plain"));
    assert_eq!(changed.after_text, None);
    assert_eq!(blobs.get(&changed.after).unwrap(), bytes);
}

#[test]
fn contents_that_cannot_be_stored_still_have_their_hash() {
    let scratch = Scratch::new();
    // blob 目录的上一级是个文件：目录建不起来，存不进去。
    std::fs::write(scratch.0.join("file"), "x").unwrap();
    let blobs = Blobs::new(scratch.0.join("file").join("blobs"));
    // 不是 UTF-8 的才走 blob。
    let bytes = [0xff, b'\n'];
    let changed = stored_one(&blobs, None, &bytes);
    assert_eq!(changed.after, ContentHash::of(&bytes));
    assert_eq!(changed.after_text, None);
    assert!(blobs.get(&ContentHash::of(&bytes)).is_err(), "真的没存进去");
}

#[test]
fn rebuilding_what_she_saw_keeps_no_texts() {
    let mut scan = SeenScan::default();
    let effects = vec![Effect::FileChanged(FileChanged {
        path: "/a".to_string(),
        before: Some(ContentHash::of(b"x")),
        after: ContentHash::of(b"y"),
        before_text: Some("x".to_string()),
        after_text: Some("y".to_string()),
    })];
    scan.note(&result(2, effects));
    match &scan.results[..] {
        [(_, kept)] => match &kept[..] {
            [Effect::FileChanged(changed)] => {
                assert_eq!(changed.before_text, None);
                assert_eq!(changed.after_text, None);
                assert_eq!(changed.after, ContentHash::of(b"y"));
            }
            other => panic!("{other:?}"),
        },
        other => panic!("{other:?}"),
    }
    let mut seen = Seen::new();
    seen.insert(PathBuf::from("/a"), ContentHash::of(b"y"));
    assert_eq!(scan.seen(), seen);
}

/// 读过 `path`，内容是 `content`。
fn read(path: &str, content: &[u8]) -> Effect {
    Effect::FileRead(FileRead {
        path: path.to_string(),
        lines: None,
        hash: ContentHash::of(content),
        skill: None,
        display_name: Default::default(),
    })
}

/// 把 `path` 改成了 `content`。
fn changed(path: &str, content: &[u8]) -> Effect {
    Effect::FileChanged(FileChanged {
        path: path.to_string(),
        before: None,
        after: ContentHash::of(content),
        before_text: None,
        after_text: None,
    })
}

#[test]
fn she_saw_what_she_read_and_what_she_wrote() {
    let mut seen = Seen::new();
    saw(&mut seen, &[read("/w/a", b"a1"), read("/w/b", b"b1")]);
    saw(&mut seen, &[changed("/w/a", b"a2")]);
    let unknown: Effect =
        serde_json::from_str(r#"{"kind":"diagram.drawn","path":"/w/c"}"#).unwrap();
    saw(
        &mut seen,
        &[
            unknown,
            Effect::FileTrashed(FileTrashed {
                path: "/w/b".to_string(),
                trash: "t".to_string(),
            }),
        ],
    );
    assert_eq!(
        seen,
        Seen::from([(PathBuf::from("/w/a"), ContentHash::of(b"a2"))]),
        "后来写的盖过先读的；删了的不在了；不认识的不管"
    );
}

/// 第 `seq` 条：一条工具结果，带着 `effects`。
fn result(seq: u64, effects: Vec<Effect>) -> Event {
    let call_id = CallId::new(Seq::new(1).unwrap(), 1).unwrap();
    Event {
        seq: Seq::new(seq).unwrap(),
        at: Timestamp::from_unix_millis(0).unwrap(),
        turn: Some(TurnId::new(Seq::new(1).unwrap())),
        by: By::Tool(Tool { call_id }),
        cause: None,
        body: Body::ToolResult(Box::new(ToolResult {
            call_id,
            status: lm_kernel::kernel::event::ToolStatus::Ok,
            blocks: Vec::new(),
            duration_ms: None,
            human: None,
            effects,
            end_turn: false,
            sent: Vec::new(),
            display_name: Default::default(),
            detail: None,
        })),
    }
}

#[test]
fn what_she_saw_is_rebuilt_from_the_log() {
    let events = [
        result(2, vec![read("/w/a", b"a1")]),
        result(3, vec![changed("/w/a", b"a2"), read("/w/b", b"b1")]),
        result(4, Vec::new()),
    ];
    let mut live = Seen::new();
    for event in &events {
        if let Body::ToolResult(result) = &event.body {
            saw(&mut live, &result.effects);
        }
    }
    let rebuilt = seen_in(&events);
    assert_eq!(rebuilt, live);
    assert_eq!(
        rebuilt.get(Path::new("/w/a")),
        Some(&ContentHash::of(b"a2"))
    );
    assert_eq!(rebuilt.len(), 2);
}

/// 派出去的任务照原样换成内核的 `job.started`（施工 7-3），不是看过的文件。
#[test]
fn a_started_job_passes_through_and_is_not_a_seen_file() {
    use lm_kernel::kernel::event::{JobKind, JobStarted};
    let scratch = Scratch::new();
    let started = JobStarted {
        foreground: false,
        fork: false,
        job: lm_kernel::kernel::id::JobId::new(1).unwrap(),
        what: JobKind::Command,
        title: "跑测试".to_string(),
        session: None,
    };
    let effects = store(
        &Blobs::new(scratch.0.join("blobs")),
        vec![crate::tool::Effect::JobStarted(started.clone())],
    );
    assert_eq!(effects, [Effect::JobStarted(started)]);
    let mut seen = Seen::new();
    saw(&mut seen, &effects);
    assert!(seen.is_empty());
}
