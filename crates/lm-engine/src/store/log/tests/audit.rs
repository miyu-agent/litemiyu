//! 审计 2026-11-18 #053 的红测试：写到一半写不进（磁盘满这一类）以后，同一份日志接着写，半行不能留在中间。
//!
//! 磁盘满用文件大小上限（`RLIMIT_FSIZE`）演：超过上限的那次 `write` 先写到上限、交回写了几个字节，下一次交回 `EFBIG`，
//! 和写到一半盘满了一样是「前一截进了文件、后一截没进」。上限是整个进程的，所以另起一个只跑探针的子进程（测试程序自己，
//! `--exact` 只挑探针），在子进程里设；`SIGXFSZ` 经 `sh` 的 `trap ''` 忽略（忽略的信号跨 `exec` 留着），不然超限当场把它杀掉。
//! 不往真盘上写满任何东西。

use std::path::PathBuf;
use std::process::Command;

use super::*;

/// 探针从这个环境变量拿会话目录；没有的（单跑 `--ignored` 时）什么都不做。
const DIR: &str = "LM_AUDIT_053_DIR";

/// 子进程里跑的那一半：写一条、把上限设在再多 100 字节、写一条长的（写到一半被截住）、放开上限、照同一个序号再写一条短的。
#[test]
#[ignore = "审计 #053 的探针：只由 a_write_cut_short_leaves_no_half_line_in_the_middle 带着环境变量起"]
fn partial_write_probe() {
    use rustix::process::{Resource, Rlimit, getrlimit, setrlimit};
    let Some(dir) = std::env::var_os(DIR).map(PathBuf::from) else {
        return;
    };
    let mut log = SessionLog::create(&dir, SEGMENT_LIMIT).unwrap();
    log.append(&[said(1)]).unwrap();
    let size = fs::metadata(dir.join("000000000001.jsonl")).unwrap().len();
    let hard = getrlimit(Resource::Fsize).maximum;
    setrlimit(
        Resource::Fsize,
        Rlimit {
            current: Some(size + 100),
            maximum: hard,
        },
    )
    .unwrap();
    let long = Event::from_line(&format!(
        r#"{{"seq":2,"at":"2026-09-25T07:00:00.000Z","kind":"message.user","by":{{"kind":"person"}},"body":{{"blocks":[{{"type":"text","text":"{}"}}]}}}}"#,
        "长".repeat(1000)
    ))
    .unwrap();
    let cut = log.write(&[long]);
    assert!(cut.is_err(), "超过上限的那一批写不进");
    setrlimit(
        Resource::Fsize,
        Rlimit {
            current: hard,
            maximum: hard,
        },
    )
    .unwrap();
    // 空间回来了：上一批没写成，序号还停在 2，照 2 再写一条。
    log.append(&[said(2)]).unwrap();
}

#[test]
fn a_write_cut_short_leaves_no_half_line_in_the_middle() {
    let scratch = Scratch::new();
    let dir = dir(&scratch);
    let exe = std::env::current_exe().unwrap();
    let status = Command::new("sh")
        .args(["-c", "trap '' XFSZ; exec \"$0\" \"$@\""])
        .arg(&exe)
        .args([
            "--exact",
            "store::log::tests::audit::partial_write_probe",
            "--ignored",
            "--test-threads=1",
            "--quiet",
        ])
        .env(DIR, &dir)
        .status()
        .unwrap();
    assert!(status.success(), "探针跑完了：{status:?}");
    let opened = SessionLog::open(&dir, SEGMENT_LIMIT);
    let events = match opened {
        Ok((_, events)) => events,
        Err(error) => panic!("写不进的那半行留在了中间，日志打不开了：{error}"),
    };
    assert_eq!(events, said_range(1, 2), "写成的两条都在，没写成的那条不在");
}
