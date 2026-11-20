//! 撤销停掉那一轮派出去的，执行器这一头（施工 7-8，`docs/blueprint/agents.md` 第七条第 1、3 条）：已经结束了的不再杀。撤销时
//! 后台命令、子代理一起停的那一条跟子代理在 L5 二搬。

use super::*;

#[tokio::test]
async fn a_command_that_already_ended_is_not_killed_again() {
    let home = Home::new();
    let held = Held::new(&[]);
    let script = Script::new([
        Play::calls(&[("start", "{}")]),
        Play::Says("放出去了。"),
        Play::Says("跑完了。"),
    ]);
    let handle = session(&home, &script, &held).await;
    talk(&handle, "cmd-1", "后台跑").await;
    let mut pushes = watch(&handle).await;
    held.end(Exit::Code(0));
    until_turn_ends(&mut pushes).await;
    assert!(matches!(
        ask(&handle, "cmd-2", Command::Revert { turn: None }).await,
        Ok(Outcome::Accepted { .. })
    ));
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let log = home.log(handle.id());
    assert_eq!(held.killed(), 0, "结束了的不杀");
    let [(_, reported)] = job_reported(&log)[..] else {
        panic!("只有它自己退出的那一条：{:?}", kinds(&log))
    };
    assert_eq!(reported.reason, JobReason::Exited);
}

#[tokio::test]
async fn undoing_the_turn_of_a_running_command_says_one_was_stopped() {
    // A15（Miyu 核心撤销回应的 `jobs`）：撤掉的那一轮派出去、还在跑的命令跟着停，回应另带停了几个。
    let home = Home::new();
    let held = Held::new(&[]);
    let script = Script::new([Play::calls(&[("start", "{}")]), Play::Says("放出去了。")]);
    let handle = session(&home, &script, &held).await;
    talk(&handle, "cmd-1", "后台跑").await;
    until_logged(&home, handle.id(), |log| {
        log.iter()
            .any(|event| matches!(event.body, Body::TurnEnded(_)))
    })
    .await;
    let answered = ask(&handle, "cmd-2", Command::Revert { turn: None }).await;
    assert!(
        matches!(
            answered,
            Ok(Outcome::Reverted {
                undid: Undid {
                    compactions: 0,
                    clears: 0,
                    jobs: 1
                },
                ..
            })
        ),
        "{answered:?}"
    );
}
