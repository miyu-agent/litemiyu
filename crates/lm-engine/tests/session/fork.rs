//! fork 子代理（LM-18，照 Miyu `miyu-endpoint/tests/fork_subagent.rs`、`agents.md`「fork 子代理」）：主会话聊过一句以后在同一步
//! 里派一个 `fork: true` 的和一个不带 fork 的。fork 的第一轮请求里带着主会话到派它那一步为止的对话（人那一句、她的回答、派它的
//! 那条回复、那一步每个调用的占位结果），system、工具面和主会话的一样；不带的只有交代。分叉以后两边各写各的：主会话接着聊
//! 的子会话看不到，子会话中间做的主会话看不到（只看到回报）。重开以后分叉的子会话照样打得开，接着聊请求还是对的；主会话
//! 后来压缩、撤销到分叉点以前、删掉，子会话怎么样照 Miyu。
//!
//! 照剧本回话的端口照请求现回（`Script::answers`）：谁的请求照 system 里有没有子代理那一段、对话里有没有 fork 的外壳认。

use lm_engine::session::testkit::{Play, Script};
use lm_kernel::kernel::event::{Body, Effect, Event, JobKind};
use lm_kernel::kernel::id::{JobId, SessionId, TurnId};
use lm_kernel::kernel::request::{Message, Request};
use lm_kernel::kernel::session::Command;

use crate::support::jobs::*;
use crate::support::*;

/// 普通子代理的 system 末尾接的那一段（`core/jobs/subagent-venue.txt`）的开头。
const CHILD: &str = "You are a subagent, started by another session";

/// fork 的外壳（`core/jobs/fork-directive.txt`）里的一句。
const FORK: &str = "You are a fork of the agent above.";

/// 占位结果（`core/tool-results/forked.txt`）。
const PLACEHOLDER: &str = "Forked. The parent handles this call.";

/// 整份请求写成一段字，查里面有没有某一句。
fn all(request: &Request) -> String {
    format!("{:?}", request.messages)
}

/// 最后一条人这边的消息写成一段字。
fn last_user(request: &Request) -> String {
    request
        .messages
        .iter()
        .rev()
        .find(|message| matches!(message, Message::User { .. }))
        .map(|message| format!("{message:?}"))
        .unwrap_or_default()
}

/// 是压缩的摘要请求：最后一条人话的最后一块以压缩的那一句开头（`core/compaction/summarize-task.txt`）。
fn summarizing(request: &Request) -> bool {
    matches!(
        request.messages.last(),
        Some(Message::User { blocks })
            if matches!(blocks.last(), Some(lm_kernel::kernel::block::Block::Text(text)) if text.text.starts_with("Respond with text only."))
    )
}

/// 照请求现回。fork 的子会话：先跑一个命令（中间做的，主会话不该看到），再答 `FORK-REPORT`；接着聊的答 `FORK-AGAIN`；
/// 它要再 fork 的照说再 fork。普通子代理答 `PLAIN-REPORT`。主会话：第一句答 `ALPHA-REPLY`；「派两个」同一步派一个 fork 的、
/// 一个不带的；工具结果以后说派出去了；看到回报说一句；「第三句」答 `AFTER-FORK`；压缩照剧本写摘要。
fn answer(request: &Request) -> Play {
    if summarizing(request) {
        return Play::Says("<summary>PARENT-SUMMARY</summary>");
    }
    let everything = all(request);
    let last = request.messages.last();
    if everything.contains(FORK) {
        return match last {
            Some(Message::Tool { .. }) if everything.contains("FORK-INNER") => {
                Play::Says("FORK-REPORT")
            }
            Some(Message::Tool { .. }) => Play::Says("FORK-NESTED-REFUSED"),
            _ if last_user(request).contains("再 fork") => Play::calls(&[(
                "subagent",
                r#"{"description":"Nested","prompt":"TASK-N","fork":true}"#,
            )]),
            _ if last_user(request).contains("接着") => Play::Says("FORK-AGAIN"),
            _ => Play::calls(&[(
                "shell",
                r#"{"command":"echo FORK-INNER","description":"Echo"}"#,
            )]),
        };
    }
    if request.system.contains(CHILD) {
        return Play::Says("PLAIN-REPORT");
    }
    match last {
        Some(Message::Tool { .. }) => Play::Says("派出去了。"),
        _ => {
            let said = last_user(request);
            if said.contains("<subagent-report") {
                Play::Says("看到回报了。")
            } else if said.contains("派两个") {
                Play::calls(&[
                    (
                        "subagent",
                        r#"{"description":"Fork one","prompt":"TASK-F","fork":true}"#,
                    ),
                    (
                        "subagent",
                        r#"{"description":"Plain one","prompt":"TASK-P"}"#,
                    ),
                ])
            } else if said.contains("第三句") {
                Play::Says("AFTER-FORK")
            } else {
                Play::Says("ALPHA-REPLY")
            }
        }
    }
}

/// 派出去的子代理：标题、任务编号、子会话、`fork`，照先后。
fn started(log: &[Event]) -> Vec<(String, JobId, SessionId, bool)> {
    results(log)
        .iter()
        .flat_map(|result| &result.effects)
        .filter_map(|effect| match effect {
            Effect::JobStarted(started) if started.what == JobKind::Agent => {
                started.session.clone().map(|session| {
                    (
                        started.title.clone(),
                        started.job.clone(),
                        session,
                        started.fork,
                    )
                })
            }
            _ => None,
        })
        .collect()
}

/// 派出去的那个 fork 的和那个不带的：子会话。
fn children(log: &[Event]) -> (SessionId, SessionId) {
    let all = started(log);
    let fork = all
        .iter()
        .find(|row| row.0 == "Fork one")
        .expect("派了 fork 的");
    let plain = all
        .iter()
        .find(|row| row.0 == "Plain one")
        .expect("派了不带的");
    assert!(fork.3, "job.started 写 fork");
    assert!(!plain.3, "不带的不写 fork");
    (fork.2.clone(), plain.2.clone())
}

/// 两份回报都到了，最后一份以后主会话又说完了一轮。
fn settled(log: &[Event]) -> bool {
    let last_report = log
        .iter()
        .filter(|event| matches!(event.body, Body::ChildReported(_)))
        .map(|event| event.seq)
        .max();
    let last_end = log
        .iter()
        .filter(|event| matches!(event.body, Body::TurnEnded(_)))
        .map(|event| event.seq)
        .max();
    reported(log) == 2 && last_end > last_report
}

/// 日志里的 `child.reported` 有几条。
fn reported(log: &[Event]) -> usize {
    log.iter()
        .filter(|event| matches!(event.body, Body::ChildReported(_)))
        .count()
}

/// 发给某个会话的请求：照对话里认（fork 的有外壳，普通子代理的 system 里有子代理那一段，别的是主会话的）。
fn of_fork(script: &Script) -> Vec<Request> {
    script
        .requests()
        .into_iter()
        .map(|(_, request)| request)
        .filter(|request| all(request).contains(FORK))
        .collect()
}

fn of_plain(script: &Script) -> Vec<Request> {
    script
        .requests()
        .into_iter()
        .map(|(_, request)| request)
        .filter(|request| request.system.contains(CHILD))
        .collect()
}

fn of_main(script: &Script) -> Vec<Request> {
    script
        .requests()
        .into_iter()
        .map(|(_, request)| request)
        .filter(|request| {
            !request.system.contains(CHILD) && !all(request).contains(FORK) && !summarizing(request)
        })
        .collect()
}

/// 主会话说第一句、再让她派两个，等两份回报都到、主会话那一轮也说完。交回主会话的把手和日志。
async fn forked_once(home: &Home, script: &Script) -> (lm_engine::session::Handle, Vec<Event>) {
    forked_after(home, script, "第一句：记住 ALPHA").await
}

/// 同 [`forked_once`]，第一句是 `first`。
async fn forked_after(
    home: &Home,
    script: &Script,
    first: &str,
) -> (lm_engine::session::Handle, Vec<Event>) {
    std::fs::create_dir_all(home.scratch.0.join("work")).expect("建得了");
    let handle = home.create_as(script, &base_system(), full(home)).await;
    talk(&handle, "cmd-1", first).await;
    talk(&handle, "cmd-2", "派两个子代理").await;
    let log = until_logged(home, handle.id(), settled).await;
    (handle, log)
}

/// 改前红第一条、第二条：fork 的第一轮请求里带着主会话到派它那一步为止的对话，system、工具面和主会话的一样，交代前面接着外壳；
/// 不带 fork 的只有交代。子会话的头几条照 Miyu：带 `forked` 的 `session.created`（从派它的那条回复分叉、`within` 写着那一轮、
/// 快照就是主会话那一份、第 1 层），序号接着分叉点；回合开始不换快照。
#[tokio::test]
async fn a_fork_starts_from_the_conversation_and_a_plain_one_from_the_prompt_only() {
    let home = Home::outside_temp();
    let script = Script::new([]).answers(answer).recording();
    let (handle, log) = forked_once(&home, &script).await;
    let (fork, plain) = children(&log);

    let forks = of_fork(&script);
    let first = forks.first().expect("fork 的发过请求");
    let text = all(first);
    assert!(text.contains("第一句：记住 ALPHA"), "人那一句：{text}");
    assert!(text.contains("ALPHA-REPLY"), "她的回答：{text}");
    assert!(
        text.contains("TASK-F") && text.contains("TASK-P"),
        "派它的那条回复：{text}"
    );
    assert_eq!(
        text.matches(PLACEHOLDER).count(),
        2,
        "那一步两个调用各一条占位结果：{text}"
    );
    let directive = last_user(first);
    assert!(
        directive.contains(FORK) && directive.contains("TASK-F"),
        "{directive}"
    );
    let main = of_main(&script);
    let parent_next = main.get(2).expect("主会话派完以后的那一次");
    assert_eq!(first.system, parent_next.system, "system 和主会话的一样");
    assert_eq!(first.tools, parent_next.tools, "工具面和主会话的一样");
    assert_eq!(
        first.messages[..4],
        parent_next.messages[..4],
        "人那一句、她的回答、人第二句、派它的那条回复和主会话的一样"
    );

    let plains = of_plain(&script);
    let plain_first = plains.first().expect("不带的发过请求");
    let plain_text = all(plain_first);
    assert!(plain_text.contains("TASK-P"), "{plain_text}");
    assert!(
        !plain_text.contains("ALPHA"),
        "不带 fork 的只有交代：{plain_text}"
    );
    assert!(!plain_text.contains(FORK), "{plain_text}");

    let parent_log = home.log(handle.id());
    let step = parent_log
        .iter()
        .filter(|event| matches!(event.body, Body::MessageAssistant(_)))
        .nth(1)
        .map(|event| event.seq)
        .expect("派它们的那条回复");
    let snapshot = match &parent_log[0].body {
        Body::SessionCreated(created) => created.policy.clone(),
        other => panic!("第一条是造会话：{other:?}"),
    };
    let fork_log = home.log(&fork);
    let Body::SessionCreated(made) = &fork_log[0].body else {
        panic!("第一条是造会话：{fork_log:#?}")
    };
    let forked = made.forked.clone().expect("分叉出来的");
    assert_eq!(forked.from, *handle.id());
    assert_eq!(forked.upto, step, "从派它的那一步分叉");
    assert!(forked.within.is_some(), "分叉点在那一轮当中");
    assert_eq!(fork_log[0].seq, step.next(), "序号接着分叉点");
    assert_eq!(made.policy, snapshot, "快照就是主会话那一份");
    assert_eq!(made.depth, Some(1));
    assert_eq!(made.parent.as_ref(), Some(handle.id()));
    assert!(
        !fork_log
            .iter()
            .any(|event| matches!(event.body, Body::PolicyChanged(_))),
        "回合开始不换快照"
    );
    let plain_log = home.log(&plain);
    let Body::SessionCreated(plain_made) = &plain_log[0].body else {
        panic!("第一条是造会话")
    };
    assert!(plain_made.forked.is_none());
    stop(&handle).await;
}

/// 改前红第三条、第四条：分叉以后两边各写各的。主会话接着聊的子会话看不到；子会话中间做的（跑的命令）主会话看不到，
/// 只看到回报。
#[tokio::test]
async fn after_the_fork_each_side_writes_its_own() {
    let home = Home::outside_temp();
    let script = Script::new([]).answers(answer).recording();
    let (handle, log) = forked_once(&home, &script).await;
    let (fork, _) = children(&log);
    talk(&handle, "cmd-3", "第三句").await;

    for request in of_main(&script) {
        let text = all(&request);
        assert!(
            !text.contains("FORK-INNER"),
            "子会话中间做的主会话看不到：{text}"
        );
        assert!(!text.contains(FORK), "{text}");
    }
    assert!(
        of_main(&script)
            .iter()
            .any(|request| all(request).contains("FORK-REPORT")),
        "回报回来了"
    );

    // 给 fork 的子会话再说一句：请求里没有主会话分叉以后说的。
    let child = handle.child(&fork).await.expect("表里有它");
    talk(&child, "cmd-c1", "接着做").await;
    let forks = of_fork(&script);
    let last = forks.last().expect("又请求了一次");
    let text = all(last);
    assert!(
        text.contains("FORK-INNER") && text.contains("FORK-REPORT"),
        "{text}"
    );
    assert!(
        !text.contains("第三句") && !text.contains("AFTER-FORK"),
        "{text}"
    );
    assert!(!text.contains("看到回报了"), "{text}");
    stop(&handle).await;
}

/// 改前红第五条：重开以后分叉的子会话打得开（直接载入它），接着聊请求还是对的：底子、它自己的、新说的都在，主会话分叉以后
/// 的不在。
#[tokio::test]
async fn a_reopened_fork_still_sees_the_base() {
    let home = Home::outside_temp();
    let script = Script::new([]).answers(answer).recording();
    let (handle, log) = forked_once(&home, &script).await;
    let (fork, _) = children(&log);
    talk(&handle, "cmd-3", "第三句").await;
    let before = of_fork(&script).last().cloned().expect("请求过");
    stop(&handle).await;
    drop(handle);

    let reopened = home.load_with(&fork, &script, &base_system()).await;
    talk(&reopened, "cmd-c1", "接着做").await;
    let after = of_fork(&script).last().cloned().expect("又请求了一次");
    let text = all(&after);
    assert!(text.contains("第一句：记住 ALPHA"), "{text}");
    assert!(
        text.contains("FORK-INNER") && text.contains("FORK-REPORT"),
        "{text}"
    );
    assert!(!text.contains("AFTER-FORK"), "{text}");
    assert_eq!(after.system, before.system);
    assert_eq!(
        after.messages[..before.messages.len()],
        before.messages[..],
        "重开以后前面的一条不差"
    );
    stop(&reopened).await;
}

/// 主会话后来压缩、撤销到分叉点以前，子会话一点不受影响（照 Miyu：子会话只读主会话日志到分叉点为止的那一截，主会话后来记的
/// 都在分叉点以后）：重开子会话，请求里照样是分叉时的那一截。
#[tokio::test]
async fn compacting_or_undoing_the_parent_later_leaves_the_fork_alone() {
    let home = Home::outside_temp();
    let script = Script::new([]).answers(answer).recording();
    // 第一句说七万个字：超了出厂 16000 token 的尾巴，手动压得掉它（同 `rebuild.rs`）。
    let first = format!("第一句：记住 ALPHA {}", "x".repeat(70_000));
    let (handle, log) = forked_after(&home, &script, &first).await;
    let (fork, _) = children(&log);
    let mut pushes = watch(&handle).await;
    let compact = Command::Compact { instructions: None };
    ask(&handle, "cmd-3", compact).await.expect("会话在跑");
    until_turn_ends(&mut pushes).await;
    let parent_log = home.log(handle.id());
    assert!(
        parent_log
            .iter()
            .any(|event| matches!(event.body, Body::ContextCompacted(_))),
        "主会话压了"
    );
    // 撤到第一轮（分叉点在第二轮里）。
    let first_turn = parent_log
        .iter()
        .find(|event| matches!(event.body, Body::TurnStarted(_)))
        .map(|event| TurnId::new(event.seq))
        .expect("开过一轮");
    let undo = Command::Revert {
        turn: Some(first_turn),
    };
    ask(&handle, "cmd-4", undo).await.expect("会话在跑");
    let main = handle.id().clone();
    stop(&handle).await;
    drop(handle);

    let reopened = home.load_with(&fork, &script, &base_system()).await;
    talk(&reopened, "cmd-c1", "接着做").await;
    let text = all(of_fork(&script).last().expect("请求了"));
    assert!(text.contains("第一句：记住 ALPHA"), "{text}");
    assert!(text.contains("ALPHA-REPLY"), "{text}");
    assert!(
        !text.contains("PARENT-SUMMARY"),
        "主会话后来的压缩不进子会话：{text}"
    );
    stop(&reopened).await;
    // 主会话自己照它的撤销：重开主会话，请求里没有撤掉的那几轮。
    let parent = home.load_with(&main, &script, &base_system()).await;
    talk(&parent, "cmd-5", "第三句").await;
    let text = all(of_main(&script).last().expect("请求了"));
    assert!(!text.contains("ALPHA-REPLY"), "{text}");
    stop(&parent).await;
}

/// fork 出来的不能再 fork（同 Miyu `agent/no-fork.txt`）：工具结果是那一句，什么都不派。
#[tokio::test]
async fn a_fork_cannot_fork_again() {
    let home = Home::outside_temp();
    let script = Script::new([]).answers(answer).recording();
    let (handle, log) = forked_once(&home, &script).await;
    let (fork, _) = children(&log);
    let child = handle.child(&fork).await.expect("表里有它");
    talk(&child, "cmd-c1", "再 fork 一个").await;
    let fork_log = home.log(&fork);
    let last = results(&fork_log).last().copied().expect("调了 subagent");
    assert!(
        format!("{:?}", last.blocks).contains("A fork cannot fork again."),
        "{:?}",
        last.blocks
    );
    assert!(last.effects.is_empty(), "什么都没派");
    stop(&handle).await;
}

/// 主会话删掉（`/delete`），它派出去的子会话一起删（照 Miyu `session.delete`）：fork 的底子在主会话的日志里，主会话没了它读
/// 不回来；不带 fork 的也一起删。
#[tokio::test]
async fn deleting_the_parent_deletes_its_forks_too() {
    let home = Home::outside_temp();
    let script = Script::new([]).answers(answer).recording();
    let (handle, log) = forked_once(&home, &script).await;
    let (fork, plain) = children(&log);
    let main = handle.id().clone();
    stop(&handle).await;
    drop(handle);
    // 这个进程里刚放下的主会话要等它放下锁（`local.rs`）：照界面删的那样在阻塞线程里删，删不成的等一下再试。
    let root = home.root.clone();
    let id = main.to_string();
    let mut removed = Err(lm_engine::session::NotRemoved::Missing);
    for _ in 0..100 {
        let (root, id) = (root.clone(), id.clone());
        removed =
            tokio::task::spawn_blocking(move || lm_engine::session::remove_session(&root, &id))
                .await
                .expect("没 panic");
        if removed.is_ok() {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    removed.expect("删得了");
    assert!(!home.root.session(&main).exists());
    assert!(!home.root.session(&fork).exists(), "fork 的子会话一起删了");
    assert!(
        !home.root.session(&plain).exists(),
        "不带 fork 的也一起删了"
    );
}

/// 父会话的日志没了（拷走了子会话、父会话被别的办法删了）：fork 的子会话载入不了，照「读不回底子」报，不当没有底子接着跑
/// （同 Miyu：底子少一截，接上的历史就是错的）。
#[tokio::test]
async fn a_fork_without_its_parent_log_does_not_load() {
    let home = Home::outside_temp();
    let script = Script::new([]).answers(answer).recording();
    let (handle, log) = forked_once(&home, &script).await;
    let (fork, _) = children(&log);
    let main = handle.id().clone();
    stop(&handle).await;
    drop(handle);
    let parent_dir = home.root.session(&main);
    let moved = home.scratch.0.join("moved-away");
    std::fs::rename(&parent_dir, &moved).expect("挪得走");
    let setup = lm_engine::session::Load {
        root: &home.root,
        id: fork.clone(),
        place: lm_engine::session::Workplace::Given(environment()),
        models: &script,
        tools: std::sync::Arc::new(base_system()),
        home: Some(&home.home),
        configs: home.configs.clone(),
    };
    let loaded = lm_engine::session::load(setup).await;
    let Err(error) = loaded else {
        panic!("父会话的日志没了，fork 的子会话载入不了")
    };
    assert!(format!("{error}").contains("no session log"), "{error}");
    // 挪回来就又打得开。
    std::fs::rename(&moved, &parent_dir).expect("挪得回来");
    let reopened = home.load_with(&fork, &script, &base_system()).await;
    stop(&reopened).await;
}

/// 红线 5（LiteMiyu 自己的）：fork 的子会话歇着时连底子一起收成槽，不在内存里另留一份主会话的历史；要用时连底子从主会话的
/// 日志读回来。看得出收了：载入完把主会话的日志挪走，说一句就读不回、这一轮不发请求；挪回来下一句照常，请求里底子一条不少。
#[tokio::test]
async fn a_resting_fork_shelves_its_base_and_reads_it_back() {
    let home = Home::outside_temp();
    let script = Script::new([]).answers(answer).recording();
    let (handle, log) = forked_once(&home, &script).await;
    let (fork, _) = children(&log);
    let main = handle.id().clone();
    stop(&handle).await;
    drop(handle);
    let reopened = home.load_with(&fork, &script, &base_system()).await;
    let parent_dir = home.root.session(&main);
    let moved = home.scratch.0.join("moved-away");
    std::fs::rename(&parent_dir, &moved).expect("挪得走");
    let before = script.requests().len();
    talk(&reopened, "cmd-c1", "接着做").await;
    assert_eq!(
        script.requests().len(),
        before,
        "底子收着、读不回来：不发请求"
    );
    std::fs::rename(&moved, &parent_dir).expect("挪得回来");
    talk(&reopened, "cmd-c2", "接着做").await;
    let text = all(of_fork(&script).last().expect("请求了"));
    assert!(
        text.contains("第一句：记住 ALPHA") && text.contains("FORK-REPORT"),
        "{text}"
    );
    stop(&reopened).await;
}
