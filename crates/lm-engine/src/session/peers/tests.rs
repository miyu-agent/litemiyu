//! 收件目录的一封往坏里测（L7 二）：写进去、照文件名的先后读回来；写到一半的临时文件不读；坏的、认不出的、太大的、不是文件的
//! 读出原因、挪到一边；对方的会话目录不在了、收件目录写不进去的交回错，不留临时文件。

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use lm_kernel::kernel::id::{CallId, SessionId};

use super::letter::{BAD, INBOX, Letter, post, read, scan, set_aside};
use crate::store::paths::Bell;

/// 测试自己的数据根：临时目录下（同 `window/tests.rs`），丢掉时删掉。
struct Root(PathBuf);

impl Root {
    fn new(name: &str) -> Root {
        let dir =
            std::env::temp_dir().join(format!("litemiyu-peers-{name}-{}", std::process::id()));
        drop(std::fs::remove_dir_all(&dir));
        std::fs::create_dir_all(&dir).expect("建得了");
        Root(dir)
    }
}

impl Root {
    /// 门铃：都在这个临时目录里（没人绑，敲了也没人听）。
    fn bell(&self) -> Bell {
        Bell::new(None, self.0.clone())
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        drop(std::fs::remove_dir_all(&self.0));
    }
}

fn session(text: &str) -> SessionId {
    SessionId::parse(text).expect("合写法")
}

const TO: &str = "0192f3a0-1111-7abc-8def-001122334455";
const FROM: &str = "0192f3a0-2222-7abc-8def-5566778899aa";

fn message(text: &str, call: &str) -> Letter {
    Letter::Message {
        from: session(FROM),
        call: CallId::parse(call).expect("合写法"),
        text: text.to_string(),
    }
}

/// 建好收的那个会话的目录，交回它。
fn target(root: &Root) -> PathBuf {
    let dir = root.0.join(TO);
    std::fs::create_dir(&dir).expect("建得了");
    dir
}

fn names(inbox: &Path) -> Vec<String> {
    scan(inbox)
        .expect("读得了")
        .into_iter()
        .map(|name| name.to_string_lossy().into_owned())
        .collect()
}

#[test]
fn letters_come_back_whole_and_in_the_order_they_were_posted() {
    let root = Root::new("letter-order");
    let dir = target(&root);
    for k in 0..20 {
        post(
            &root.0,
            &session(TO),
            &message(&format!("第 {k} 句"), &format!("call_1_{}", k + 1)),
            &root.bell(),
        )
        .expect("写得进");
    }
    let inbox = dir.join(INBOX);
    let got: Vec<Letter> = names(&inbox)
        .iter()
        .map(|name| read(&inbox.join(name)).expect("读得懂").expect("还在"))
        .collect();
    let want: Vec<Letter> = (0..20)
        .map(|k| message(&format!("第 {k} 句"), &format!("call_1_{}", k + 1)))
        .collect();
    assert_eq!(got, want, "照文件名排就是照发的先后");
}

/// 人从命令行说的一句（LM-59）：放进去、读出来是同一封；命令编号照 `<会话>/say/<那一句的编号>` 写，内核认它是合写法的。
#[test]
fn a_sentence_said_from_the_command_line_comes_back_whole() {
    let root = Root::new("letter-say");
    let dir = target(&root);
    let said = Letter::Say {
        from: session(TO),
        id: "0199a2b3-0000-7000-8000-00000000abcd".to_string(),
        text: "从命令行来的一句".to_string(),
    };
    post(&root.0, &session(TO), &said, &root.bell()).expect("写得进");
    let inbox = dir.join(INBOX);
    let names = names(&inbox);
    assert_eq!(names.len(), 1);
    assert_eq!(read(&inbox.join(&names[0])).expect("读得懂"), Some(said));
    let command = super::said_command(&session(TO), "0199a2b3-0000-7000-8000-00000000abcd");
    assert_eq!(
        command,
        format!("{TO}/say/0199a2b3-0000-7000-8000-00000000abcd")
    );
    assert!(lm_kernel::kernel::id::CommandId::parse(&command).is_ok());
}

#[test]
fn half_written_temp_files_are_not_read() {
    let root = Root::new("letter-temp");
    let dir = target(&root);
    let inbox = dir.join(INBOX);
    std::fs::create_dir(&inbox).expect("建得了");
    std::fs::write(inbox.join(".0192-half.json"), "{\"kind\":\"mess").expect("写得进");
    assert!(names(&inbox).is_empty(), "`.` 开头的是发的一方还没写完的");
    assert!(
        names(&dir.join("nothing")).is_empty(),
        "没有收件目录的是空的"
    );
}

#[test]
fn broken_odd_or_huge_letters_give_a_reason_and_are_set_aside() {
    let root = Root::new("letter-bad");
    let dir = target(&root);
    let inbox = dir.join(INBOX);
    std::fs::create_dir(&inbox).expect("建得了");
    std::fs::write(inbox.join("1-half.json"), "{\"kind\":\"message\",\"fr").expect("写得进");
    std::fs::write(
        inbox.join("2-odd.json"),
        "{\"kind\":\"poem\",\"from\":\"x\"}",
    )
    .expect("写得进");
    std::fs::write(inbox.join("3-big.json"), vec![b' '; (2 << 20) + 1]).expect("写得进");
    std::fs::create_dir(inbox.join("4-dir")).expect("建得了");
    std::fs::write(inbox.join("5-notes.txt"), "随手放的").expect("写得进");
    let found = scan(&inbox).expect("读得了");
    assert_eq!(found.len(), 5);
    for name in &found {
        let why = read(&inbox.join(name)).expect_err("读不懂");
        assert!(!why.is_empty(), "{name:?} 有原因");
        set_aside(&dir, name).expect("挪得走");
    }
    assert!(names(&inbox).is_empty(), "都挪走了");
    assert_eq!(names(&dir.join(BAD)).len(), 5, "挪进了旁边的 bad 目录");
    assert_eq!(read(&inbox.join("gone.json")), Ok(None), "已经没了的不算坏");
}

#[test]
fn a_missing_session_or_an_unwritable_inbox_is_an_error_and_leaves_nothing() {
    let root = Root::new("letter-fail");
    let gone = post(
        &root.0,
        &session(TO),
        &message("在吗", "call_1_1"),
        &root.bell(),
    )
    .expect_err("会话目录不在");
    assert_eq!(gone.kind(), std::io::ErrorKind::NotFound);
    assert!(!root.0.join(TO).exists(), "不替它建会话目录");
    let dir = target(&root);
    let inbox = dir.join(INBOX);
    std::fs::create_dir(&inbox).expect("建得了");
    std::fs::set_permissions(&inbox, std::fs::Permissions::from_mode(0o555)).expect("改得了");
    let refused = post(
        &root.0,
        &session(TO),
        &message("在吗", "call_1_2"),
        &root.bell(),
    );
    std::fs::set_permissions(&inbox, std::fs::Permissions::from_mode(0o755)).expect("改得回");
    // 跑测试的是 root 时只读挡不住：只查挡住了的那一种。
    if refused.is_err() {
        assert!(
            std::fs::read_dir(&inbox).expect("读得了").next().is_none(),
            "不留临时文件"
        );
    }
}

/// 「空了告诉我」的计时（L7 三，`watches.rs`）：拨 tokio 的钟，不真等。订了到点交 `expired`；撤掉了（不在等了）的不交；会话
/// 目录不在的当场交 `gone`；已经过了点的不放登记，当场交 `expired`。
mod expiry {
    use std::time::Duration;

    use tokio::sync::mpsc;

    use lm_kernel::kernel::event::IdleReason;
    use lm_kernel::kernel::time::Timestamp;

    use super::super::watches::Watches;
    use super::{FROM, Root, TO, session};
    use crate::session::port::Back;

    const HOUR_MS: i64 = 3_600_000;

    fn ms(millis: i64) -> Timestamp {
        Timestamp::from_unix_millis(millis).expect("在范围里")
    }

    /// 下一份交回来的：到点、不在了、没开着的会话和原因。
    async fn next(backs: &mut mpsc::UnboundedReceiver<Back>) -> (String, IdleReason) {
        match backs.recv().await.expect("通道没断") {
            Back::WatchEnded { session, reason } => (session.to_string(), reason),
            _ => panic!("只交等不到了的"),
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_watch_expires_after_its_hours_unless_dropped() {
        let root = Root::new("expiry");
        let (sender, mut backs) = mpsc::unbounded_channel();
        let mut watches = Watches::default();
        let since = ms(1_000 * HOUR_MS);
        let me = session(FROM);
        watches.sync(
            vec![(session(TO), since)],
            2,
            since,
            (&root.0, &me, &root.bell()),
            &sender,
        );
        // 会话目录不在：当场交 `gone`；计时照走。
        assert_eq!(next(&mut backs).await, (TO.to_string(), IdleReason::Gone));
        let started = tokio::time::Instant::now();
        assert_eq!(
            next(&mut backs).await,
            (TO.to_string(), IdleReason::Expired)
        );
        assert_eq!(
            started.elapsed(),
            Duration::from_millis(2 * 3_600_000),
            "正好两个小时"
        );
        // 又订了一次、再撤掉：计时跟着撤，到点了也不交。
        let again = ms(2_000 * HOUR_MS);
        watches.sync(
            vec![(session(TO), again)],
            2,
            again,
            (&root.0, &me, &root.bell()),
            &sender,
        );
        assert_eq!(next(&mut backs).await, (TO.to_string(), IdleReason::Gone));
        watches.sync(Vec::new(), 2, again, (&root.0, &me, &root.bell()), &sender);
        let quiet = tokio::time::timeout(Duration::from_secs(3 * 3_600), backs.recv()).await;
        assert!(quiet.is_err(), "撤掉了的不交：{quiet:?}");
        // 已经过了点的：不放登记，当场交作废。
        let late = ms(3_000 * HOUR_MS);
        watches.sync(
            vec![(session(TO), since)],
            2,
            late,
            (&root.0, &me, &root.bell()),
            &sender,
        );
        assert_eq!(
            next(&mut backs).await,
            (TO.to_string(), IdleReason::Expired)
        );
    }
}
