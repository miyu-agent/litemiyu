//! 崩溃记录（Miyu 终端 `crash.rs`，蓝图 `tui.md`「崩了」）：把这一次的时刻、版本、报错和位置追加进状态目录的 `crash.log`
//! （Miyu 记在机器缓存目录的 `tui/crash.log`；LiteMiyu 的状态目录放的就是这类给人查的记录，`docs/00-设计.md` 第六节），
//! 下一次复现不出来的崩溃也留得下位置。发行版崩了直接退出（`panic = "abort"`），崩的钩子照样先跑：终端还原以后记这一笔
//! （`terminal.rs`）。
//!
//! 不记调用栈（审计 #050，2026-11-18）：发行版链接时丢了展开表（`bench/no-unwind.ld`），栈恒是一行 `0: <unknown>`；崩在哪个
//! 文件哪一行哪一列是 panic 的位置，照样有。

use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::panic::PanicHookInfo;
use std::path::{Path, PathBuf};

use lm_engine::store::paths::{Env, Paths};

/// 文件名，在状态那一处。
const FILE: &str = "crash.log";

/// 记下这一次崩溃，交回记在哪；状态目录找不到、写不进去的交回 `None`（已经在崩了，不再多事）。
pub fn record(info: &PanicHookInfo<'_>) -> Option<PathBuf> {
    // 应急模式什么都不写盘（`rescue.rs`）：报错照旧打在终端里。
    if crate::rescue::is_on() {
        return None;
    }
    let path = Paths::locate(&Env::current()).ok()?.state.join(FILE);
    let time = crate::zone::now().strftime("%Y-%m-%d %H:%M:%S").to_string();
    let text = entry(&time, &info.to_string());
    append(&path, &text).ok()?;
    Some(path)
}

/// 把一段追加进 `path`，目录没有的先建。
fn append(path: &Path, text: &str) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(text.as_bytes())
}

/// 一次崩溃写成的一段：时刻和版本一行，报错连位置，末尾空一行隔开下一次。
fn entry(time: &str, message: &str) -> String {
    format!(
        "── {time} {} {}\n{message}\n\n",
        env!("CARGO_PKG_NAME"),
        env!("CARGO_PKG_VERSION")
    )
}

#[cfg(test)]
mod tests {
    use super::{append, entry};

    #[test]
    fn an_entry_keeps_the_time_and_the_message() {
        let text = entry(
            "2026-09-30 03:10:00",
            "panicked at src/ui/menu.rs:40:9:\nrange end index 1 out of range for slice of length 0",
        );
        assert!(text.starts_with("── 2026-09-30 03:10:00 litemiyu "));
        assert!(text.contains("range end index 1") && text.contains("src/ui/menu.rs:40:9"));
        assert!(text.ends_with("\n\n"), "和下一次之间空一行");
    }

    #[test]
    fn crashes_are_appended_under_a_state_dir_made_on_the_spot() {
        let dir = std::env::temp_dir().join(format!("litemiyu-crash-{}", std::process::id()));
        let path = dir.join("state").join(super::FILE);
        append(&path, "一\n\n").expect("目录当场建、写得进");
        append(&path, "二\n\n").expect("接着写");
        assert_eq!(
            std::fs::read_to_string(&path).expect("读得到"),
            "一\n\n二\n\n"
        );
        std::fs::remove_dir_all(&dir).expect("删得掉");
    }

    /// 审计 #050（2026-11-18）：崩了的时候终端还原的那几步照发（退出备用屏、关鼠标和粘贴、光标显示），崩溃记录里有版本、
    /// 位置（文件:行:列）和消息，不再有调用栈（发行版链接时丢了展开表，栈恒是一行 `0: <unknown>`）。装钩子是全进程的事，
    /// 所以另起一个子进程（本测试程序自己，只跑这一条）去崩，这里看它的输出和记录。
    #[test]
    fn a_crash_restores_the_terminal_and_records_where_without_a_backtrace() {
        const PROBE: &str = "LITEMIYU_TEST_CRASH_PROBE";
        const NAME: &str =
            "crash::tests::a_crash_restores_the_terminal_and_records_where_without_a_backtrace";
        if std::env::var_os(PROBE).is_some() {
            let report = std::panic::take_hook();
            std::panic::set_hook(Box::new(move |info| {
                crate::terminal::on_panic(info, report.as_ref());
            }));
            let crashed = std::thread::spawn(|| panic!("LM-1 验崩溃的口子")).join();
            assert!(crashed.is_err());
            return;
        }
        let home = std::env::temp_dir().join(format!("litemiyu-crash-hook-{}", std::process::id()));
        let child = std::process::Command::new(std::env::current_exe().expect("找得到自己"))
            .args(["--exact", NAME, "--nocapture", "--test-threads=1"])
            .env(PROBE, "1")
            .env("LITEMIYU_HOME", &home)
            .env_remove("RUST_BACKTRACE")
            .output()
            .expect("起得了子进程");
        let out = String::from_utf8_lossy(&child.stdout).into_owned();
        let err = String::from_utf8_lossy(&child.stderr).into_owned();
        assert!(child.status.success(), "{:?}\n{out}\n{err}", child.status);
        for sequence in ["\x1b[?1049l", "\x1b[?2004l", "\x1b[?25h"] {
            assert!(out.contains(sequence), "终端还原照发 {sequence:?}：{out:?}");
        }
        assert!(
            err.contains("LM-1 验崩溃的口子"),
            "终端里照旧打出报错：{err}"
        );
        assert!(err.contains("litemiyu: crash recorded in"), "{err}");
        let log = std::fs::read_to_string(home.join("state").join(super::FILE)).expect("记下了");
        std::fs::remove_dir_all(&home).expect("删得掉");
        let mut lines = log.lines();
        assert!(
            lines.next().is_some_and(|l| l.starts_with("── ")
                && l.ends_with(concat!(" litemiyu ", env!("CARGO_PKG_VERSION")))),
            "{log}"
        );
        assert!(
            lines.next().is_some_and(|l| l.starts_with("panicked at ")
                && l.contains("src/crash.rs:")
                && l.ends_with(':')),
            "位置是 文件:行:列：{log}"
        );
        assert_eq!(lines.next(), Some("LM-1 验崩溃的口子"), "{log}");
        assert_eq!(lines.next(), Some(""), "末尾空一行：{log}");
        assert_eq!(lines.next(), None, "没有调用栈：{log}");
    }
}
