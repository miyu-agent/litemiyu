//! 别的会话发来的话一律收（L7 二）：Miyu 在这里照账本拒（施工 C-2，同一个发话方 600 秒里第 6 句、一字不差的、没听到的第 51 句）。
//! LiteMiyu 没有共同的核心，收的一方拒了发的一方不知道：防刷屏挪到了发的一方（`lm-engine` 的 `session/peers/outbox.rs`），
//! 内核读到的都收。父子之间的留言照旧不受它管。
//!
//! 用没人看着的一次性会话：话只记下、不开轮。

use super::peers::PEER;
use super::reports::{CHILD, dispatched};
use super::*;

/// 没人看着的一次性会话。
fn unwatched() -> Stage {
    Stage::oneshot(policy, environment("~/src/miyu"), at(0))
}

/// 这个命令收下了。
fn taken(s: &Stage, id: &CommandId) {
    assert!(
        matches!(s.outcome(id), Some(Outcome::Accepted { .. })),
        "{id}：{:?}",
        s.outcome(id)
    );
}

#[test]
fn messages_from_another_session_are_all_taken() {
    let mut s = unwatched();
    let before = s.log().len();
    for k in 0..60 {
        let id = s.peer_says(PEER, &format!("第 {k} 句"));
        taken(&s, &id);
    }
    let same = s.peer_says(PEER, "第 0 句");
    taken(&s, &same);
    assert_eq!(s.log().len(), before + 61, "一句记一条");
}

#[test]
fn a_subagent_is_not_held_to_five_messages() {
    let mut s = dispatched(stage());
    s.model([Line::says("好").held(), Line::says("收到。")]);
    s.say("接着查");
    for k in 0..7 {
        let id = s.child_says(CHILD, &format!("进度 {k}"));
        taken(&s, &id);
    }
    let same = s.child_says(CHILD, "进度 0");
    taken(&s, &same);
}
