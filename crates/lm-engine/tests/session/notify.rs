//! 「空了告诉我」（L7 三，`docs/00-设计.md` 第五节第 8 条）：同一个测试进程里几个会话当几个窗口。甲订忙着的乙，乙忙完时甲
//! 收到「空了」、带乙最后一句的第一行，被叫醒开一轮；甲订没开着的丙，当场收到 `closed`；甲订闲着的乙、乙关了窗口，甲收到
//! `closed`。一直忙到过期的那一种：计时在 `peers/tests.rs` 里拨钟测，内核照到点记 `expired` 在内核的 `scenario/watch.rs`
//! （这里拨不了会话的钟：内核照系统时间查到没到点）。登记在乙忙完以后才读到的、一边的钟停在前头的（LM-12）见最后三条。

use lm_engine::session::Handle;
use lm_engine::session::testkit::Play;
use lm_kernel::kernel::event::{Body, Event, IdleReason, PeerIdle};
use lm_kernel::kernel::id::SessionId;
use lm_kernel::kernel::origin::By;

use crate::inbox::{answering, close, closed, drop_in, said, script, words};
use crate::support::jobs::{base_system, full, talk, turns_ended};
use crate::support::*;

/// `send_message` 只订不发：等 `to`（短编号）空下来。
fn watch_for(to: &str) -> String {
    serde_json::json!({ "to": to, "notify_when_idle": true }).to_string()
}

/// 日志里的 `peer.idle`，和记它的是谁。
fn notices(log: &[Event]) -> Vec<(&PeerIdle, &By)> {
    log.iter()
        .filter_map(|event| match &event.body {
            Body::PeerIdle(idle) => Some((idle, &event.by)),
            _ => None,
        })
        .collect()
}

/// 甲订 `to`，说完这一轮；收到「空了」的通知会再开一轮，剧本里排着那一轮的话（排完了照请求现回的不记请求）。
async fn watcher(home: &Home, to: &str) -> (Handle, lm_engine::session::testkit::Script) {
    let models = script([
        Play::calls(&[("send_message", &watch_for(to))]),
        Play::Says("等着。"),
        Play::Says("知道了。"),
    ]);
    let a = home.create_as(&models, &base_system(), full(home)).await;
    talk(&a, "m1", "它空了告诉我").await;
    assert_eq!(
        said(&home.log(a.id()))[0],
        format!("You will get a notice when {to} is next idle.")
    );
    (a, models)
}

#[tokio::test]
async fn she_hears_when_a_busy_window_goes_idle() {
    let home = Home::new();
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
    let b_models = script([
        Play::calls(&[("shell", &wait)]),
        Play::Says("CI 修好了。\n细节在日志里。"),
    ]);
    let b = home.create_as(&b_models, &base_system(), full(&home)).await;
    let mut b_pushes = watch(&b).await;
    ask(&b, "m1", say("修一下 CI")).await.expect("会话在跑");
    let short = b.id().short().to_string();
    let (a, a_models) = watcher(&home, &short).await;
    let opened = tokio::task::spawn_blocking(move || std::fs::write(&fifo, "ok\n"));
    within("放它走", opened)
        .await
        .expect("写得进")
        .expect("写得进");
    until_turn_ends(&mut b_pushes).await;
    let log = until_logged(&home, a.id(), |log| turns_ended(log) == 2).await;
    let got = notices(&log);
    assert_eq!(got.len(), 1, "{got:?}");
    let (idle, by) = got[0];
    assert_eq!(idle.reason, IdleReason::Idle);
    assert_eq!(
        idle.status.as_deref(),
        Some("CI 修好了。"),
        "带最后一句的第一行"
    );
    assert!(matches!(by, By::Session(session) if &session.id == b.id()));
    let request = words(&a_models.requests()[2].1);
    assert!(
        request.contains(&format!(
            "<session-idle session=\"{short}\" reason=\"idle\">"
        )),
        "叫醒她的那一轮看得到：{request}"
    );
}

#[tokio::test]
async fn a_closed_window_is_known_at_once() {
    let home = Home::new();
    let c = closed(&home).await;
    let (a, _models) = watcher(&home, c.short()).await;
    let log = until_logged(&home, a.id(), |log| !notices(log).is_empty()).await;
    let (idle, by) = notices(&log)[0];
    assert_eq!((&idle.session, &idle.reason), (&c, &IdleReason::Closed));
    assert_eq!(by, &By::Kernel);
    assert!(
        !inbox_has_letters(&home, &c),
        "没开着的不放登记：它下次打开不会再回一封"
    );
}

#[tokio::test]
async fn closing_a_watched_window_tells_the_watcher() {
    let home = Home::new();
    let b = home
        .create_as(&answering(), &base_system(), full(&home))
        .await;
    let b_id = b.id().clone();
    let (a, _models) = watcher(&home, b_id.short()).await;
    // 甲放的那封登记什么时候到看不见：照甲日志里订的那一刻，测试自己再放一封一样的（同一个会话只记一个），等它被读掉，
    // 乙的名单上就有甲了（闲着订的，不上膛、不当场回）。再关。
    let since = home
        .log(a.id())
        .iter()
        .find(|event| matches!(&event.body, Body::ToolResult(result) if !result.effects.is_empty()))
        .map(|event| event.at.unix_millis())
        .expect("订过");
    let letter = serde_json::json!({ "kind": "watch", "from": a.id(), "since": since }).to_string();
    drop_in(&home, &b_id, "watch.json", &letter);
    let mine = home.root.session(&b_id).join("inbox").join("watch.json");
    until_logged(&home, a.id(), |_| !mine.exists()).await;
    assert!(notices(&home.log(a.id())).is_empty(), "闲着订的不当场回");
    close(b).await;
    let log = until_logged(&home, a.id(), |log| !notices(log).is_empty()).await;
    let (idle, by) = notices(&log)[0];
    assert_eq!((&idle.session, &idle.reason), (&b_id, &IdleReason::Closed));
    assert_eq!(by, &By::Kernel);
}

/// 会话 `session` 的收件目录里还有没读的。
fn inbox_has_letters(home: &Home, session: &SessionId) -> bool {
    std::fs::read_dir(home.root.session(session).join("inbox"))
        .is_ok_and(|mut entries| entries.next().is_some())
}

/// 造一个 FIFO，交回等它的那条 `shell` 的参数：读到有人写进去才走。
fn fifo_wait(home: &Home, name: &str) -> (std::path::PathBuf, String) {
    let work = home.scratch.0.join("work");
    std::fs::create_dir_all(&work).expect("建得了");
    let fifo = work.join(name);
    let made = std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .expect("起得来");
    assert!(made.success());
    let wait =
        serde_json::json!({ "command": format!("cat {}", fifo.display()), "description": "等" })
            .to_string();
    (fifo, wait)
}

/// 往 FIFO 里写一行，放等它的那条命令走。
async fn release(fifo: std::path::PathBuf) {
    let opened = tokio::task::spawn_blocking(move || std::fs::write(&fifo, "ok\n"));
    within("放它走", opened)
        .await
        .expect("写得进")
        .expect("写得进");
}

/// 甲的时钟停在同一毫秒（系统时间往回拨了，引擎线第七趟）：甲两次订乙，两次订的起算时刻一样；乙两次忙完，甲两次都收到「空了」、
/// 都被叫醒，第二封不因为和第一封同一个编号被当成同一次。
#[tokio::test]
async fn she_hears_each_time_while_her_clock_stands_still() {
    let home = Home::new();
    let (first, wait_first) = fifo_wait(&home, "one");
    let (second, wait_second) = fifo_wait(&home, "two");
    let b_models = script([
        Play::calls(&[("shell", &wait_first)]),
        Play::Says("第一次忙完了。"),
        Play::calls(&[("shell", &wait_second)]),
        Play::Says("第二次忙完了。"),
    ]);
    let b = home.create_as(&b_models, &base_system(), full(&home)).await;
    let short = b.id().short().to_string();
    let a_models = script([
        Play::calls(&[("send_message", &watch_for(&short))]),
        Play::Says("等着。"),
        Play::Says("知道了。"),
        Play::calls(&[("send_message", &watch_for(&short))]),
        Play::Says("再等着。"),
        Play::Says("又知道了。"),
    ]);
    let a = home.create_as(&a_models, &base_system(), full(&home)).await;
    let a_id = home.stuck(a).await;
    let a = home
        .load_at(&a_id, &a_models, &base_system(), &full(&home).cwd)
        .await;
    let mut b_pushes = watch(&b).await;
    for (round, fifo) in [(1, first), (2, second)] {
        ask(&b, &format!("m{round}"), say("修一下 CI"))
            .await
            .expect("会话在跑");
        talk(&a, &format!("m{round}"), "它空了告诉我").await;
        release(fifo).await;
        until_turn_ends(&mut b_pushes).await;
        let log = until_logged(&home, &a_id, |log| turns_ended(log) == 2 * round).await;
        assert_eq!(notices(&log).len(), round, "第 {round} 次忙完收到了");
    }
}

/// 兜底扫一小时一次：下面几条里乙什么时候读到登记由测试定（挪开、挪回它的门铃再敲）。
const NEVER: std::time::Duration = std::time::Duration::from_secs(3600);

/// 把会话 `session` 的门铃挪到旁边：这以后放进它收件目录的信敲不到它（兜底扫是 [`NEVER`]，就一直不读）。交回挪回去的那一下
/// 要的两处。
fn bell_aside(home: &Home, session: &SessionId) -> (std::path::PathBuf, std::path::PathBuf) {
    let bell = lm_engine::session::testkit::bell_spots(&home.root, session)
        .into_iter()
        .next()
        .expect("绑了门铃");
    let aside = bell.with_extension("aside");
    std::fs::rename(&bell, &aside).expect("挪得开");
    (bell, aside)
}

/// 把挪开的门铃挪回去、敲一下：它当场去读收件目录。
fn bell_back(
    home: &Home,
    session: &SessionId,
    (bell, aside): (std::path::PathBuf, std::path::PathBuf),
) {
    std::fs::rename(&aside, &bell).expect("挪得回");
    lm_engine::session::testkit::ring(&home.root, session);
}

/// 等到会话 `session` 的收件目录里有（`want`）、没有（`!want`）没读的信。
async fn until_inbox(home: &Home, session: &SessionId, want: bool) {
    within("收件目录", async {
        while inbox_has_letters(home, session) != want {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    })
    .await;
}

/// 甲订忙着的乙，乙却在忙完以后才读到这封登记（乙的门铃挪开了，登记放进去、乙忙完，再挪回来敲）：订在忙完以前，乙读到时照样
/// 当场回「空了」（`peers/waiters.rs`：订的时刻不晚于上一次忙完的时刻的，当场上膛）。`stuck`：甲的时钟停在 2100 年。
async fn hears_a_watch_read_late(stuck: bool) {
    let mut home = Home::new();
    home.root.bell.every = NEVER;
    let (fifo, wait) = fifo_wait(&home, "late");
    let b_models = script([Play::calls(&[("shell", &wait)]), Play::Says("忙完了。")]);
    let b = home.create_as(&b_models, &base_system(), full(&home)).await;
    let short = b.id().short().to_string();
    let a_models = script([
        Play::calls(&[("send_message", &watch_for(&short))]),
        Play::Says("等着。"),
        Play::Says("知道了。"),
    ]);
    let mut a = home.create_as(&a_models, &base_system(), full(&home)).await;
    if stuck {
        let a_id = home.stuck(a).await;
        a = home
            .load_at(&a_id, &a_models, &base_system(), &full(&home).cwd)
            .await;
    }
    let a_id = a.id().clone();
    let mut b_pushes = watch(&b).await;
    ask(&b, "m1", say("修一下 CI")).await.expect("会话在跑");
    let aside = bell_aside(&home, b.id());
    talk(&a, "m1", "它空了告诉我").await;
    until_inbox(&home, b.id(), true).await;
    release(fifo).await;
    until_turn_ends(&mut b_pushes).await;
    bell_back(&home, b.id(), aside);
    let log = until_logged(&home, &a_id, |log| turns_ended(log) == 2).await;
    let got = notices(&log);
    assert_eq!(got.len(), 1, "{got:?}");
    assert_eq!(got[0].0.reason, IdleReason::Idle);
    assert_eq!(got[0].0.status.as_deref(), Some("忙完了。"));
}

#[tokio::test]
async fn she_hears_when_her_watch_is_read_after_it_went_idle() {
    hears_a_watch_read_late(false).await;
}

/// LM-12：甲的时钟停在 2100 年，登记里的起算时刻是 2100 年；乙原来拿它和自己的钟记的忙完时刻比，当成「订在忙完以后」，不回，
/// 甲一直等不到（macOS 的 CI 上 `she_hears_each_time_while_her_clock_stands_still` 偶发红在这）。
#[tokio::test]
async fn she_hears_when_her_watch_is_read_late_while_her_clock_stands_still() {
    hears_a_watch_read_late(true).await;
}

/// LM-12，反过来的那一种：乙的时钟停在 2100 年、甲的没停。乙上一轮早说完了，甲才订：不当场回（回的是上一轮的旧话），等乙下一次
/// 忙完、带那一轮的话。乙原来拿自己的钟记的忙完时刻（2100 年）和甲的钟记的起算时刻比，当成「订在忙完以前」，当场回了上一轮的。
#[tokio::test]
async fn she_does_not_get_its_old_answer_while_its_clock_stands_still() {
    let mut home = Home::new();
    home.root.bell.every = NEVER;
    let b_models = script([Play::Says("上一轮的话。"), Play::Says("这一轮的话。")]);
    let b = home.create_as(&b_models, &base_system(), full(&home)).await;
    let b_id = home.stuck(b).await;
    let b = home
        .load_at(&b_id, &b_models, &base_system(), &full(&home).cwd)
        .await;
    talk(&b, "m1", "说一句").await;
    let a_models = script([
        Play::calls(&[("send_message", &watch_for(b_id.short()))]),
        Play::Says("等着。"),
        Play::Says("知道了。"),
    ]);
    let a = home.create_as(&a_models, &base_system(), full(&home)).await;
    // 登记先攒在乙的收件目录里，再叫乙读掉：读掉了（交给乙的 actor 排在下一句话前面）再让乙忙下一轮。
    let aside = bell_aside(&home, &b_id);
    talk(&a, "m1", "它空了告诉我").await;
    until_inbox(&home, &b_id, true).await;
    bell_back(&home, &b_id, aside);
    until_inbox(&home, &b_id, false).await;
    talk(&b, "m2", "再说一句").await;
    let log = until_logged(&home, a.id(), |log| !notices(log).is_empty()).await;
    let got = notices(&log);
    assert_eq!(got[0].0.reason, IdleReason::Idle);
    assert_eq!(
        got[0].0.status.as_deref(),
        Some("这一轮的话。"),
        "带的是订了以后那一轮的话"
    );
}
