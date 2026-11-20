//! 场景：人要求重起标题（施工 TT-1，`docs/blueprint/kernel/session.md`「起标题」）。请求喂最近几轮（`Assembler::retitle`）；
//! 收下就回，不等模型；人起过名的、一次性的会话照样起；有一次在路上的不再发；回合正在进行、还组装不出来的等这一轮完；空闲着
//! 没东西可起的拒绝；在路上时人又改了名的不盖掉；没起成的不留到下一轮；会话不自己起。标题取第一行、超了上限截掉。起好的标题
//! 记成要它的那个人起的（施工 TT-2），重启以后照样。

use super::*;
use crate::kernel::event::{ErrorClass, MetaChanged, Purpose};
use crate::kernel::session::{Reason, Titles};

/// 起标题的策略：标题最多 50 个字。
fn titled() -> Policy {
    Policy {
        titles: Some(Titles { chars: 50 }),
        ..policy()
    }
}

/// 照 [`titled`] 造的会话的替身。
fn titled_stage() -> Stage {
    Stage::new(titled, environment("~/src/miyu"), at(0))
}

/// 日志里改过的标题，照先后：是不是内核记的、改成了什么。
fn titles(stage: &Stage) -> Vec<(bool, String)> {
    stage
        .log()
        .iter()
        .filter_map(|event| match &event.body {
            Body::MetaChanged(MetaChanged {
                title: Some(title), ..
            }) => Some((event.by == By::Kernel, title.clone())),
            _ => None,
        })
        .collect()
}

/// 起标题的 `model.called` 有几条。
fn title_calls(stage: &Stage) -> usize {
    stage
        .model_calls()
        .into_iter()
        .filter(|called| called.purpose == Some(Purpose::Title))
        .count()
}

/// 说一句，她答一句就完。
fn one_turn(stage: &mut Stage, words: &str) {
    stage.model([Line::says("好。")]);
    stage.say(words);
}

/// 这个命令收下了，没等什么事件。
fn taken(stage: &Stage, id: &CommandId) {
    assert_eq!(
        stage.outcome(id),
        Some(&Outcome::Accepted { events: Vec::new() })
    );
}

#[test]
fn asking_feeds_the_latest_turns_and_replaces_a_title_a_person_gave() {
    let mut stage = titled_stage();
    for words in ["一", "二", "三", "四", "五"] {
        one_turn(&mut stage, words);
    }
    stage.set_meta(Some("人起的"), None);
    stage.title_model([Line::says("  重起的\n第二行不要").held()]);
    let asked = stage.retitle();
    taken(&stage, &asked);
    assert_eq!(title_calls(&stage), 0, "收下就回，模型还没说完");
    stage.release_title();
    assert_eq!(
        titles(&stage),
        [(false, "人起的".to_string()), (false, "重起的".to_string())]
    );
    let [(latest, again)] = stage.titles() else {
        panic!("一次起标题的请求");
    };
    // 替身的组装：最近三个回答的清单（`retitle`），没有第一轮。
    let answers: Vec<Seq> = stage
        .log()
        .iter()
        .filter(|event| matches!(event.body, Body::MessageAssistant(_)))
        .map(|event| event.seq)
        .collect();
    let [.., third, fourth, fifth] = answers[..] else {
        panic!("五个回答");
    };
    assert_eq!(*latest, fifth, "名字是最新那个回答");
    assert_eq!(
        listed_request(again),
        format!(
            "{third} message.assistant\n{fourth} message.assistant\n{fifth} message.assistant\nretitle\n"
        )
    );
    // 记账的那条是内核的；标题那条是要它的人起的（施工 TT-2），两条都不带回合编号、没有 `cause`。
    let [called, renamed] = &stage.log()[stage.log().len() - 2..] else {
        panic!("两条");
    };
    assert_eq!(called.by, By::Kernel, "{called:?}");
    assert!(matches!(renamed.by, By::Person(_)), "{renamed:?}");
    assert!(
        [called, renamed]
            .iter()
            .all(|event| event.turn.is_none() && event.cause.is_none()),
        "{called:?} {renamed:?}"
    );
    // 起过了就放下：下一轮答完不再起。
    one_turn(&mut stage, "sixth");
    assert_eq!(stage.titles().len(), 1);
}

/// 标题按字数截到 50 个、不加省略号，截完末尾的空白去掉。
#[test]
fn a_long_title_is_cut_at_the_limit_without_an_ellipsis() {
    let mut stage = titled_stage();
    one_turn(&mut stage, "hi");
    stage.title_model([Line::says(&format!("{} {}", "字".repeat(49), "尾巴"))]);
    stage.retitle();
    assert_eq!(titles(&stage), [(false, "字".repeat(49))]);
    let mut stage = titled_stage();
    one_turn(&mut stage, "hi");
    stage.title_model([Line::says(&"題".repeat(60))]);
    stage.retitle();
    assert_eq!(titles(&stage), [(false, "題".repeat(50))]);
}

#[test]
fn sessions_that_never_title_themselves_are_titled_when_asked() {
    for mut stage in [
        Stage::oneshot(titled, environment("~/src/miyu"), at(0)),
        titled_stage(),
    ] {
        stage.set_meta(Some("人起的"), None);
        one_turn(&mut stage, "hi");
        assert!(stage.titles().is_empty(), "自己不起");
        stage.title_model([Line::says("重起的")]);
        let asked = stage.retitle();
        taken(&stage, &asked);
        assert_eq!(titles(&stage).last(), Some(&(false, "重起的".to_string())));
    }
}

#[test]
fn with_nothing_to_title_it_is_refused_and_nothing_is_sent() {
    let mut stage = titled_stage();
    let asked = stage.retitle();
    assert_eq!(
        stage.outcome(&asked),
        Some(&Outcome::rejected(Reason::NothingToTitle))
    );
    // 策略里没有起标题的数的也是。
    let mut bare = Stage::new(policy, environment("~/src/miyu"), at(0));
    one_turn(&mut bare, "hi");
    let asked = bare.retitle();
    assert_eq!(
        bare.outcome(&asked),
        Some(&Outcome::rejected(Reason::NothingToTitle))
    );
    assert!(stage.titles().is_empty() && bare.titles().is_empty());
    // 拒了的不留着：后来答完的几轮不起。
    stage.title_model([Line::says("留着的")]);
    one_turn(&mut stage, "hi");
    one_turn(&mut stage, "again");
    assert!(stage.titles().is_empty());
}

#[test]
fn a_second_ask_joins_the_one_on_its_way() {
    let mut stage = titled_stage();
    one_turn(&mut stage, "hi");
    stage.set_meta(Some("人起的"), None);
    // 第一次没排剧本：一直在路上。
    let first = stage.retitle();
    assert_eq!(stage.titles().len(), 1);
    let second = stage.retitle();
    taken(&stage, &first);
    taken(&stage, &second);
    assert_eq!(stage.titles().len(), 1, "不再发");
}

#[test]
fn asked_during_the_first_turn_it_waits_for_the_turn_to_end() {
    let mut stage = Stage::oneshot(titled, environment("~/src/miyu"), at(0));
    stage.model([Line::says("好。").held()]);
    stage.say("hi");
    stage.title_model([Line::says("等完了起的")]);
    let asked = stage.retitle();
    taken(&stage, &asked);
    assert!(stage.titles().is_empty(), "这一轮还没答完，组装不出来");
    stage.release_model();
    assert_eq!(titles(&stage), [(false, "等完了起的".to_string())]);
    assert_eq!(stage.requests().len(), 1, "不打断这一轮");
}

/// 要了（施工 TT-2）：起好的算人起的，以后答完的轮不再起，重启以后也不起。
#[test]
fn a_title_asked_for_counts_as_the_persons_and_is_kept_after_a_restart() {
    let mut stage = titled_stage();
    stage.model([Line::says("好。").held()]);
    stage.say("hi");
    stage.title_model([Line::says("要了起的"), Line::says("又起的")]);
    stage.retitle();
    stage.release_model();
    assert_eq!(titles(&stage), [(false, "要了起的".to_string())]);
    one_turn(&mut stage, "again");
    stage.restart();
    one_turn(&mut stage, "after");
    assert_eq!(stage.titles().len(), 1, "不再起");
    assert_eq!(titles(&stage), [(false, "要了起的".to_string())]);
}

#[test]
fn a_person_renaming_after_asking_wins_and_a_failure_is_not_retried() {
    let mut stage = titled_stage();
    stage.set_meta(Some("人起的"), None);
    one_turn(&mut stage, "hi");
    stage.title_model([Line::says("重起的").held()]);
    stage.retitle();
    stage.set_meta(Some("又改了"), None);
    stage.release_title();
    assert_eq!(
        titles(&stage).last(),
        Some(&(false, "又改了".to_string())),
        "要了以后人又改的不盖掉"
    );
    stage.title_model([Line::fails(ErrorClass::Retryable, "503")]);
    stage.retitle();
    assert_eq!(title_calls(&stage), 2, "没起成的照记一条");
    one_turn(&mut stage, "again");
    assert_eq!(stage.titles().len(), 2, "不留到下一轮再试");
    assert_eq!(titles(&stage).last(), Some(&(false, "又改了".to_string())));
}

/// 告诉头的 `session.retitle_failed`（施工 TT-3；LiteMiyu 是 `Action::RetitleFailed`），照先后：谁要的、错的分类。
fn retitle_failures(stage: &Stage) -> Vec<(By, ErrorClass)> {
    stage
        .retitle_failures()
        .iter()
        .map(|(_, by, failed)| {
            assert_eq!(failed.human.key, "core/sessions/retitle-failed");
            (by.clone(), failed.error.class.clone())
        })
        .collect()
}

/// 人要的重起没起成（施工 TT-3）：出错、超时（会话那一层照出错送进来）、回复里取不出标题都告诉头，`by` 是要它的人，标题不动。
#[test]
fn a_retitle_that_fails_tells_the_heads_and_keeps_the_title() {
    for (line, class) in [
        (
            Line::fails(ErrorClass::Retryable, "503"),
            ErrorClass::Retryable,
        ),
        (
            Line::fails(ErrorClass::Stalled, "空闲超时"),
            ErrorClass::Stalled,
        ),
        (Line::says("   "), ErrorClass::EmptyReply),
        // 要了工具、网关把它换成一句占位的字一起交回来的：那句字不是标题。
        (
            Line::calls(PLACEHOLDER, &[("read", "{}")]),
            ErrorClass::EmptyReply,
        ),
    ] {
        let mut stage = titled_stage();
        stage.set_meta(Some("人起的"), None);
        one_turn(&mut stage, "hi");
        stage.title_model([line]);
        stage.retitle();
        let failures = retitle_failures(&stage);
        let [(by, failed)] = &failures[..] else {
            panic!("推一条：{failures:?}");
        };
        assert!(matches!(by, By::Person(_)), "{by:?}");
        assert_eq!(*failed, class);
        assert_eq!(titles(&stage), [(false, "人起的".to_string())], "标题不动");
    }
}

/// 起成了的、要了以后人又改了名的，都不推（施工 TT-3）。
#[test]
fn only_a_failed_retitle_is_told() {
    let mut stage = titled_stage();
    one_turn(&mut stage, "hi");
    stage.title_model([Line::says("重起的")]);
    stage.retitle();
    assert_eq!(titles(&stage), [(false, "重起的".to_string())]);
    stage.title_model([Line::fails(ErrorClass::Retryable, "503").held()]);
    stage.retitle();
    stage.set_meta(Some("又改了"), None);
    stage.release_title();
    assert!(
        retitle_failures(&stage).is_empty(),
        "{:?}",
        stage.retitle_failures()
    );
}

/// 请求里没有工具、模型要了一件时，有的网关交回来的那句占位的字（施工 TT-3，主人本机 2026-11-13 起标题时见到的；原来定义在
/// 回顾的测试里，回顾 LM-8 删了，挪过来）。
const PLACEHOLDER: &str = "(The model asked for a tool this session doesn't have)";
