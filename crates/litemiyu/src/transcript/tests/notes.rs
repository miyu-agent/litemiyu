//! 插进正文的信息（回顾、换模型这些旁白）：前面那一段还在进行就先收起，信息画在它下面，接着的步另起一段（2026-10-02 项目
//! 主人报：回报直接插进展开的一段里，连接线断、前面那段一直不收）。一轮进行中来的回报（后台任务、子代理结束这些）改成那一段
//! 里的一步，不切断（Miyu 发版前 check 21，2026-11-09 项目主人挑的样板 A，`transcript/tests/notes.rs`）；一轮外来的照旧单独一行。

use super::super::{Kind, MarkTone, ReportClass, Segment, StepKind, Transcript};
use super::apply;
use crate::core::{Block, EndReason, Push};

/// 开一轮、跑一条命令：时间线的一段在进行中。
fn started_with_command(t: &mut Transcript) {
    apply(
        t,
        vec![
            Push::TurnStarted(1, None),
            Push::BlockStart {
                index: 0,
                block: Block::ToolCall("shell".into()),
            },
            Push::Delta {
                index: 0,
                text: "{\"command\":\"sleep 3\",\"description\":\"等三秒\"}".into(),
            },
            Push::BlockEnd(0),
            Push::Calls(vec!["c1".into()]),
        ],
    );
}

/// 正文里每一段（照先后）。
fn segments(t: &Transcript) -> Vec<&Segment> {
    t.entries
        .iter()
        .filter_map(|e| e.segment.as_ref())
        .collect()
}

#[test]
fn a_job_notice_inside_a_turn_is_a_step_of_the_live_segment() {
    let mut t = Transcript::default();
    started_with_command(&mut t);
    let id = t.marked_from(
        ("● ".into(), MarkTone::Good),
        "后台命令完成 · sleep 3 · 3s".into(),
        "sleep 3".into(),
        ReportClass::Command,
    );
    assert!(!segments(&t)[0].finished, "回报不切断：那一段还在进行");
    assert!(t.entries.iter().all(|e| !e.report), "不另起一行");
    // 输出读回来照编号补进那一步。
    t.fill_report(id, "done".into());
    apply(
        &mut t,
        vec![
            Push::BlockStart {
                index: 1,
                block: Block::Reasoning,
            },
            Push::Delta {
                index: 1,
                text: "接着来".into(),
            },
            Push::BlockEnd(1),
            Push::TurnEnded(EndReason::Completed),
        ],
    );
    let segs = segments(&t);
    assert_eq!(segs.len(), 1, "回报后面的步还在同一段");
    let kinds: Vec<&StepKind> = segs[0].steps.iter().map(|s| &s.kind).collect();
    let StepKind::Report { note, class, .. } = kinds[1] else {
        panic!("回报是第二步：{kinds:?}");
    };
    assert_eq!(*class, ReportClass::Command);
    assert_eq!(note.detail, "done");
    assert!(matches!(kinds[2], StepKind::Thought { .. }), "后面的步接着");
    assert!(!segs[0].steps[1].busy(), "回报不转圈");
}

#[test]
fn a_job_notice_after_the_turn_is_its_own_line() {
    let mut t = Transcript::default();
    started_with_command(&mut t);
    apply(&mut t, vec![Push::TurnEnded(EndReason::Completed)]);
    t.marked_with(
        "● ".into(),
        MarkTone::Good,
        "后台任务完成 · 查 · 9s".into(),
        String::new(),
    );
    assert!(t.entries.last().is_some_and(|e| e.report), "单独一行");
    assert_eq!(segments(&t)[0].steps.len(), 1, "不并进上一轮");
}

#[test]
fn a_denied_reason_inside_a_turn_stays_its_own_line() {
    // 抽屉里被拒的理由是人对那一步的回答，不是回报（Miyu `job_line`）：一轮进行中也单独一行，前面那段先收起。
    let mut t = Transcript::default();
    started_with_command(&mut t);
    t.marked_line("✗ ".into(), MarkTone::Bad, "不允许".into(), String::new());
    assert!(t.entries.last().is_some_and(|e| e.report), "单独一行");
    assert!(segments(&t)[0].finished, "前面那段收起");
}

#[test]
fn a_note_folds_the_live_segment_before_it() {
    // 换模型、回顾这类旁白走同一个 `note`，也一样。
    let mut t = Transcript::default();
    started_with_command(&mut t);
    t.note(Kind::Note, "↻ 换了模型".into());
    assert!(segments(&t)[0].finished, "旁白前面那一段先收起");
    assert_eq!(t.entries[1].kind, Kind::Note);
    // 没有在进行的那一段时，旁白照常在末尾，不凭空造段。
    t.note(Kind::Note, "又一句".into());
    assert_eq!(t.entries.len(), 3);
    assert_eq!(segments(&t).len(), 1);
}
