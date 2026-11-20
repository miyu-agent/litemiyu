//! 收成槽的对照（红线 5，内核线第三趟）：同一份剧本交给两个替身，一个歇着就把有效历史收成槽、要看历史时从「磁盘」
//! 读回放回（平常的样子，`testkit/shelf.rs`），一个一直留着整份（[`Stage::hold_history`]，红线 5 以前的样子）。两个交出来的每一次
//! 请求（主请求、摘要请求、转述图的请求）照内核的写法逐字节一样，照三家驱动编出来也一样，日志一样；收成槽的那个真收过、放回过。
//!
//! 剧本走到：多轮带工具、自动压缩前后、手动压缩、撤销又恢复、撤销了不恢复、重做、换模型、别的会话发来的话、循环叫醒、带图（看
//! 不了图的模型，转述成了的、没成的）、崩了再载入以后；子代理的会话另跑一份（人说的话都是父会话的交代）。

use crate::support::sight::{blind, picture};
use crate::support::{anthropic_wire, child_stage, lines, responses_wire, stage, summarizes, wire};
use lm_kernel::kernel::block::{Block, Text};
use lm_kernel::kernel::testkit::{Line, Play, Stage};

/// 别的会话。
const PEER: &str = "01a0d75d-2180-7a3c-9e41-5b7d2c8f6a99";

/// 一轮：调两件读的工具，再说完。
fn turn(s: &mut Stage) {
    s.model([
        Line::calls(
            "看看。",
            &[("read", r#"{"path":"a"}"#), ("read", r#"{"path":"b"}"#)],
        ),
        Line::says("好。"),
    ]);
    s.tools([Play::done("结果"), Play::done("结果")]);
}

/// 照剧本跑：`blind` 的是看不了图的模型。窗口小，长几轮就过线自动压缩。
fn play(s: &mut Stage, blind_model: bool) {
    summarizes(s);
    let window = Some(33_000 + 260);
    match blind_model {
        true => {
            s.vision([Some("A cat."), None, Some("A dog."), Some("A bird.")]);
            s.limits_with(blind(window));
        }
        false => s.limits(window, None),
    }
    for k in 0..14_u64 {
        turn(s);
        let words = format!("第 {k} 句话");
        match blind_model && k % 3 == 1 {
            true => s.send(vec![Block::Text(Text { text: words }), picture(k % 3)]),
            false => s.say(&words),
        };
        s.advance(3);
        let last = *s.turns().last().expect("开过回合");
        match k {
            2 => {
                s.revert(last);
                s.unrevert();
            }
            3 => {
                s.revert(last);
            }
            4 => {
                turn(s);
                s.redo(None);
            }
            5 => {
                s.configure("b/n");
            }
            6 => {
                s.model([Line::says("看到了。")]);
                s.peer_says(PEER, "那边的构建跑完了。");
            }
            7 => {
                s.request_compaction(None);
            }
            8 => {
                s.model([Line::says("我看一眼。")]);
                s.schedule_says("t1", "到点了，看一眼构建。");
            }
            10 => s.crash(),
            _ => {}
        }
    }
}

/// 照 `make` 造两个替身跑同一份剧本，一个收成槽、一个不收，比。
fn twins(make: fn() -> Stage, blind_model: bool) {
    let mut kept = make();
    kept.hold_history();
    let once = kept.shelvings();
    play(&mut kept, blind_model);
    let mut shelved = make();
    play(&mut shelved, blind_model);
    assert_eq!(kept.shelvings(), once, "不收的以后再没收过");
    assert!(!kept.shelved());
    assert!(
        shelved.shelvings() >= 14,
        "每一轮以后都歇着、收过：收了 {} 次",
        shelved.shelvings()
    );
    assert_eq!(lines(&kept), lines(&shelved), "日志一样");
    let canonical = |s: &Stage| -> Vec<Vec<u8>> {
        s.requests()
            .iter()
            .map(|(_, request)| request.canonical_bytes())
            .collect()
    };
    assert!(kept.requests().len() > 20, "剧本真跑起来了");
    assert_eq!(canonical(&kept), canonical(&shelved), "请求逐字节一样");
    for ((_, a), (_, b)) in kept.requests().iter().zip(shelved.requests()) {
        if a.messages.iter().all(|message| !has_image(message)) {
            assert!(wire(a).body == wire(b).body, "openai-chat 编出来一样");
            assert!(
                anthropic_wire(a).body == anthropic_wire(b).body,
                "anthropic 编出来一样"
            );
            assert!(
                responses_wire(a).body == responses_wire(b).body,
                "openai-responses 编出来一样"
            );
        }
    }
    let described = |s: &Stage| -> Vec<Vec<u8>> {
        s.describes()
            .iter()
            .map(|(_, request)| request.canonical_bytes())
            .collect()
    };
    assert_eq!(described(&kept), described(&shelved), "转述图的请求一样");
}

/// 编码的探针不带 blob：带图的请求只比内核的写法。
fn has_image(message: &lm_kernel::kernel::request::Message) -> bool {
    use lm_kernel::kernel::request::Message;
    let blocks = match message {
        Message::User { blocks } | Message::Assistant { blocks } | Message::Tool { blocks, .. } => {
            blocks
        }
    };
    blocks.iter().any(|block| matches!(block, Block::Image(_)))
}

#[test]
fn a_shelved_session_sends_what_a_whole_one_sends() {
    twins(stage, false);
}

#[test]
fn a_shelved_session_that_cannot_see_sends_what_a_whole_one_sends() {
    twins(stage, true);
}

#[test]
fn a_shelved_subagent_sends_what_a_whole_one_sends() {
    twins(child_stage, false);
}
