//! 收件目录（L7 二，`docs/00-设计.md` 第五节第 8 条）：同一个测试进程里起几个会话当几个窗口（同一个数据根，各有各的把手）。
//! 甲发给开着、闲着的乙，乙被叫醒开一轮，请求里看得到是哪个会话发的；乙在回合中途的，话进下一步；发给没开着的丙，`sessions`
//! 里它是 `closed`，丙下次打开时收到；写到一半的、坏的、认不出的不弄坏会话、挪到一边；两封照文件名的先后到；同时发的都到；
//! 收件目录写不进去的发的一方拿到一句看得懂的错；防刷屏在发的一方当场拦，对方回过话以后又能发。

use std::os::unix::fs::PermissionsExt;

use lm_engine::session::Handle;
use lm_engine::session::testkit::{Play, Script};
use lm_kernel::kernel::block::Block;
use lm_kernel::kernel::event::{Body, Event};
use lm_kernel::kernel::id::SessionId;
use lm_kernel::kernel::origin::By;
use lm_kernel::kernel::request::{Message, Request};

use crate::support::jobs::{base_system, full, results, talk, turns_ended};
use crate::support::*;

/// 照 `plays` 回。
pub(crate) fn script(plays: impl IntoIterator<Item = Play>) -> Script {
    Script::new(plays)
}

/// `send_message` 的参数：发给 `to`（短编号）。
pub(crate) fn send(to: &Handle, text: &str) -> String {
    serde_json::json!({ "to": to.id().short(), "message": text }).to_string()
}

/// 请求里的全部字（不算 system），连成一段。
pub(crate) fn words(request: &Request) -> String {
    request
        .messages
        .iter()
        .flat_map(|message| match message {
            Message::User { blocks }
            | Message::Assistant { blocks }
            | Message::Tool { blocks, .. } => blocks.iter(),
        })
        .filter_map(|block| match block {
            Block::Text(text) => Some(text.text.as_str()),
            _ => None,
        })
        .collect()
}

/// 日志里每个工具结果的字。
pub(crate) fn said(log: &[Event]) -> Vec<String> {
    results(log)
        .iter()
        .map(|result| {
            result
                .blocks
                .iter()
                .filter_map(|block| match block {
                    Block::Text(text) => Some(text.text.clone()),
                    _ => None,
                })
                .collect::<String>()
                .trim_end()
                .to_string()
        })
        .collect()
}

/// 日志里会话 `from` 发来的话的字，照先后。
pub(crate) fn heard(log: &[Event], from: &SessionId) -> Vec<String> {
    log.iter()
        .filter(|event| matches!(&event.by, By::Session(session) if &session.id == from))
        .filter_map(|event| match &event.body {
            Body::MessageUser(message) => Some(
                message
                    .blocks
                    .iter()
                    .filter_map(|block| match block {
                        Block::Text(text) => Some(text.text.clone()),
                        _ => None,
                    })
                    .collect::<String>(),
            ),
            _ => None,
        })
        .collect()
}

/// 关掉一个窗口里的会话：有计划地停下，等它的 actor 退出（推送断了）。
pub(crate) async fn close(handle: Handle) {
    let mut pushes = watch(&handle).await;
    stop(&handle).await;
    drop(handle);
    while within("等它退出", pushes.next()).await.is_ok() {}
}

/// 造一个会话，关掉，交回编号。
pub(crate) async fn closed(home: &Home) -> SessionId {
    let handle = home
        .create_as(&script([]), &base_system(), full(home))
        .await;
    let id = handle.id().clone();
    close(handle).await;
    id
}

/// 剧本排完了一律回一句：几封话分几轮到都接得住。
pub(crate) fn answering() -> Script {
    Script::new([]).answers(|_| Play::Says("收到。"))
}

/// 会话 `session` 的收件目录。
fn inbox(home: &Home, session: &SessionId) -> std::path::PathBuf {
    home.root.session(session).join("inbox")
}

/// 收件目录里放一封：先写 `.` 开头的临时文件再改名，再敲门铃（同发的一方）。
pub(crate) fn drop_in(home: &Home, session: &SessionId, name: &str, body: &str) {
    let dir = inbox(home, session);
    std::fs::create_dir_all(&dir).expect("建得了");
    let temp = dir.join(format!(".{name}"));
    std::fs::write(&temp, body).expect("写得进");
    std::fs::rename(&temp, dir.join(name)).expect("改得了名");
    lm_engine::session::testkit::ring(&home.root, session);
}

#[tokio::test]
async fn a_message_wakes_an_idle_window_and_shows_who_sent_it() {
    let home = Home::new();
    let b_models = script([Play::Says("收到。")]);
    let b = home.create_as(&b_models, &base_system(), full(&home)).await;
    let mut b_pushes = watch(&b).await;
    let message = send(&b, "CI 修好了");
    let a_models = script([
        Play::calls(&[("send_message", &message)]),
        Play::Says("发了。"),
    ]);
    let a = home.create_as(&a_models, &base_system(), full(&home)).await;
    talk(&a, "m1", "告诉它").await;
    assert_eq!(
        said(&home.log(a.id()))[0],
        format!("Message sent to {}.", b.id().short())
    );
    until_turn_ends(&mut b_pushes).await;
    let request = words(&b_models.requests()[0].1);
    assert!(
        request.contains(&format!("<session-message from=\"{}\">", a.id().short()))
            && request.contains("CI 修好了"),
        "{request}"
    );
    assert_eq!(heard(&home.log(b.id()), a.id()), ["CI 修好了"]);
    assert!(
        std::fs::read_dir(inbox(&home, b.id()))
            .expect("有收件目录")
            .next()
            .is_none(),
        "收下的删掉了"
    );
}

#[tokio::test]
async fn a_message_mid_turn_comes_in_at_the_next_step() {
    let home = Home::new();
    // 在她的工作目录里：读别处的文件要人确认，没人能确认的会被拒。
    let work = home.scratch.0.join("work");
    std::fs::create_dir_all(&work).expect("建得了");
    let fifo = work.join("go");
    let made = std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .expect("起得来");
    assert!(made.success());
    let wait =
        serde_json::json!({ "command": format!("cat {}", fifo.display()), "description": "等" })
            .to_string();
    let b_models = script([Play::calls(&[("shell", &wait)]), Play::Says("看到了。")]);
    let b = home.create_as(&b_models, &base_system(), full(&home)).await;
    let mut b_pushes = watch(&b).await;
    ask(&b, "m1", say("等一下")).await.expect("会话在跑");
    let message = send(&b, "顺便看看日志");
    let a_models = script([
        Play::calls(&[("send_message", &message)]),
        Play::Says("发了。"),
    ]);
    let a = home.create_as(&a_models, &base_system(), full(&home)).await;
    talk(&a, "m1", "告诉它").await;
    // 等它收下：那时它的命令还在等。
    let from = a.id().clone();
    within("等它收下", async {
        loop {
            let next = b_pushes.next().await.expect("订阅没断");
            if let lm_engine::session::Pushed::Events(events) = &*next
                && events.iter().any(|event| {
                    matches!(&event.by, By::Session(session) if session.id == from)
                        && event.turn.is_none()
                })
            {
                return;
            }
        }
    })
    .await;
    let log = home.log(b.id());
    assert_eq!(turns_ended(&log), 0, "还在回合中途：{:?}", said(&log));
    let opened = tokio::task::spawn_blocking(move || std::fs::write(&fifo, "ok\n"));
    within("放它走", opened)
        .await
        .expect("写得进")
        .expect("写得进");
    until_turn_ends(&mut b_pushes).await;
    let requests = b_models.requests();
    assert_eq!(requests.len(), 2, "一轮里两步");
    assert!(!words(&requests[0].1).contains("顺便看看日志"));
    assert!(words(&requests[1].1).contains("顺便看看日志"), "进了下一步");
}

#[tokio::test]
async fn a_closed_session_is_listed_as_closed_and_reads_its_message_when_opened() {
    let home = Home::new();
    let c = closed(&home).await;
    let short = c.short().to_string();
    let message = serde_json::json!({ "to": short, "message": "回来看看" }).to_string();
    let a_models = script([
        Play::calls(&[("sessions", "{}")]),
        Play::calls(&[("send_message", &message)]),
        Play::Says("发了。"),
    ]);
    let a = home.create_as(&a_models, &base_system(), full(&home)).await;
    talk(&a, "m1", "告诉它").await;
    let results = said(&home.log(a.id()));
    assert!(
        results[0].contains(&format!("{short} ")) && results[0].contains(": closed,"),
        "{}",
        results[0]
    );
    assert_eq!(results[1], format!("Message sent to {short}."));
    assert!(heard(&home.log(&c), a.id()).is_empty(), "没开着的不替它开");
    let c_models = script([Play::Says("看到了。")]);
    let reopened = home.load_with(&c, &c_models, &base_system()).await;
    let log = until_logged(&home, &c, |log| turns_ended(log) == 1).await;
    assert_eq!(heard(&log, a.id()), ["回来看看"], "打开以后收到");
    assert!(words(&c_models.requests()[0].1).contains("回来看看"));
    drop(reopened);
}

#[tokio::test]
async fn broken_letters_are_set_aside_and_letters_come_in_name_order() {
    let home = Home::new();
    let b = closed(&home).await;
    let a = SessionId::parse("0192f3a0-2222-7abc-8def-5566778899aa").expect("合写法");
    let letter = |text: &str, call: &str| {
        serde_json::json!({ "kind": "message", "from": a, "call": call, "text": text }).to_string()
    };
    // 后写的名字排在前面：照文件名，不照写的先后。
    drop_in(&home, &b, "0002.json", &letter("二", "call_1_2"));
    drop_in(&home, &b, "0001.json", &letter("一", "call_1_1"));
    drop_in(&home, &b, "0000-half.json", "{\"kind\":\"message\",\"fr");
    drop_in(&home, &b, "0003-odd.json", "{\"kind\":\"poem\"}");
    drop_in(&home, &b, "notes.txt", "随手放的");
    std::fs::write(inbox(&home, &b).join(".0004-writing.json"), "{\"kind\"").expect("写得进");
    let opened = home.load_with(&b, &answering(), &base_system()).await;
    let bad_dir = home.root.session(&b).join("inbox.bad");
    let set_aside = || std::fs::read_dir(&bad_dir).map_or(0, Iterator::count);
    let log = until_logged(&home, &b, |log| {
        heard(log, &a).len() == 2 && set_aside() == 3
    })
    .await;
    assert_eq!(heard(&log, &a), ["一", "二"], "照文件名的先后");
    let left: Vec<String> = std::fs::read_dir(inbox(&home, &b))
        .expect("还在")
        .map(|entry| {
            entry
                .expect("读得了")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert_eq!(left, [".0004-writing.json"], "写到一半的不动");
    // 开着时再来一封坏的、一封好的：会话照样收。
    drop_in(&home, &b, "0005-bad.json", "不是 JSON");
    drop_in(&home, &b, "0006.json", &letter("三", "call_1_3"));
    let log = until_logged(&home, &b, |log| heard(log, &a).len() == 3).await;
    assert_eq!(heard(&log, &a), ["一", "二", "三"]);
    drop(opened);
}

#[tokio::test]
async fn two_windows_sending_at_once_both_arrive() {
    let home = Home::new();
    let b_models = answering();
    let b = home.create_as(&b_models, &base_system(), full(&home)).await;
    let a_models = script([
        Play::calls(&[("send_message", &send(&b, "甲来了"))]),
        Play::Says("发了。"),
    ]);
    let c_models = script([
        Play::calls(&[("send_message", &send(&b, "丙来了"))]),
        Play::Says("发了。"),
    ]);
    let a = home.create_as(&a_models, &base_system(), full(&home)).await;
    let c = home.create_as(&c_models, &base_system(), full(&home)).await;
    tokio::join!(talk(&a, "m1", "告诉它"), talk(&c, "m1", "告诉它"));
    let log = until_logged(&home, b.id(), |log| {
        heard(log, a.id()).len() + heard(log, c.id()).len() == 2
    })
    .await;
    assert_eq!(heard(&log, a.id()), ["甲来了"]);
    assert_eq!(heard(&log, c.id()), ["丙来了"]);
}

#[tokio::test]
async fn an_unwritable_inbox_gives_the_sender_a_plain_error() {
    let home = Home::new();
    let b = closed(&home).await;
    let dir = inbox(&home, &b);
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).expect("改得了");
    let message = serde_json::json!({ "to": b.short(), "message": "在吗" }).to_string();
    let a_models = script([
        Play::calls(&[("send_message", &message)]),
        Play::Says("没发出去。"),
    ]);
    let a = home.create_as(&a_models, &base_system(), full(&home)).await;
    talk(&a, "m1", "告诉它").await;
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).expect("改得回");
    let results = said(&home.log(a.id()));
    // 跑测试的是 root 时只读挡不住：那时照常发出去。
    if !results[0].starts_with("Message sent") {
        assert_eq!(results[0], "The message could not be delivered.");
    }
    assert_eq!(turns_ended(&home.log(a.id())), 1, "发的一方照常往下走");
}

#[tokio::test]
async fn flooding_is_refused_on_the_sending_side_until_the_other_side_answers() {
    let home = Home::new();
    let b = closed(&home).await;
    let to = b.short().to_string();
    let say_to = |text: &str| serde_json::json!({ "to": to, "message": text }).to_string();
    let mut plays: Vec<Play> = (0..6)
        .map(|k| Play::calls(&[("send_message", &say_to(&format!("第 {k} 句")))]))
        .collect();
    plays.push(Play::calls(&[("send_message", &say_to("第 0 句"))]));
    plays.push(Play::Says("先这样。"));
    plays.push(Play::calls(&[("send_message", &say_to("收到回话了"))]));
    plays.push(Play::Says("好。"));
    let a_models = script(plays);
    let a = home.create_as(&a_models, &base_system(), full(&home)).await;
    talk(&a, "m1", "一直发").await;
    let results = said(&home.log(a.id()));
    for result in &results[..5] {
        assert_eq!(result, &format!("Message sent to {to}."));
    }
    assert_eq!(
        results[5],
        format!(
            "Too many messages to {to} just now. Put the rest into one message and send it later."
        )
    );
    assert_eq!(results[6], format!("{to} already has this exact message."));
    // 乙打开、读到五句、回一句：甲被叫醒，又能发了。
    let back = serde_json::json!({ "to": a.id().short(), "message": "都看到了" }).to_string();
    let b_models = script([
        Play::calls(&[("send_message", &back)]),
        Play::Says("回了。"),
    ])
    .answers(|_| Play::Says("收到。"));
    let opened = home.load_with(&b, &b_models, &base_system()).await;
    let log = until_logged(&home, a.id(), |log| turns_ended(log) == 2).await;
    assert_eq!(heard(&log, &b), ["都看到了"]);
    assert_eq!(
        said(&log)[7],
        format!("Message sent to {to}."),
        "回过话以后又能发"
    );
    let b_log = until_logged(&home, &b, |log| heard(log, a.id()).len() == 6).await;
    assert_eq!(
        heard(&b_log, a.id()).last().map(String::as_str),
        Some("收到回话了")
    );
    drop(opened);
}

#[tokio::test]
async fn temp_files_left_by_a_killed_sender_are_swept_when_the_inbox_is_opened() {
    let home = Home::new();
    let b = closed(&home).await;
    let dir = inbox(&home, &b);
    std::fs::create_dir_all(&dir).expect("建得了");
    let old = dir.join(".0192-killed.json");
    let fresh = dir.join(".0192-writing.json");
    std::fs::write(&old, "{\"kind\":\"mess").expect("写得进");
    std::fs::write(&fresh, "{\"kind\":\"mess").expect("写得进");
    // 改修改时刻，不睡：一个是 11 分钟以前的，一个是 9 分钟以前的。
    let ago =
        |minutes: u64| std::time::SystemTime::now() - std::time::Duration::from_secs(minutes * 60);
    let touch = |path: &std::path::Path, at| {
        std::fs::File::options()
            .write(true)
            .open(path)
            .and_then(|file| file.set_modified(at))
            .expect("改得了修改时刻");
    };
    touch(&old, ago(11));
    touch(&fresh, ago(9));
    let opened = home.load_with(&b, &answering(), &base_system()).await;
    until_logged(&home, &b, |_| !old.exists()).await;
    assert!(fresh.exists(), "10 分钟以内的不动：可能有人正在写");
    drop(opened);
}
