//! 和 herdr 一起用（`src/herdr.rs`）：在（假的）herdr 窗格里起程序，看它调 herdr 的命令报了什么。假的 herdr 是一个
//! `/bin/sh` 脚本，把收到的参数一行行记进文件；不碰真的 herdr。

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use crate::replies::{calls, says};
use crate::support::{Options, Session, served};

/// 假的 herdr：一个目录里放着脚本 `herdr`、它记的 `log`，还有一个 `bin/litemiyu`（让恢复命令的命令名在 PATH 上找得到）。
struct Fake {
    dir: PathBuf,
}

impl Fake {
    fn new() -> Fake {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "lm-herdr-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(dir.join("bin")).expect("建得了目录");
        std::fs::write(dir.join("bin/litemiyu"), b"").expect("写得进");
        let log = dir.join("log");
        std::fs::write(&log, b"").expect("写得进");
        let script = dir.join("herdr");
        std::fs::write(
            &script,
            // 往标准输出、标准错误也各写一句：程序没把它们接到空设备的话，这两句会落在全屏界面上。
            format!(
                "#!/bin/sh\necho \"$*\" >> '{}'\necho herdr-noise-out\necho herdr-noise-err >&2\n",
                log.display()
            ),
        )
        .expect("写得进");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))
                .expect("改得了权限");
        }
        Fake { dir }
    }

    /// 起程序时带的环境变量：`binary` 是 `HERDR_BIN_PATH`。
    fn options(&self, binary: &std::path::Path) -> Options {
        let path = format!("{}:/usr/bin:/bin", self.dir.join("bin").display());
        Options {
            more_env: vec![
                ("HERDR_ENV".into(), "1".into()),
                ("HERDR_PANE_ID".into(), "p7".into()),
                ("HERDR_BIN_PATH".into(), binary.as_os_str().to_os_string()),
                ("PATH".into(), OsString::from(path)),
            ],
            ..Options::default()
        }
    }

    /// 假 herdr 到这一刻收到的，一次一行。
    fn calls(&self) -> Vec<String> {
        std::fs::read_to_string(self.dir.join("log"))
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }

    /// 等到收到的那些让 `ready` 满意，交回它们。
    fn wait(&self, what: &str, ready: impl Fn(&[String]) -> bool) -> Vec<String> {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let calls = self.calls();
            if ready(&calls) {
                return calls;
            }
            assert!(
                Instant::now() < deadline,
                "没等到{what}，假 herdr 收到的：\n{}",
                calls.join("\n")
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

impl Drop for Fake {
    fn drop(&mut self) {
        if std::fs::remove_dir_all(&self.dir).is_err() {}
    }
}

/// 一行里 `flag` 后面那个词。
fn after<'a>(call: &'a str, flag: &str) -> Option<&'a str> {
    let mut words = call.split(' ');
    words.find(|word| *word == flag)?;
    words.next()
}

/// 最后一行是不是报的 `state`、登记的会话是不是 `session`。
fn last_is(calls: &[String], state: &str, session: Option<&str>) -> bool {
    calls.last().is_some_and(|call| {
        after(call, "--state") == Some(state) && after(call, "--agent-session-id") == session
    })
}

/// `pid` 的线程里名字叫 `herdr` 的有几条（只在 Linux 上看得了）。
#[cfg(target_os = "linux")]
fn herdr_threads(pid: u32) -> usize {
    std::fs::read_dir(format!("/proc/{pid}/task"))
        .expect("进程在")
        .filter_map(Result::ok)
        .filter(|task| {
            std::fs::read_to_string(task.path().join("comm")).is_ok_and(|n| n.trim() == "herdr")
        })
        .count()
}

/// `pid` 的子进程里成了僵尸的有几个（只在 Linux 上看得了）。
#[cfg(target_os = "linux")]
fn zombies_of(pid: u32) -> usize {
    std::fs::read_dir("/proc")
        .expect("读得了")
        .filter_map(Result::ok)
        .filter(|entry| {
            let Ok(stat) = std::fs::read_to_string(entry.path().join("stat")) else {
                return false;
            };
            // `号 (名字) 状态 父进程号 …`：名字里可以有空格、括号，从最后一个 `)` 往后读。
            let Some((_, rest)) = stat.rsplit_once(") ") else {
                return false;
            };
            let mut fields = rest.split(' ');
            fields.next() == Some("Z") && fields.next() == Some(pid.to_string().as_str())
        })
        .count()
}

#[test]
fn states_and_the_resume_command_are_reported_to_herdr() {
    let fake = Fake::new();
    let run = r#"{"command":"git push --force","description":"Push"}"#;
    let nap = r#"{"command":"sleep 0.5","description":"Nap"}"#;
    let server = served(vec![
        calls("shell", run),
        says(&["推不上去。"]),
        calls("shell", nap),
        says(&["睡醒了。"]),
    ]);
    let options = fake.options(&fake.dir.join("herdr"));
    let config = format!(
        "[providers.fake]\ndriver = \"openai-chat\"\nbase_url = \"{}\"\nkey = {{ env = \"LITEMIYU_TEST_KEY\" }}\n\n\
         [providers.fake.models.\"m1\"]\n\n[models]\nchat = \"fake/m1\"\n",
        server.base_url
    );
    let session = Session::launch_with(Some(&config), &[], &[], &options);
    session.wait_for("❯", "输入框");
    // 一起来就报空闲：还没有会话，不带会话编号。
    let calls_now = fake.wait("一起来报的空闲", |calls| !calls.is_empty());
    let first: Vec<&str> = calls_now[0].split(' ').collect();
    assert_eq!(
        first[..10],
        [
            "pane",
            "report-agent",
            "p7",
            "--source",
            "custom:litemiyu",
            "--agent",
            "litemiyu",
            "--state",
            "idle",
            "--seq"
        ]
    );
    assert_eq!(first.len(), 11, "{first:?}");
    #[cfg(target_os = "linux")]
    assert_eq!(herdr_threads(session.pid().expect("有进程号")), 1);

    // 说一句话，引出一次确认：等你回复，带着会话编号和恢复命令。
    session.type_text("推一下");
    session.press_enter();
    session.wait_for("后自动拒绝", "确认的抽屉");
    let calls_now = fake.wait("等你回复", |calls| {
        calls
            .last()
            .is_some_and(|c| after(c, "--state") == Some("blocked"))
    });
    let blocked = calls_now.last().expect("有");
    let one = after(blocked, "--agent-session-id")
        .expect("登记了会话")
        .to_string();
    assert!(
        blocked.ends_with(&format!("--agent-session-id {one} -- litemiyu -s {one}")),
        "{blocked}"
    );
    // 允许：说完回到空闲。
    session.type_text("1");
    session.wait_for("推不上去。", "允许以后接着说");
    fake.wait("说完的空闲", |calls| {
        last_is(calls, "idle", Some(&one))
    });

    // `/new`：先退还（清掉旧的恢复命令），再照空闲取回，不带会话。
    session.type_text("/new");
    session.press_enter();
    let calls_now = fake.wait("`/new` 以后的空闲", |calls| {
        last_is(calls, "idle", None)
    });
    let before = &calls_now[calls_now.len() - 2];
    assert!(before.starts_with("pane release-agent p7 "), "{before}");

    // 新会话里说一句（这一轮里睡半秒）：工作中、空闲，带新会话的恢复命令。
    session.type_text("睡一下");
    session.press_enter();
    session.wait_for("睡醒了。", "第二个会话答完");
    let calls_now = fake.wait("第二个会话的空闲", |calls| {
        calls.last().is_some_and(|c| {
            after(c, "--state") == Some("idle") && after(c, "--agent-session-id").is_some()
        })
    });
    let two = after(calls_now.last().expect("有"), "--agent-session-id")
        .expect("登记了会话")
        .to_string();
    assert_ne!(one, two);
    assert!(
        calls_now.iter().any(|c| {
            after(c, "--state") == Some("working")
                && c.ends_with(&format!("--agent-session-id {two} -- litemiyu -s {two}"))
        }),
        "第二个会话报过工作中：\n{}",
        calls_now.join("\n")
    );
    // 假 herdr 往标准输出、标准错误写的字没有落到终端上。
    assert!(!session.screen().contains("herdr-noise"));
    assert!(!crate::support::contains(&session.raw(), b"herdr-noise"));
    #[cfg(target_os = "linux")]
    assert_eq!(zombies_of(session.pid().expect("有进程号")), 0);

    // 人退出：退还窗格。
    session.type_text("/exit");
    session.press_enter();
    assert!(session.wait_exit(), "退出码 0");
    let calls_now = fake.calls();
    let last = calls_now.last().expect("有");
    assert!(last.starts_with("pane release-agent p7 "), "{last}");
    // 序号一路往上，从毫秒数起。
    let seq: Vec<u64> = calls_now
        .iter()
        .map(|c| after(c, "--seq").expect("带序号").parse().expect("是数"))
        .collect();
    assert!(seq[0] > 1_700_000_000_000, "{seq:?}");
    assert!(seq.windows(2).all(|s| s[0] < s[1]), "{seq:?}");
    // 状态没变不重复报：相邻两条上报不会状态、会话都一样。
    for pair in calls_now.windows(2) {
        let same = pair[0].contains("report-agent")
            && pair[1].contains("report-agent")
            && after(&pair[0], "--state") == after(&pair[1], "--state")
            && after(&pair[0], "--agent-session-id") == after(&pair[1], "--agent-session-id");
        assert!(!same, "重复报了：\n{}\n{}", pair[0], pair[1]);
    }

    // `-s <编号>` 重开第二个会话：补完了报空闲，带它的恢复命令，中间不退还。
    let count = calls_now.len();
    let again = session.another_with(&["-s", &two], &options);
    again.wait_for("睡醒了。", "补发了那一句");
    let calls_now = fake.wait("重开以后的空闲", |calls| {
        last_is(calls, "idle", Some(&two))
    });
    assert!(
        calls_now[count..]
            .iter()
            .all(|c| c.contains("report-agent")),
        "重开没有退还：\n{}",
        calls_now[count..].join("\n")
    );
    let last = calls_now.last().expect("有");
    assert!(last.ends_with(&format!("-- litemiyu -s {two}")), "{last}");
    again.press(b"\x04");
    assert!(again.wait_exit(), "Ctrl+D 好好退出");
}

/// 不在 herdr 里：没有上报的线程。herdr 的程序不在了（报不上去）：屏幕上不出现报错的字，照常说话、照常退出。
#[test]
fn outside_herdr_no_thread_and_a_missing_herdr_writes_nothing_on_screen() {
    let server = served(vec![says(&["在。"])]);
    let plain = Session::start(&server.base_url, &[]);
    plain.wait_for("❯", "输入框");
    #[cfg(target_os = "linux")]
    assert_eq!(herdr_threads(plain.pid().expect("有进程号")), 0);
    drop(plain);

    let fake = Fake::new();
    let missing = fake.dir.join("gone");
    let options = fake.options(&missing);
    let session = Session::start_with(&server.base_url, &[], &options);
    session.wait_for("❯", "输入框");
    session.type_text("在吗");
    session.press_enter();
    session.wait_for("在。", "她回了");
    let screen = session.screen();
    assert!(
        !screen.contains("No such file") && !screen.contains("gone"),
        "{screen}"
    );
    assert!(fake.calls().is_empty());
    session.type_text("/exit");
    session.press_enter();
    assert!(session.wait_exit(), "退出码 0");
}
