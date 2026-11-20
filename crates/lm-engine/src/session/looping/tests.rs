//! 循环的账（L5 四）：载入时照日志认出上一次关掉时循环还开着；定时器换了代，旧一代到点不认。

use lm_kernel::kernel::event::Event;

use super::*;

/// 一条事件：序号、种类、`by`、`body`。
fn event(seq: u64, kind: &str, by: &str, body: &str) -> Event {
    serde_json::from_str(&format!(
        r#"{{"seq":{seq},"at":"2026-11-12T01:00:{seq:02}.000Z","kind":"{kind}","by":{by},"body":{body}}}"#
    ))
    .expect("合写法")
}

/// 到点交进来的一句：`looping` 是循环的种类，没有的是定时任务。
fn fired(seq: u64, looping: Option<&str>) -> Event {
    let by = match looping {
        Some(kind) => format!(r#"{{"kind":"schedule","schedule":"t1","loop":"{kind}"}}"#),
        None => r#"{"kind":"schedule","schedule":"t1"}"#.to_string(),
    };
    event(seq, "message.user", &by, r#"{"blocks":[]}"#)
}

/// 照先后记完一份日志，交回关掉时循环还开着没有。
fn left(events: &[Event]) -> bool {
    let mut trail = Trail::default();
    for event in events {
        trail.note(event);
    }
    trail.left()
}

const PERSON: &str = r#"{"kind":"person"}"#;
const KERNEL: &str = r#"{"kind":"kernel"}"#;

#[test]
fn a_loop_that_fired_last_was_left_running() {
    assert!(left(&[fired(1, Some("fixed"))]));
    assert!(left(&[fired(1, Some("dynamic"))]));
    assert!(
        left(&[
            fired(1, Some("dynamic")),
            event(2, "message.user", PERSON, r#"{"blocks":[]}"#),
        ]),
        "人后来又说过话也算：循环没停"
    );
}

#[test]
fn nothing_looped_or_a_plain_schedule_is_not_a_loop() {
    assert!(!left(&[]));
    assert!(!left(&[event(
        1,
        "message.user",
        PERSON,
        r#"{"blocks":[]}"#
    )]));
    assert!(!left(&[fired(1, None)]), "定时任务不是循环");
}

#[test]
fn a_loop_command_or_the_model_stopping_it_settles_it() {
    let ran = |seq, by, text: &str| {
        event(
            seq,
            "command.ran",
            by,
            &format!(r#"{{"text":"{text}","command":"loop"}}"#),
        )
    };
    assert!(!left(&[
        fired(1, Some("fixed")),
        ran(2, PERSON, "/loop off")
    ]));
    assert!(
        !left(&[fired(1, Some("fixed")), ran(2, KERNEL, "/loop off")]),
        "重开时写过那一行的，再重开不再写"
    );
    assert!(
        left(&[ran(1, PERSON, "/loop x"), fired(2, Some("dynamic"))]),
        "敲了 /loop 又到点交过的照开着"
    );
    let other = event(
        2,
        "command.ran",
        PERSON,
        r#"{"text":"/clear","command":"clear"}"#,
    );
    assert!(left(&[fired(1, Some("fixed")), other]), "别的命令不算");
    let stopped = event(
        2,
        "tool.result",
        r#"{"kind":"tool","call_id":"call_2_1"}"#,
        r#"{"call_id":"call_2_1","status":"ok","blocks":[],"human":{"key":"software/loop/wakeup/stopped","fields":{}}}"#,
    );
    assert!(!left(&[fired(1, Some("dynamic")), stopped]));
}

#[tokio::test]
async fn an_old_timer_is_not_due() {
    let mut run = LoopRun::default();
    let at = lm_kernel::kernel::time::Timestamp::parse("2026-11-12T01:00:00.000Z").expect("合写法");
    run.start(Pace::Dynamic, "盯着".to_string(), at);
    let mut first = 0;
    run.arm(|generation| {
        first = generation;
        tokio::spawn(async {}).abort_handle()
    });
    let mut second = 0;
    run.arm(|generation| {
        second = generation;
        tokio::spawn(async {}).abort_handle()
    });
    assert!(!run.due(first), "换掉的那一代不认");
    assert!(run.due(second));
    assert!(run.stop());
    assert!(!run.due(second), "停了的不认");
    assert!(!run.stop(), "没有的停不了");
}
