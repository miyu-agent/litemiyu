//! 读系统剪贴板里的字（`Ctrl+V`，蓝图 `tui.md`「按键」）：按平台依次试剪贴板命令，第一个读得到的算数，
//! 每个最多等一会儿（卡住的剪贴板命令不能把界面也卡住）。只读字，图片以后再说。
//!
//! 经 SSH 连过来时读的是那台机器的剪贴板；要粘手边的东西，用终端自己的粘贴（走括号粘贴，不经这里）。

use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// 一个剪贴板命令最多等多久。
pub(super) fn wait() -> Duration {
    Duration::from_secs(1)
}

/// 读不到：这台机器上没有能用的剪贴板命令，或者都失败了、卡住了。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Unreadable;

/// 读系统剪贴板里的字。剪贴板是空的交回空字符串。
///
/// # Errors
///
/// 每个剪贴板命令都读不到（没装、失败、卡住）时交回 [`Unreadable`]。
pub fn read() -> Result<String, Unreadable> {
    read_with(&candidates(this_os(), wayland()), wait())
}

fn this_os() -> Os {
    if cfg!(target_os = "macos") {
        Os::Mac
    } else {
        Os::Unix
    }
}

fn wayland() -> bool {
    std::env::var_os("WAYLAND_DISPLAY").is_some()
}

/// 哪种系统。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Os {
    /// Linux、BSD：X11、Wayland 的剪贴板命令。
    Unix,
    /// macOS：`pbpaste`。
    Mac,
}

/// 这种系统上依次试哪些命令（命令、参数）。Linux、BSD 有 Wayland 的先试 `wl-paste`。
pub(super) fn candidates(os: Os, wayland: bool) -> Vec<(&'static str, &'static [&'static str])> {
    match os {
        Os::Mac => vec![("pbpaste", &[])],
        Os::Unix => {
            let mut list: Vec<(&str, &[&str])> = Vec::new();
            if wayland {
                list.push(("wl-paste", &["--no-newline"]));
            }
            list.push(("xclip", &["-selection", "clipboard", "-o"]));
            list.push(("xsel", &["--clipboard", "--output"]));
            list
        }
    }
}

/// 依次试 `list` 里的命令，第一个成功退出的算数；每个最多等 `wait`。
pub(super) fn read_with(list: &[(&str, &[&str])], wait: Duration) -> Result<String, Unreadable> {
    list.iter()
        .find_map(|(cmd, args)| run(cmd, args, wait))
        .ok_or(Unreadable)
}

/// 跑一个命令，读它的输出当字。不是 UTF-8 的（剪贴板里是图片：`wl-paste` 交出 PNG 的原始字节）当没读到，不硬转成一串
/// 替代符（2026-10-22 项目主人在确认的理由里 `Ctrl+V`，贴进来一大片乱码）。
fn run(cmd: &str, args: &[&str], wait: Duration) -> Option<String> {
    run_bytes(cmd, args, wait).and_then(|out| String::from_utf8(out).ok())
}

/// 读进来的输出最多这么多字节（16 MiB），多出来的当读不到：剪贴板里塞了几百兆的东西，不能全读进内存。
pub(super) const READ_LIMIT: u64 = 16 << 20;

/// 跑一个命令，读它的输出；没装、失败、到点还没完、超过 [`READ_LIMIT`] 的交回 `None`（到点的杀掉）。输出边跑边在另一个
/// 线程里读：剪贴板里东西多时（超过管道的缓冲），命令要等人读走才写得完、才会退出。读的线程也只等到点：命令起的后台进程
/// 继承了标准输出、还拿着管道时，读的线程读不到头，到点就不管它（它读到头自己退出），不把界面卡住（审计 #055）。
pub(super) fn run_bytes(cmd: &str, args: &[&str], wait: Duration) -> Option<Vec<u8>> {
    let mut child = Command::new(cmd)
        .env_remove(crate::rescue::API_KEY)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        // 读到头，或者读过了上限就停（多出来的当读不到）。
        let mut out = Vec::new();
        let mut chunk = [0; 16 * 1024];
        loop {
            let n = match stdout.read(&mut chunk) {
                Ok(n) => n,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => return Err(error),
            };
            out.extend_from_slice(&chunk[..n]);
            if n == 0 || !u64::try_from(out.len()).is_ok_and(|len| len <= READ_LIMIT) {
                return Ok(out);
            }
        }
    });
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if start.elapsed() < wait => std::thread::sleep(Duration::from_millis(5)),
            _ => {
                // 杀不掉、等不到也没别的办法：到点就当读不到。
                child.kill().unwrap_or_default();
                child.wait().map(drop).unwrap_or_default();
                break None;
            }
        }
    };
    status.filter(std::process::ExitStatus::success)?;
    // 命令退出到读的线程读到头之间还差一点：快到点才退出的，也给它留这一点。读不到头的（后台进程还拿着管道）到点不等，
    // 读的线程不管它（读到头自己退出）。
    let until = (start + wait).max(Instant::now() + TAIL);
    while !reader.is_finished() {
        if Instant::now() >= until {
            return None;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let out = reader.join().ok()?.ok()?;
    u64::try_from(out.len())
        .is_ok_and(|len| len <= READ_LIMIT)
        .then_some(out)
}

/// 命令退出以后，读的线程最少再等这么久读完管道里剩下的。
const TAIL: Duration = Duration::from_millis(100);

#[cfg(test)]
mod tests;
