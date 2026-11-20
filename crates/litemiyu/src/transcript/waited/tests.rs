//! 等人的那几段（Miyu 终端 `transcript/waited/tests.rs`，事件照 LiteMiyu 读成的样子造）。

use std::time::{Duration, Instant};

use serde_json::json;

use super::Waited;

use crate::core::Asking;

fn at(t0: Instant, secs: u64) -> Instant {
    t0 + Duration::from_secs(secs)
}

/// 测试里的调用编号 `id` 写成内核的样子：`call_<id>_1`（`id` 是数字）。
pub(in crate::transcript) fn call(id: u32) -> String {
    format!("call_{id}_1")
}

/// 照 `kind` 造一条确认、提问的事件，调用编号是 [`call`] 那样的。
pub(in crate::transcript) fn asking(kind: &str, id: u32) -> Asking {
    fn event<T: serde::de::DeserializeOwned>(body: serde_json::Value) -> T {
        serde_json::from_value(body).expect("事件读得懂")
    }
    match kind {
        "approval" => Asking::Approval(event(json!({"call_id": call(id), "access": "write"}))),
        "decided" => Asking::Decided(event(json!({"call_id": call(id), "decision": "once"}))),
        "asked" => Asking::Asked(event(json!({"call_id": call(id), "questions": []}))),
        _ => Asking::Answered(event(json!({"call_id": call(id), "answers": []}))),
    }
}

#[test]
fn waiting_on_a_person_counts_from_the_ask_until_answered_or_a_result() {
    let t0 = Instant::now();
    let mut w = Waited::default();
    w.asking(&asking("approval", 1), at(t0, 2));
    assert!(w.waiting());
    assert_eq!(
        w.total(at(t0, 5)),
        Duration::from_secs(3),
        "还在等的算到现在"
    );
    w.asking(&asking("decided", 1), at(t0, 7));
    assert!(!w.waiting());
    assert_eq!(w.total(at(t0, 30)), Duration::from_secs(5), "定了就不再算");
    // 提问没等到答，那一次调用先有了结果（别处答的、超时的）。
    w.asking(&asking("asked", 3), at(t0, 40));
    w.end(&call(3), at(t0, 41));
    assert_eq!(w.total(at(t0, 50)), Duration::from_secs(6));
}

#[test]
fn overlapping_waits_count_once() {
    let t0 = Instant::now();
    let mut w = Waited::default();
    w.asking(&asking("approval", 1), at(t0, 0));
    w.asking(&asking("approval", 2), at(t0, 2));
    w.asking(&asking("decided", 1), at(t0, 4));
    assert!(w.waiting(), "b 还在等");
    w.asking(&asking("decided", 2), at(t0, 6));
    assert_eq!(w.total(at(t0, 9)), Duration::from_secs(6));
    // 不认识的调用（这一轮开始前问的）有了结果：不动。
    w.end(&call(9), at(t0, 10));
    assert_eq!(w.total(at(t0, 10)), Duration::from_secs(6));
    w.reset();
    assert_eq!(w.total(at(t0, 11)), Duration::ZERO, "新的一轮从零算");
}
