//! 占有和概要（L7 一）：同一个会话第二把锁拿不到，放下了再拿得到；`/proc/locks` 认得出开着、忙着；概要写了读得回，写坏了的、
//! 写到一半的临时文件当没有；还没有概要的第一轮结束写一次，之后只在标题变了时写；会话目录不在的列出来是空的。

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use lm_kernel::kernel::block::{Block, Text};
use lm_kernel::kernel::event::{
    Body, EndReason, MessageUser, MetaChanged, TurnEnded, TurnStarted, TypedCommand,
};
use lm_kernel::kernel::id::Seq;
use lm_kernel::kernel::origin::By;

use super::lock::{self, BUSY, Claim, LOCK};
use super::summary::{self, Summary, Tracker};
use super::*;
use crate::store::paths::Paths;

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Scratch {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("litemiyu-window-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("建得了临时目录");
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

fn summary(title: &str) -> Summary {
    Summary {
        title: title.to_string(),
        cwd: "~/src".to_string(),
        last_active: lm_kernel::kernel::time::Timestamp::parse("2026-11-12T08:00:00.000Z")
            .expect("合写法"),
        turns: 3,
        preview: Some(String::new()),
        agents: None,
    }
}

#[test]
fn a_second_claim_is_refused_until_the_first_is_dropped() {
    let scratch = Scratch::new();
    let first = Claim::take(&scratch.0).expect("拿得到");
    assert!(
        matches!(Claim::take(&scratch.0), Err(Claimed::Elsewhere)),
        "第二个窗口打不开"
    );
    assert!(lock::held_by_other(&scratch.0, LOCK), "别的窗口看得出开着");
    assert!(!lock::held_by_other(&scratch.0, BUSY), "还没忙");
    drop(first);
    assert!(!lock::held_by_other(&scratch.0, LOCK), "放下了就没了");
    let again = Claim::take(&scratch.0).expect("放下了再拿得到");
    drop(again);
}

#[test]
fn busy_follows_the_turn() {
    let scratch = Scratch::new();
    let mut claim = Claim::take(&scratch.0).expect("拿得到");
    claim.set_busy(true);
    assert!(lock::held_by_other(&scratch.0, BUSY), "忙着");
    claim.set_busy(true);
    claim.set_busy(false);
    assert!(!lock::held_by_other(&scratch.0, BUSY), "闲了");
}

#[test]
fn a_claim_in_a_missing_folder_is_a_disk_error() {
    let scratch = Scratch::new();
    assert!(matches!(
        Claim::take(&scratch.0.join("gone")),
        Err(Claimed::Disk(_))
    ));
}

#[test]
fn the_summary_round_trips_and_a_broken_one_counts_as_none() {
    let scratch = Scratch::new();
    assert_eq!(summary::read(&scratch.0), None, "没有");
    summary::write(&scratch.0, &summary("修 CI")).expect("写得进");
    assert_eq!(summary::read(&scratch.0), Some(summary("修 CI")));
    std::fs::write(scratch.0.join(summary::FILE), "{\"title\":\"半").expect("写得进");
    assert_eq!(summary::read(&scratch.0), None, "写坏了的当没有");
    assert!(
        summary::from_log(&scratch.0).is_none(),
        "没有日志的也算不出来"
    );
}

#[test]
fn no_sessions_folder_lists_nothing() {
    let scratch = Scratch::new();
    let paths = Paths::under(&scratch.0);
    assert_eq!(list(&paths).expect("列得出"), Vec::new());
}

/// 第 `seq` 条事件，时刻照 [`summary`] 的。
fn event(seq: u64, body: Body) -> Event {
    Event {
        seq: Seq::new(seq).expect("不是 0"),
        at: summary("").last_active,
        turn: None,
        by: By::Kernel,
        cause: None,
        body,
    }
}

/// 一整轮：开始、结束。
fn turn(seq: u64) -> [Event; 2] {
    let started = TurnStarted {
        trigger: None,
        cwd: None,
        dirs: Vec::new(),
    };
    let ended = TurnEnded {
        reason: EndReason::Completed,
    };
    [
        event(seq, Body::TurnStarted(started)),
        event(seq + 1, Body::TurnEnded(ended)),
    ]
}

#[test]
fn a_summary_not_yet_on_disk_is_written_once_when_the_first_turn_ends() {
    // 没起标题的会话（自动起标题砍了以后，多半都这样）：第一轮结束写一次，不用等到标题变了、会话结束。
    let mut fresh = Tracker::new(summary(""), false);
    let [started, ended] = turn(2);
    assert!(!fresh.saw(&[started]), "一轮还没结束不写");
    assert!(fresh.saw(&[ended]), "第一轮结束写一次");
    assert!(!fresh.saw(&turn(4)), "写过了，以后一轮一轮不写");
    let titled = MetaChanged {
        title: Some("修 CI".to_string()),
        pinned: None,
    };
    assert!(
        fresh.saw(&[event(6, Body::MetaChanged(titled))]),
        "标题变了写"
    );
    assert_eq!(fresh.summary().turns, 5, "轮数照样跟着记");
    // 已经有概要文件的：一轮结束不写。
    let mut kept = Tracker::new(summary("修 CI"), true);
    assert!(!kept.saw(&turn(2)), "有了的一轮结束不写");
}

/// 一块字。
fn text(text: &str) -> Block {
    Block::Text(Text {
        text: text.to_string(),
    })
}

/// 人说的一句：几块。
fn said(seq: u64, blocks: Vec<Block>, typed: Option<&str>) -> Event {
    event(
        seq,
        Body::MessageUser(MessageUser {
            blocks,
            command: typed.map(|text| TypedCommand {
                name: "review".to_string(),
                text: text.to_string(),
            }),
            shell: false,
        }),
    )
}

/// 预览（引擎 十，照 Miyu 核心 `index/tests.rs` 的 `the_preview_is_the_first_line_of_the_first_words`）：跳过开头的空行、去掉
/// 前后空白、截到 50 个字；只有附件的不算，第二句盖不过第一句；斜杠命令展开的照人敲的那一行；旧概要（没有这一格）不拿之后的话
/// 当预览。预览变了不另写概要。
#[test]
fn the_preview_is_the_first_line_of_the_first_words() {
    let mut fresh = Tracker::new(summary(""), false);
    let file: Block = serde_json::from_str(
        r#"{"type":"file","blob":"sha256:97f5f58cebf9e368ddcc668976ce5da07ceb80c7be52ac6d4edcf2ac8a639894","name":"a.pdf","media_type":"application/pdf"}"#,
    )
    .expect("文件块");
    assert!(!fresh.saw(&[said(2, vec![file], None)]));
    assert_eq!(
        fresh.summary().preview.as_deref(),
        Some(""),
        "只有附件的不算"
    );
    let long = "长".repeat(60);
    assert!(
        !fresh.saw(&[said(3, vec![text(&format!("\n  {long}  \n第二行"))], None)]),
        "预览变了不另写"
    );
    assert_eq!(fresh.summary().preview, Some("长".repeat(50)));
    fresh.saw(&[said(4, vec![text("后来的")], None)]);
    assert_eq!(
        fresh.summary().preview,
        Some("长".repeat(50)),
        "第二句盖不过第一句"
    );

    let mut typed = Tracker::new(summary(""), false);
    typed.saw(&[said(2, vec![text("展开的模板\n很长")], Some("/review src"))]);
    assert_eq!(
        typed.summary().preview.as_deref(),
        Some("/review src"),
        "照人敲的"
    );

    let mut old = Tracker::new(
        Summary {
            preview: None,
            ..summary("")
        },
        true,
    );
    old.saw(&[said(2, vec![text("后来的")], None)]);
    assert_eq!(old.summary().preview, None, "旧概要不拿之后的话当预览");
}

/// 删会话（`/delete`、`/sessions` 里 `Ctrl+D`，LM-2 砍了回收站）：拿着锁直接删掉整个目录，家目录里不出现回收站；别的窗口开着的
/// 不删，目录原样在。
#[test]
fn removing_a_session_deletes_its_folder_outright() {
    let scratch = Scratch::new();
    let paths = Paths::under(&scratch.0);
    let home = scratch.0.join("home");
    std::fs::create_dir_all(&home).expect("建得了家目录");
    let id = "0192f3a0-1111-7abc-8def-001122334455";
    let dir = paths.sessions().join(id);
    std::fs::create_dir_all(dir.join("inbox")).expect("建得了会话目录");
    std::fs::write(dir.join("000000000001.jsonl"), "{}\n").expect("写得进");

    let open = Claim::take(&dir).expect("拿得到");
    assert!(
        matches!(remove(&paths, id), Err(NotRemoved::Open)),
        "别的窗口开着的不删"
    );
    assert!(dir.join("000000000001.jsonl").exists(), "目录原样在");
    drop(open);

    remove(&paths, id).expect("删得了");
    assert!(!dir.exists(), "会话目录没了");
    assert!(
        !home.join(".local/share/Trash").exists(),
        "不经回收站：家目录里没有回收站"
    );
    assert!(
        matches!(remove(&paths, id), Err(NotRemoved::Missing)),
        "删过的再删说没有"
    );
}
