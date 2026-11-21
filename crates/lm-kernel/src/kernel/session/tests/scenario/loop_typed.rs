//! 人亲手敲 `/loop` 交进来的第一句（LM-34，2026-11-22 项目主人：「我都 /loop 了，我还要撤回干嘛？这消息是用户发的！」）：
//! `by` 是定时任务、带着人敲的那一行（`Command::Typed`），算人的一次操作。还能恢复撤销时照样开一轮，开了以后那次撤销恢复
//! 不了；同样的话照普通的定时任务交（不带那一行）的照旧只记下。没人看着的一次性会话、要重启了的，带着那一行也不开。载入时
//! 照日志算的一样：开没开看 `turn.started`。

use super::reports::event;
use super::*;
use crate::kernel::event::MessageUser;

/// 说两句、撤掉第二轮：还能恢复。
fn undone() -> Stage {
    let mut s = stage();
    s.model([Line::says("好。"), Line::says("嗯。")]);
    s.say("hi");
    s.say("再来");
    s.revert(TurnId::new(seq(10)));
    assert!(s.undo_pending(), "撤了还没恢复");
    s
}

/// 命令 `id` 记下的那一条 `message.user`。
fn message_of<'s>(s: &'s Stage, id: &CommandId) -> &'s MessageUser {
    s.log()
        .iter()
        .find_map(|event| match &event.body {
            Body::MessageUser(message) if event.cause.as_ref() == Some(id) => Some(message),
            _ => None,
        })
        .unwrap_or_else(|| panic!("{id} 没记下：{:#?}", story(s)))
}

#[test]
fn the_first_words_of_a_loop_the_person_starts_open_a_turn_even_while_an_undo_waits() {
    let mut s = undone();
    s.model([Line::says("CI 还在跑。")]);
    let id = s.loop_typed("t1", "/loop 5m 看看 CI", "看看 CI");
    assert_eq!(s.turns().len(), 3, "人亲手开的：开了一轮：{:#?}", story(&s));
    let opened = s.turns()[2];
    assert_eq!(event(&s, opened.started().get()).cause, Some(id.clone()));
    let message = message_of(&s, &id);
    assert_eq!(
        message.typed(),
        Some("/loop 5m 看看 CI"),
        "带着人敲的那一行"
    );
    assert_eq!(message.text(), "看看 CI", "内容块照旧是那句话");
    let by = &s
        .log()
        .iter()
        .find(|event| event.cause.as_ref() == Some(&id))
        .map(|event| event.by.clone());
    assert!(
        matches!(by, Some(By::Schedule(schedule)) if schedule.schedule == "t1"),
        "照旧记成定时任务交的：{by:?}"
    );
    assert!(!s.undo_pending(), "开了一轮：那次撤销恢复不了");
    let unrevert = s.unrevert();
    assert_eq!(
        s.outcome(&unrevert),
        Some(&Outcome::Rejected {
            reason: Reason::NothingToUnrevert,
        }),
        "/restore 被拒"
    );
    // 重放这段日志得到一样的状态。
    let before = story(&s);
    s.crash();
    assert_eq!(story(&s), before, "载入不多记、不开轮");
    assert!(!s.undo_pending(), "载入以后照样恢复不了");
    s.model([Line::says("好的。")]);
    s.say("接着");
    assert_eq!(s.turns().len(), 4, "人的下一句照常开");
}

#[test]
fn the_same_words_sent_as_a_plain_schedule_only_wait() {
    let mut s = undone();
    s.schedule_says("t1", "看看 CI");
    assert_eq!(s.turns().len(), 2, "照普通的定时任务交：只记下");
    assert!(s.undo_pending(), "撤销照旧能恢复");
    s.crash();
    assert_eq!(s.turns().len(), 2, "载入不因为它开轮");
    s.model([Line::says("看了。")]);
    s.unrevert();
    assert_eq!(s.turns().len(), 3, "恢复了由它接着开");
}

#[test]
fn an_unwatched_oneshot_or_a_restarting_session_still_only_records_it() {
    let mut s = Stage::oneshot(policy, environment("~/src/miyu"), at(0));
    s.loop_typed("t1", "/loop 5m 看看 CI", "看看 CI");
    assert_eq!(s.turns().len(), 0, "没人看着的一次性会话：只记下");
    let mut s = stage();
    s.model([Line::says("好。")]);
    s.say("hi");
    s.restarting();
    s.loop_typed("t1", "/loop 5m 看看 CI", "看看 CI");
    assert_eq!(s.turns().len(), 1, "要重启了：只记下");
}
