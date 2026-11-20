//! L5 二「做完的样子」：主会话经真的会话表派一个子代理，子会话在同一个进程里另有日志、回合，调一件真的
//! 工具（`read`）、答完；回报向上交给主会话，闲着的主会话被叫醒开一轮，那一轮的请求里看得到回报。父子之间 `send_message`
//! 留言：主会话给子代理留言，子会话收到开一轮、再回报。照剧本回话的端口照请求现回（`Script::answers`）：两个会话的请求谁先
//! 到不一定，照 system 里有没有子代理的那一段、最后一条是什么认是谁、该说什么。

use lm_engine::session::testkit::{Play, Script};
use lm_kernel::kernel::block::Block;
use lm_kernel::kernel::event::{Body, ChildReason, Effect, Event, JobKind};
use lm_kernel::kernel::id::{JobId, SessionId};
use lm_kernel::kernel::request::{Message, Request};

use crate::support::jobs::*;
use crate::support::*;

/// 子会话的 system 末尾接的那一段（`core/jobs/subagent-venue.txt`）的开头。
const CHILD: &str = "You are a subagent, started by another session";

/// 一条消息里的字，连成一段。
fn text(message: &Message) -> String {
    let blocks = match message {
        Message::User { blocks } | Message::Assistant { blocks } | Message::Tool { blocks, .. } => {
            blocks
        }
    };
    blocks
        .iter()
        .filter_map(|block| match block {
            Block::Text(text) => Some(text.text.as_str()),
            _ => None,
        })
        .collect()
}

/// 照请求现回：子会话先用 `shell` 读 `note.txt` 再答；主会话先派子代理，派出去说一句，看到回报说一句；人让她留言的，给 `j1` 留言。
fn answer(request: &Request) -> Play {
    let last = request.messages.last();
    let child = request.system.contains(CHILD);
    match (child, last) {
        (true, Some(Message::Tool { .. })) => Play::Says("child: the note says hello"),
        (true, Some(message)) if text(message).contains("Look again") => {
            Play::Says("child: still hello")
        }
        // 先停一秒再读：父会话那一轮先说完、闲下来，回报才是叫醒它的。
        (true, _) => Play::calls(&[(
            "shell",
            r#"{"command":"sleep 1; cat note.txt","description":"Read the note"}"#,
        )]),
        (false, Some(Message::Tool { .. })) => Play::Says("派出去了。"),
        (false, Some(message)) if text(message).contains("<subagent-report") => {
            Play::Says("看到回报了。")
        }
        (false, Some(message)) if text(message).contains("留个言") => Play::calls(&[(
            "send_message",
            r#"{"to":"j1","message":"Look again, please."}"#,
        )]),
        (false, _) => Play::calls(&[(
            "subagent",
            r#"{"description":"Read the note","prompt":"Read note.txt and tell me what it says."}"#,
        )]),
    }
}

/// 同 [`answer`]，只是主会话派子代理时还写了池 `pool`：LM-8 以前造的会话，快照里 `subagent` 带着这一格，她照样会写。
fn answer_with_a_pool(request: &Request) -> Play {
    let child = request.system.contains(CHILD);
    match (child, request.messages.last()) {
        (false, Some(Message::User { .. })) => Play::calls(&[(
            "subagent",
            r#"{"description":"Read the note","prompt":"Read note.txt and tell me what it says.","pool":"daily"}"#,
        )]),
        _ => answer(request),
    }
}

/// 派出去的子代理：编号、子会话。
fn spawned(log: &[Event]) -> (JobId, SessionId) {
    results(log)
        .iter()
        .flat_map(|result| &result.effects)
        .find_map(|effect| match effect {
            Effect::JobStarted(started) if started.what == JobKind::Agent => started
                .session
                .clone()
                .map(|session| (started.job.clone(), session)),
            _ => None,
        })
        .expect("派了一个子代理")
}

/// 日志里的 `child.reported`。
fn child_reports(log: &[Event]) -> Vec<&Event> {
    log.iter()
        .filter(|event| matches!(event.body, Body::ChildReported(_)))
        .collect()
}

#[tokio::test]
async fn a_subagent_reads_answers_and_wakes_its_parent() {
    let home = Home::outside_temp();
    std::fs::write(home.scratch.0.join("work/note.txt"), "hello\n").expect("写得进");
    let script = Script::new([]).answers(answer);
    let handle = home.create_as(&script, &base_system(), full(&home)).await;
    talk(&handle, "cmd-1", "派个子代理读一下").await;
    let log = until_logged(&home, handle.id(), |log| turns_ended(log) == 2).await;
    let (job, child) = spawned(&log);
    assert_eq!(job, JobId::new(1).expect("合写法"));

    // 子会话另有日志、回合：父会话、第几层记在它的 `session.created` 里，它调了一件真的 `shell`。
    let child_log = home.log(&child);
    let Some(Body::SessionCreated(created)) = child_log.first().map(|event| &event.body) else {
        panic!("子会话的第一条是 session.created：{child_log:#?}")
    };
    assert_eq!(created.parent.as_ref(), Some(handle.id()));
    assert_eq!(created.depth, Some(1));
    let read = results(&child_log);
    assert_eq!(read.len(), 1, "子会话调了一件工具");
    assert!(
        format!("{:?}", read[0].blocks).contains("hello"),
        "{:?}",
        read[0].blocks
    );
    assert!(turns_ended(&child_log) >= 1);

    // 回报交给了父会话：记一条 `child.reported`，由它开了一轮，那一轮的回复照剧本说看到了。
    let reports = child_reports(&log);
    let [report] = reports[..] else {
        panic!("记了一条回报：{:?}", kinds(&log))
    };
    let Body::ChildReported(reported) = &report.body else {
        unreachable!()
    };
    assert_eq!(reported.job, job);
    assert_eq!(reported.session, child);
    assert_eq!(reported.reason, ChildReason::Done);
    assert_eq!(reported.text, "child: the note says hello");
    let woke = log.iter().find_map(|later| match &later.body {
        Body::TurnStarted(started) if later.seq > report.seq => Some(started.trigger),
        _ => None,
    });
    assert_eq!(woke, Some(Some(report.seq)), "闲着的父会话由回报叫醒");
    let last = log
        .iter()
        .rev()
        .find_map(|event| match &event.body {
            Body::MessageAssistant(reply) => Some(format!("{:?}", reply.blocks)),
            _ => None,
        })
        .expect("说过话");
    assert!(last.contains("看到回报了。"), "{last}");
    stop(&handle).await;
}

#[tokio::test]
async fn a_parent_leaves_its_subagent_a_message_and_hears_back() {
    let home = Home::outside_temp();
    std::fs::write(home.scratch.0.join("work/note.txt"), "hello\n").expect("写得进");
    let script = Script::new([]).answers(answer);
    let handle = home.create_as(&script, &base_system(), full(&home)).await;
    talk(&handle, "cmd-1", "派个子代理读一下").await;
    until_logged(&home, handle.id(), |log| turns_ended(log) == 2).await;
    talk(&handle, "cmd-2", "给它留个言").await;
    let log = until_logged(&home, handle.id(), |log| child_reports(log).len() == 2).await;
    let sent = results(&log)
        .into_iter()
        .rev()
        .find(|result| format!("{:?}", result.blocks).contains("j1"))
        .expect("留言的结果");
    assert!(
        sent.effects
            .iter()
            .any(|effect| matches!(effect, Effect::JobMessaged(messaged) if messaged.job == JobId::new(1).expect("合写法"))),
        "给子代理的留言记一样效果：{sent:?}"
    );
    let (_, child) = spawned(&log);
    let child_log = home.log(&child);
    assert!(
        child_log.iter().any(|event| matches!(&event.body, Body::MessageUser(said) if format!("{:?}", said.blocks).contains("Look again"))),
        "子会话收到了留言：{:?}",
        kinds(&child_log)
    );
    let Body::ChildReported(second) = &child_reports(&log)[1].body else {
        unreachable!()
    };
    assert_eq!(second.text, "child: still hello");
    stop(&handle).await;
}

/// 照请求现回：子会话一直停在第一次请求上；主会话派出去以后用 `jobs` 停掉它，再说一句。
fn stopping(request: &Request) -> Play {
    if request.system.contains(CHILD) {
        return Play::Holds;
    }
    match request.messages.last() {
        Some(message @ Message::Tool { .. }) if text(message).contains("Stopped j1") => {
            Play::Says("停掉了。")
        }
        Some(Message::Tool { .. }) => Play::calls(&[("jobs", r#"{"action":"stop","id":"j1"}"#)]),
        _ => Play::calls(&[(
            "subagent",
            r#"{"description":"Wait forever","prompt":"Take your time."}"#,
        )]),
    }
}

#[tokio::test]
async fn she_stops_her_subagent_through_the_table() {
    let home = Home::outside_temp();
    let script = Script::new([]).answers(stopping);
    let handle = home.create_as(&script, &base_system(), full(&home)).await;
    talk(&handle, "cmd-1", "派一个再停掉").await;
    let log = until_logged(&home, handle.id(), |log| child_reports(log).len() == 1).await;
    let Body::ChildReported(reported) = &child_reports(&log)[0].body else {
        unreachable!()
    };
    assert_eq!(reported.reason, ChildReason::Stopped);
    assert!(reported.by_model, "她自己停的");
    let (_, child) = spawned(&log);
    let child_log = until_logged(&home, &child, |log| turns_ended(log) == 1).await;
    assert!(
        child_log
            .iter()
            .any(|event| matches!(event.body, Body::TurnEnded(_))),
        "子会话那一轮被打断收尾了：{:?}",
        kinds(&child_log)
    );
    stop(&handle).await;
}

/// 照请求现回：子会话要强推（高危，问人），推完说一句；主会话派出去说一句，看到回报说一句。
fn risky(request: &Request) -> Play {
    let last = request.messages.last();
    match (request.system.contains(CHILD), last) {
        (true, Some(Message::Tool { .. })) => Play::Says("child: pushed"),
        (true, _) => Play::calls(&[(
            "shell",
            r#"{"command":"git push --force","description":"Push"}"#,
        )]),
        (false, Some(Message::Tool { .. })) => Play::Says("派出去了。"),
        (false, Some(message)) if text(message).contains("<subagent-report") => {
            Play::Says("看到回报了。")
        }
        (false, _) => Play::calls(&[(
            "subagent",
            r#"{"description":"Push it","prompt":"Force-push the branch."}"#,
        )]),
    }
}

#[tokio::test]
async fn the_head_answers_a_subagents_confirmation_through_the_main_handle() {
    // 界面线第二趟：子代理的高危命令要人确认，界面经主会话的把手按编号拿到子会话、答它；不答的话子代理一直卡着。
    let home = Home::outside_temp();
    let script = Script::new([]).answers(risky);
    let opening = Opening {
        attended: true,
        ..full(&home)
    };
    let handle = home.create_as(&script, &base_system(), opening).await;
    talk(&handle, "cmd-1", "派个子代理推一下").await;
    let log = until_logged(&home, handle.id(), |log| turns_ended(log) == 1).await;
    let (_, child) = spawned(&log);
    let child_log = until_logged(&home, &child, |log| {
        kinds(log).contains(&"tool.approval_requested")
    })
    .await;
    let call_id = child_log
        .iter()
        .find_map(|event| match &event.body {
            Body::ApprovalRequested(asked) => Some(asked.call_id),
            _ => None,
        })
        .expect("子会话在问人");
    assert!(handle.child(handle.id()).await.is_none(), "主会话自己不算");
    let stranger = lm_engine::session::new_id(now());
    assert!(handle.child(&stranger).await.is_none(), "表里没有的不载入");
    let held = handle.child(&child).await.expect("拿得到子会话");
    assert!(held.child(&child).await.is_none(), "子会话的把手没有会话表");
    let answer = lm_kernel::kernel::session::Answer::Approval {
        decision: lm_kernel::kernel::event::Decision::Deny,
        reason: Some("不许强推".into()),
        attachments: Vec::new(),
    };
    let outcome = ask(
        &held,
        "cmd-2",
        lm_kernel::kernel::session::Command::Answer { call_id, answer },
    )
    .await
    .expect("子会话在跑");
    assert!(
        !matches!(
            outcome,
            lm_kernel::kernel::session::Outcome::Rejected { .. }
        ),
        "{outcome:?}"
    );
    let log = until_logged(&home, handle.id(), |log| child_reports(log).len() == 1).await;
    let Body::ChildReported(reported) = &child_reports(&log)[0].body else {
        unreachable!()
    };
    assert_eq!(reported.text, "child: pushed", "答了以后子代理接着走完");
    stop(&handle).await;
}

/// 第二十六趟（验收时报的：重开会话以后累计用量变少）：子代理用掉的记进主会话的概要，重开主会话不读子会话的日志也
/// 知道它用了多少（`Handle::earlier_agents`），和子会话日志里 `model.called` 加起来的一样。没派过子代理的会话没有。
#[tokio::test]
async fn a_reopened_session_still_knows_what_its_subagents_spent() {
    let home = Home::outside_temp();
    std::fs::write(home.scratch.0.join("work/note.txt"), "hello\n").expect("写得进");
    let script = Script::new([]).answers(answer);
    let handle = home.create_as(&script, &base_system(), full(&home)).await;
    assert!(handle.earlier_agents().is_none(), "新开的会话没有以前的");
    talk(&handle, "cmd-1", "派个子代理读一下").await;
    let log = until_logged(&home, handle.id(), |log| turns_ended(log) == 2).await;
    let (_, child) = spawned(&log);
    let called: Vec<_> = home
        .log(&child)
        .into_iter()
        .filter_map(|event| match event.body {
            Body::ModelCalled(called) => called.usage,
            _ => None,
        })
        .collect();
    assert_eq!(called.len(), 2, "子会话请求了两次：调工具、答");
    let main = handle.id().clone();
    stop(&handle).await;
    drop(handle);

    let reopened = home.load_with(&main, &script, &base_system()).await;
    let earlier = reopened.earlier_agents().expect("概要里记着子代理用掉的");
    let output: u64 = called.iter().map(|usage| usage.output).sum();
    let input: u64 = called.iter().map(|usage| usage.input()).sum();
    assert_eq!(earlier.total.output, output);
    assert_eq!(earlier.total.input(), input);
    assert_eq!(earlier.main.output, output, "子会话的都是主请求");
    stop(&reopened).await;
}

/// LM-8 砍了模型池：她写了池 `pool`（旧会话的快照里那一格还在）也照不写办，子代理照派，用父会话的模型；不报参数不对。
#[tokio::test]
async fn an_old_pool_argument_is_ignored_and_the_subagent_still_starts() {
    let home = Home::outside_temp();
    std::fs::write(home.scratch.0.join("work/note.txt"), "hello\n").expect("写得进");
    let script = Script::new([]).answers(answer_with_a_pool);
    let handle = home.create_as(&script, &base_system(), full(&home)).await;
    talk(&handle, "cmd-1", "派个子代理读一下").await;
    let log = until_logged(&home, handle.id(), |log| turns_ended(log) == 2).await;
    let first = results(&log)
        .first()
        .map(|result| format!("{:?}", result.blocks))
        .expect("调了一次 subagent");
    assert!(!first.contains("unknown variant"), "不报池写错：{first}");
    let (_, child) = spawned(&log);
    let child_log = home.log(&child);
    let Some(Body::SessionCreated(created)) = child_log.first().map(|event| &event.body) else {
        panic!("子会话的第一条是 session.created：{child_log:#?}")
    };
    assert_eq!(
        created.model, None,
        "不记 @daily：用父会话的（照剧本的端口没有引用）"
    );
    stop(&handle).await;
}
