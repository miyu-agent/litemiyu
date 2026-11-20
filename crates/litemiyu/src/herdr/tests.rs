use std::collections::HashMap;
use std::sync::mpsc;

use super::{Herdr, Resume, State, decimal, first_seq, resume_program};

fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> + use<> {
    let map: HashMap<String, String> = pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    move |name| map.get(name).cloned()
}

/// 不起线程的一份：放进通道的参数由测试收。
fn queued(resume: Option<&str>) -> (Herdr, mpsc::Receiver<Vec<String>>) {
    let (tx, rx) = mpsc::channel();
    let herdr = Herdr {
        reports: Some(tx),
        ended: None,
        session: None,
        resume: Resume::Known(resume.map(str::to_string)),
        pane: "p1".into(),
        next: 100,
        last: None,
    };
    (herdr, rx)
}

/// 这个测试进程有没有名字叫 `herdr` 的线程（只在 Linux 上看得了）。
#[cfg(target_os = "linux")]
fn herdr_threads() -> usize {
    std::fs::read_dir("/proc/self/task")
        .unwrap()
        .filter_map(Result::ok)
        .filter(|task| {
            std::fs::read_to_string(task.path().join("comm")).is_ok_and(|n| n.trim() == "herdr")
        })
        .count()
}

/// 不在 herdr 里：三个环境变量缺一个都不算，交回 `None`，一条线程都不起。
#[test]
fn outside_herdr_nothing_is_created() {
    let cases: [&[(&str, &str)]; 5] = [
        &[],
        &[("HERDR_ENV", "1"), ("HERDR_PANE_ID", "p1")],
        &[("HERDR_ENV", "1"), ("HERDR_BIN_PATH", "/bin/herdr")],
        &[("HERDR_PANE_ID", "p1"), ("HERDR_BIN_PATH", "/bin/herdr")],
        &[
            ("HERDR_ENV", "0"),
            ("HERDR_PANE_ID", "p1"),
            ("HERDR_BIN_PATH", "/bin/herdr"),
        ],
    ];
    for case in cases {
        assert!(
            Herdr::detect(env(case), || Some("litemiyu".into())).is_none(),
            "{case:?}"
        );
    }
    // 这个测试程序里别的测试不起真的上报线程（都用 `queued`），所以这里数得准。
    #[cfg(target_os = "linux")]
    assert_eq!(herdr_threads(), 0);
}

#[test]
fn reports_carry_a_rising_sequence_from_the_clock() {
    let (mut h, _rx) = queued(None);
    h.next = first_seq();
    let first = h.report_args(State::Working);
    assert_eq!(
        first[..9],
        [
            "pane",
            "report-agent",
            "p1",
            "--source",
            "custom:litemiyu",
            "--agent",
            "litemiyu",
            "--state",
            "working"
        ]
    );
    assert_eq!(first[9], "--seq");
    let seq: u64 = first[10].parse().unwrap();
    assert!(seq > 1_700_000_000_000, "从毫秒数起：{seq}");
    let release = h.release_args();
    assert_eq!(release[1], "release-agent");
    assert_eq!(release[7], "--seq");
    assert_eq!(
        release[8].parse::<u64>().unwrap(),
        seq + 1,
        "退还也带序号，往上加"
    );
}

/// 状态没变不重复报；换会话带上恢复命令；`/new`（主会话没了）先退还再取回；退出时退还。照先后排着，序号一路往上。
#[test]
fn unchanged_states_are_skipped_and_switches_clear_and_exit_are_queued_in_order() {
    let (mut h, rx) = queued(Some("litemiyu"));
    h.sync(State::Idle, None);
    h.sync(State::Idle, None);
    h.sync(State::Working, Some("first"));
    h.sync(State::Working, Some("first"));
    h.sync(State::Blocked, Some("first"));
    h.sync(State::Idle, Some("second"));
    h.sync(State::Idle, None);
    h.release();
    drop(h);
    let reports: Vec<Vec<String>> = rx.into_iter().collect();
    let shape: Vec<(&str, Option<&str>)> = reports
        .iter()
        .map(|r| {
            let state = r.iter().position(|s| s == "--state").map(|at| &*r[at + 1]);
            (&*r[1], state)
        })
        .collect();
    assert_eq!(
        shape,
        [
            ("report-agent", Some("idle")),
            ("report-agent", Some("working")),
            ("report-agent", Some("blocked")),
            ("report-agent", Some("idle")),
            ("release-agent", None),
            ("report-agent", Some("idle")),
            ("release-agent", None),
        ]
    );
    fn tail(id: &str) -> [&str; 6] {
        ["--agent-session-id", id, "--", "litemiyu", "-s", id]
    }
    assert_eq!(reports[0].len(), 11, "还没有会话：不带会话编号");
    assert_eq!(&reports[1][11..], tail("first"));
    assert_eq!(&reports[2][11..], tail("first"));
    assert_eq!(&reports[3][11..], tail("second"));
    assert_eq!(reports[5].len(), 11, "`/new` 以后不带旧的恢复命令");
    let seq: Vec<u64> = reports
        .iter()
        .map(|r| {
            r[r.iter().position(|s| s == "--seq").unwrap() + 1]
                .parse()
                .unwrap()
        })
        .collect();
    assert_eq!(seq, [100, 101, 102, 103, 104, 105, 106]);
}

/// PATH 上找不到命令名的：登记会话编号，不报恢复命令。
#[test]
fn without_a_command_on_path_only_the_session_is_registered() {
    let (mut h, rx) = queued(None);
    h.sync(State::Idle, Some("first"));
    drop(h);
    let report = rx.recv().unwrap();
    assert_eq!(&report[11..], ["--agent-session-id", "first"]);
}

/// 恢复命令照人敲的那个命令的文件名写，PATH 上找不到的不报。
#[test]
fn the_resume_command_is_the_name_the_person_typed_and_must_be_on_path() {
    let dir = std::env::temp_dir().join(format!("lm-herdr-path-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("litemiyu"), b"").unwrap();
    std::fs::write(dir.join("lm"), b"").unwrap();
    let path = dir.to_string_lossy().into_owned();
    assert_eq!(
        resume_program(Some("litemiyu"), &path).as_deref(),
        Some("litemiyu")
    );
    assert_eq!(
        resume_program(Some("/opt/x/bin/lm"), &path).as_deref(),
        Some("lm"),
        "写了路径的只取文件名"
    );
    assert_eq!(
        resume_program(Some("./target/debug/litemiyu-dev"), &path),
        None,
        "PATH 上没有的不报"
    );
    assert_eq!(resume_program(None, &path), None);
    assert_eq!(resume_program(Some("litemiyu"), ""), None);
    // 头一次要报会话时才算，算一次。
    let (mut h, rx) = queued(None);
    h.resume = Resume::Later {
        program: Some("/opt/x/bin/lm".into()),
        path: path.clone(),
    };
    h.sync(State::Idle, None);
    assert!(matches!(h.resume, Resume::Later { .. }), "还没有会话：不算");
    h.sync(State::Idle, Some("first"));
    assert!(matches!(h.resume, Resume::Known(Some(_))));
    drop(h);
    let reports: Vec<Vec<String>> = rx.into_iter().collect();
    assert_eq!(
        &reports[1][11..],
        ["--agent-session-id", "first", "--", "lm", "-s", "first"]
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn decimal_writes_what_the_standard_formatter_writes() {
    for n in [0, 7, 10, 99, 100, 1_795_178_420_138, u64::MAX - 1, u64::MAX] {
        assert_eq!(decimal(n), n.to_string());
    }
}
