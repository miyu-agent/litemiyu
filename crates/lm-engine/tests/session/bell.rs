//! 窗口之间的门铃（LM-4，`docs/00-设计.md` 第五节第 8 条）：同一个测试进程里起几个会话当几个窗口。经链接的数据根发的话照样
//! 叫得醒；崩溃留下的套接字删了重绑在原处；关窗时删掉自己的套接字。兜底扫、删会话见下面几条（间隔照测试注入的短值，不真等
//! 30 秒）。

use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
use std::os::unix::net::UnixDatagram;
use std::path::PathBuf;
use std::time::Duration;

use lm_engine::session::testkit::{Play, bell_spots};
use lm_kernel::kernel::id::SessionId;

use crate::inbox::{close, heard, script, send};
use crate::support::jobs::{base_system, full, talk};
use crate::support::*;

/// 门铃叫得醒的等多久：叫醒是毫秒级的，给慢机器留足；等不到就是没叫醒。
const RUNG: Duration = Duration::from_secs(20);

/// 兜底扫一小时一次：这一条只靠门铃。
const NEVER: Duration = Duration::from_secs(3600);

/// 乙收到 `from` 发来的 `text`，最多等 `wait`。
async fn until_heard(home: &Home, b: &SessionId, from: &SessionId, text: &str, wait: Duration) {
    let got = tokio::time::timeout(wait, async {
        loop {
            if heard(&home.log(b), from).iter().any(|said| said == text) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await;
    assert!(got.is_ok(), "{wait:?} 内乙没收到「{text}」");
}

/// 甲（在 `home` 里造）给乙发一句 `text`，等甲这一轮说完。
async fn a_sends(home: &Home, b: &SessionId, text: &str) -> SessionId {
    let message = serde_json::json!({ "to": b.short(), "message": text }).to_string();
    let a_models = script([
        Play::calls(&[("send_message", &message)]),
        Play::Says("发了。"),
    ]);
    let a = home.create_as(&a_models, &base_system(), full(home)).await;
    talk(&a, "m1", "告诉它").await;
    a.id().clone()
}

/// 这一处是不是有人在听的套接字。
fn live(path: &std::path::Path) -> bool {
    UnixDatagram::unbound().is_ok_and(|probe| probe.connect(path).is_ok())
}

#[tokio::test]
async fn a_message_sent_by_way_of_a_linked_data_root_wakes_the_window() {
    let mut home = Home::new();
    home.root.bell.every = NEVER;
    let b_models = script([]).answers(|_| Play::Says("收到。"));
    let b = home.create_as(&b_models, &base_system(), full(&home)).await;
    // 甲的数据根经一个链接、还多一段 `..`：两边的路径写法不一样，指的是同一个目录。
    let linked = home.scratch.0.join("linked");
    std::os::unix::fs::symlink(&home.root.data, &linked).expect("建得了链接");
    let mut root = home.root.clone();
    root.data = linked.join("sessions").join("..");
    let other = Home {
        scratch: Scratch(home.scratch.0.join("unused")),
        root,
        home: home.home.clone(),
        configs: home.configs.clone(),
    };
    let message = send(&b, "经链接发来的");
    let a_models = script([
        Play::calls(&[("send_message", &message)]),
        Play::Says("发了。"),
    ]);
    let a = other
        .create_as(&a_models, &base_system(), full(&home))
        .await;
    talk(&a, "m1", "告诉它").await;
    until_heard(&home, b.id(), a.id(), "经链接发来的", RUNG).await;
}

#[tokio::test]
async fn a_stale_socket_left_by_a_crash_is_rebound_in_place() {
    let mut home = Home::new();
    home.root.bell.every = NEVER;
    let b = home
        .create_as(&script([]), &base_system(), full(&home))
        .await;
    let id = b.id().clone();
    close(b).await;
    // 崩溃：套接字关了，文件留着。
    let spot = bell_spots(&home.root, &id).remove(0);
    drop(UnixDatagram::bind(&spot).expect("绑得上"));
    assert!(spot.exists() && !live(&spot));
    let b_models = script([]).answers(|_| Play::Says("收到。"));
    let opened = home.load_with(&id, &b_models, &base_system()).await;
    assert!(live(&spot), "删了重绑在原处，不退到下一处");
    let a = a_sends(&home, &id, "崩溃以后").await;
    until_heard(&home, &id, &a, "崩溃以后", RUNG).await;
    drop(opened);
}

#[tokio::test]
async fn closing_the_window_removes_its_socket() {
    let home = Home::new();
    let b = home
        .create_as(&script([]), &base_system(), full(&home))
        .await;
    let id = b.id().clone();
    let spots = bell_spots(&home.root, &id);
    assert_eq!(spots.len(), 1, "用的是运行时目录，没建退路：{spots:?}");
    let spot = spots[0].clone();
    assert!(live(&spot), "开着：绑在运行时目录");
    let meta = std::fs::symlink_metadata(&spot).expect("在");
    assert_eq!(meta.permissions().mode() & 0o077, 0, "别人不可写");
    close(b).await;
    assert!(!spot.exists(), "关了：套接字文件删掉");
    let run: Vec<PathBuf> = std::fs::read_dir(home.scratch.0.join("run"))
        .expect("在")
        .map(|entry| entry.expect("读得了").path())
        .collect();
    assert!(run.is_empty(), "运行时目录里什么都没留：{run:?}");
}

#[tokio::test]
async fn without_a_runtime_dir_the_bell_goes_to_an_own_folder_under_tmp() {
    let mut home = Home::new();
    home.root.bell.runtime = None;
    home.root.bell.every = NEVER;
    let b_models = script([]).answers(|_| Play::Says("收到。"));
    let b = home.create_as(&b_models, &base_system(), full(&home)).await;
    let uid = std::fs::metadata(&home.scratch.0).expect("在").uid();
    let folder = home.root.bell.tmp.join(format!("litemiyu-{uid}"));
    let meta = std::fs::symlink_metadata(&folder).expect("建了自己的目录");
    assert_eq!(meta.permissions().mode() & 0o777, 0o700);
    let spots = bell_spots(&home.root, b.id());
    assert_eq!(spots.len(), 1);
    assert!(spots[0].starts_with(&folder) && live(&spots[0]));
    let a = a_sends(&home, b.id(), "退到 tmp").await;
    until_heard(&home, b.id(), &a, "退到 tmp", RUNG).await;
}

/// 测试自己的「`/tmp`」：`litemiyu-<uid>` 被别人抢先建了、权限宽。
fn hijacked(home: &Home) -> PathBuf {
    let tmp = home.scratch.0.join("tmp");
    let uid = std::fs::metadata(&home.scratch.0).expect("在").uid();
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o777)
        .create(tmp.join(format!("litemiyu-{uid}")))
        .expect("建得了");
    std::fs::set_permissions(
        tmp.join(format!("litemiyu-{uid}")),
        std::fs::Permissions::from_mode(0o777),
    )
    .expect("改得了");
    tmp
}

#[tokio::test]
async fn a_lost_bell_is_made_up_for_by_the_scan() {
    let mut home = Home::new();
    home.root.bell.every = Duration::from_millis(200);
    let b_models = script([]).answers(|_| Play::Says("收到。"));
    let b = home.create_as(&b_models, &base_system(), full(&home)).await;
    // 门铃丢了：套接字文件被人删了，甲敲不到。
    for spot in bell_spots(&home.root, b.id()) {
        std::fs::remove_file(spot).expect("删得了");
    }
    let a = a_sends(&home, b.id(), "门铃丢了").await;
    until_heard(&home, b.id(), &a, "门铃丢了", RUNG).await;
}

#[tokio::test]
async fn a_window_without_any_bell_still_hears_by_scanning() {
    let mut home = Home::new();
    home.root.bell.runtime = None;
    home.root.bell.tmp = hijacked(&home);
    home.root.bell.bare = Duration::from_millis(200);
    let b_models = script([]).answers(|_| Play::Says("收到。"));
    let b = home.create_as(&b_models, &base_system(), full(&home)).await;
    assert!(bell_spots(&home.root, b.id()).is_empty(), "被占的目录不用");
    let a = a_sends(&home, b.id(), "没有门铃").await;
    until_heard(&home, b.id(), &a, "没有门铃", RUNG).await;
}

#[tokio::test]
async fn deleting_a_session_removes_a_socket_left_by_a_crash() {
    let home = Home::new();
    let b = home
        .create_as(&script([]), &base_system(), full(&home))
        .await;
    let id = b.id().clone();
    close(b).await;
    let spot = bell_spots(&home.root, &id).remove(0);
    drop(UnixDatagram::bind(&spot).expect("绑得上"));
    assert!(spot.exists());
    lm_engine::session::remove_session(&home.root, id.as_str()).expect("删得了");
    assert!(!spot.exists(), "删会话时连它的门铃一起删");
}

#[tokio::test]
async fn opening_a_session_clears_bells_nobody_listens_on() {
    // 建会话那一刻（LM-14）顺手清运行时目录里别的会话崩溃留下的套接字：没人听的删，有人听的、名字不对的、不是套接字的不动。
    let home = Home::new();
    let run = home.scratch.0.join("run");
    let stale = run.join("litemiyu-00000000000000a1.sock");
    drop(UnixDatagram::bind(&stale).expect("绑得上"));
    let heard_at = run.join("litemiyu-00000000000000a2.sock");
    let listening = UnixDatagram::bind(&heard_at).expect("绑得上");
    let beside = run.join("litemiyu-00000000000000a3.sock.4242");
    drop(UnixDatagram::bind(&beside).expect("绑得上"));
    let file = run.join("litemiyu-00000000000000a4.sock");
    std::fs::write(&file, "").expect("写得进");
    let b = home
        .create_as(&script([]), &base_system(), full(&home))
        .await;
    assert!(!stale.exists(), "没人听的删掉");
    assert!(live(&heard_at), "有人听的还在");
    assert!(beside.exists(), "换门铃时旁边的临时名字不当旧文件");
    assert!(file.exists(), "不是套接字的不动");
    let knock = UnixDatagram::unbound().expect("开得了");
    knock.send_to(b"!", &heard_at).expect("还敲得响");
    let mut buffer = [0u8; 4];
    listening.set_nonblocking(true).expect("设得了");
    assert!(listening.recv(&mut buffer).is_ok(), "收得到");
    close(b).await;
}
