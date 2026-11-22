//! 补发一块块办进草稿收的你说的话，和整份正文办完以后列的一样：排着的几句拼成一段、退回的不算、跑命令的照「! 命令」列、
//! 时刻照事件的。

use super::{Gather, Said};
use crate::config::Config;
use crate::core::{EndReason, Push, Update};
use crate::transcript::{Kind, Transcript};

fn clock(secs: i64) -> Push {
    Push::Clock(Some(
        jiff::Timestamp::from_second(1_800_000_000 + secs).expect("在范围里"),
    ))
}

/// 几轮：第 2 轮回答时排进两句（听到了，拼成一段），第 3 轮排进一句又撤回，第 4 句是 `!` 跑的命令。
fn pushes() -> Vec<Push> {
    let said = |seq: u64, text: &str| Push::Said {
        seq,
        text: text.to_string(),
    };
    let mut out = Vec::new();
    let turn = |out: &mut Vec<Push>, at: i64, seq: u64, inner: Vec<Push>| {
        out.push(clock(at));
        out.push(said(seq, &format!("第 {seq} 句")));
        out.push(Push::TurnStarted(seq + 1, Some(seq)));
        out.extend(inner);
        out.push(clock(at + 3));
        out.push(Push::TurnEnded(EndReason::Completed));
    };
    turn(&mut out, 0, 1, Vec::new());
    turn(
        &mut out,
        10,
        3,
        vec![
            clock(11),
            said(5, "排着的一句"),
            said(6, "排着的又一句"),
            Push::Heard(6),
        ],
    );
    turn(
        &mut out,
        20,
        7,
        vec![clock(21), said(9, "撤回的"), Push::Withdrawn(vec![9])],
    );
    out.push(clock(30));
    out.push(said(10, "ls"));
    out.push(Push::ShellAsked(10));
    turn(&mut out, 40, 11, Vec::new());
    out
}

#[test]
fn gathered_piece_by_piece_lists_what_the_whole_transcript_lists() {
    let config = Config::builtin().expect("出厂的配置读得懂");
    let texts = config.text();
    let mut whole = Transcript::default();
    let mut gather = Gather::new(std::sync::Arc::new(texts.clone()));
    for push in pushes() {
        whole.update(Update::Push(push.clone()), texts);
        gather.push(push);
    }
    let want: Vec<Said> = whole
        .entries
        .iter()
        .filter(|e| e.kind == Kind::User && !e.hidden)
        .map(|e| Said {
            seq: e.seq.expect("落了盘"),
            text: e.listed(),
            at: e.at,
        })
        .collect();
    let got = gather.finish();
    assert_eq!(got, want);
    let texts: Vec<&str> = got.iter().map(|s| s.text.as_str()).collect();
    assert_eq!(
        texts,
        [
            "第 1 句",
            "第 3 句",
            "排着的一句\n排着的又一句",
            "第 7 句",
            "! ls",
            "第 11 句"
        ]
    );
}
