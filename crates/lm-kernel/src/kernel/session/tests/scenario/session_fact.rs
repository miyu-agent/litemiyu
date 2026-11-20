//! 场景：会话编号那一块事实（施工 1-13 再补，`docs/blueprint/kernel/request.md`「事实」）。照环境的规矩注入：第一轮
//! 注入，之后的轮和最近那一块一样、不注入；压缩以后、撤掉带着它的那一轮以后，下一个边界重新注入；载入以后照旧是这个
//! 编号，不重发；子会话写它自己的编号；以前造的快照没有这份模板，不注入。

use super::upward::PARENT;
use super::*;
use crate::kernel::session::Compaction;
use crate::kernel::testkit::{CHILD_SESSION, SESSION};

/// 替身的策略加上会话编号的模板：短，一眼认得出是哪个字段。
fn with_session() -> Policy {
    let mut policy = policy();
    policy.facts = FactTemplates::new(
        r#"<e t="{time}" z="{timezone}" d="{cwd}"/>"#,
        "<reply-cut/>",
        Some(r#"<s i="{id}"/>"#),
    )
    .unwrap();
    policy
}

/// 一个带会话编号模板的替身。
fn stage_with_session() -> Stage {
    Stage::new(with_session, environment("~/src/miyu"), at(0))
}

/// 日志里内核注入的会话编号那几块的原文，照先后。
fn session_facts(stage: &Stage) -> Vec<&str> {
    stage
        .log()
        .iter()
        .filter_map(|event| match &event.body {
            Body::ContextInjected(fact) if fact.kind.as_str() == "session" => {
                Some(fact.text.as_str())
            }
            _ => None,
        })
        .collect()
}

/// 编号 `id` 那一块的原文。
fn block(id: &str) -> String {
    format!(r#"<s i="{id}"/>"#)
}

#[test]
fn the_first_turn_injects_the_session_id_after_the_env() {
    let mut stage = stage_with_session();
    stage.model([Line::says("好。"), Line::says("嗯。")]);
    stage.say("hi");
    assert_eq!(
        story(&stage)[..6],
        [
            "1 session.created person",
            "2 message.user person",
            "3 turn.started kernel t3",
            "4 context.injected:env kernel t3",
            "5 context.injected:session kernel t3",
            "6 message.assistant model t3",
        ]
    );
    assert_eq!(session_facts(&stage), [block(SESSION)]);
    // 第二轮：编号没变，和最近那一块一样，不注入。
    stage.say("再说一句");
    assert_eq!(session_facts(&stage), [block(SESSION)]);
    assert!(
        !story(&stage)[8..]
            .iter()
            .any(|line| line.contains("context.injected")),
        "{:?}",
        story(&stage)
    );
}

#[test]
fn after_a_compaction_it_is_injected_once_more() {
    let make = || {
        let mut policy = with_session();
        policy.compaction = Some(Compaction {
            reserve_cap: 10,
            margin: 10,
            line_percent: 100,
            margin_percent: 100,
            tail: 0,
            lead: 0,
            price: crate::kernel::estimate::Flat {
                image: 50,
                file: 50,
            },
            rebuild: None,
            pause: None,
            shorten: None,
            isolate: false,
        });
        policy
    };
    let mut stage = Stage::new(make, environment("~/src/miyu"), at(0));
    stage.model([Line::says("好。")]);
    stage.say("hi");
    super::compaction::line(&mut stage, 100);
    stage.model([Line::says("S1"), Line::says("嗯。")]);
    stage.say("再说一句");
    assert_eq!(
        story(&stage)[9..],
        [
            "10 turn.started kernel t10",
            "11 model.called:ok kernel t10",
            "12 context.compacted kernel t10",
            "13 context.injected:env kernel t10",
            "14 context.injected:session kernel t10",
            "15 message.assistant model t10",
            "16 model.called:ok kernel t10",
            "17 turn.ended:completed kernel t10",
        ]
    );
    assert_eq!(session_facts(&stage), [block(SESSION), block(SESSION)]);
}

#[test]
fn after_undoing_the_turn_that_carried_it_the_next_turn_injects_it_again() {
    let mut stage = stage_with_session();
    stage.model([Line::says("好。"), Line::says("嗯。"), Line::says("在。")]);
    stage.say("hi");
    stage.say("再说一句");
    assert_eq!(session_facts(&stage).len(), 1);
    let undone = stage.revert(TurnId::new(seq(3)));
    assert!(matches!(
        stage.outcome(&undone),
        Some(Outcome::Accepted { .. })
    ));
    stage.say("还在吗");
    assert_eq!(
        session_facts(&stage),
        [block(SESSION), block(SESSION)],
        "撤掉了带着它的那一轮，她看不到编号了：重新注入"
    );
}

#[test]
fn after_loading_it_is_the_same_id_and_not_sent_again() {
    let mut stage = stage_with_session();
    stage.model([Line::says("好。"), Line::says("嗯。")]);
    stage.say("hi");
    stage.crash();
    stage.say("再说一句");
    assert_eq!(session_facts(&stage), [block(SESSION)]);
}

#[test]
fn a_child_session_injects_its_own_id() {
    let mut stage = Stage::child(with_session, environment("~/src/miyu"), at(0), PARENT);
    stage.model([Line::says("好。")]);
    stage.say("做这件事");
    assert_eq!(session_facts(&stage), [block(CHILD_SESSION)]);
    assert_ne!(CHILD_SESSION, PARENT);
}

#[test]
fn older_snapshots_without_the_template_inject_no_session_block() {
    let without = || {
        let mut policy = policy();
        policy.facts = FactTemplates::new(
            r#"<e t="{time}" z="{timezone}" d="{cwd}"/>"#,
            "<reply-cut/>",
            None,
        )
        .unwrap();
        policy
    };
    let mut stage = Stage::new(without, environment("~/src/miyu"), at(0));
    stage.model([Line::says("好。")]);
    stage.say("hi");
    assert!(session_facts(&stage).is_empty());
    assert_eq!(story(&stage)[3], "4 context.injected:env kernel t3");
    assert_eq!(story(&stage)[4], "5 message.assistant model t3");
}
