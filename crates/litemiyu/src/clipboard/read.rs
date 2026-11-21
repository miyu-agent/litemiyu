//! 读系统剪贴板里的字（`Ctrl+V`，蓝图 `tui.md`「按键」）：按平台依次试剪贴板命令，第一个读得到的算数，
//! 每个最多等一会儿（卡住的剪贴板命令不能把界面也卡住）。这里只读字；图片在 `image.rs`，复制的文件在 `files.rs`。
//!
//! 经 SSH 连过来时读的是那台机器的剪贴板；要粘手边的东西，用终端自己的粘贴（走括号粘贴，不经这里）。

use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// 一个剪贴板命令最多等多久。
pub(super) fn wait() -> Duration {
    Duration::from_secs(1)
}

/// 读不到字：各是各的原因，提示照它说（2026-11-21 项目主人：剪贴板里是截图，提示「无法读取剪贴板」，不知道是什么情况）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unreadable {
    /// 剪贴板命令读到了东西，但不是字（`wl-paste` 交出的是原始字节），也不是认得的图片。
    NotText,
    /// 剪贴板里的东西超过了一次读的上限（[`READ_LIMIT`]）。
    TooBig,
    /// 这台机器上一个剪贴板命令都没装。
    Missing,
    /// 剪贴板命令装了，但是失败了、卡住了。
    Failed,
}

/// 读系统剪贴板里的字。剪贴板是空的交回空字符串。
///
/// # Errors
///
/// 读不到字的照原因交回 [`Unreadable`]。
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
    // 不是 UTF-8 的（剪贴板里是原始字节）不硬转成一串替代符（2026-10-22 项目主人在确认的理由里 `Ctrl+V`，贴进来一大片
    // 乱码）：接着试下一个命令。都读不成字的照最说明问题的那个原因说：读到了不是字的、太大的、失败的、一个都没装的。
    let mut why = Unreadable::Missing;
    for (cmd, args) in list {
        let seen = match run(cmd, args, wait) {
            Ran::Out(out) => match String::from_utf8(out) {
                Ok(text) => return Ok(text),
                Err(_) => Unreadable::NotText,
            },
            Ran::Over => Unreadable::TooBig,
            Ran::Failed => Unreadable::Failed,
            Ran::Missing => Unreadable::Missing,
        };
        why = match (why, seen) {
            (Unreadable::NotText, _) | (_, Unreadable::NotText) => Unreadable::NotText,
            (Unreadable::TooBig, _) | (_, Unreadable::TooBig) => Unreadable::TooBig,
            (Unreadable::Failed, _) | (_, Unreadable::Failed) => Unreadable::Failed,
            _ => Unreadable::Missing,
        };
    }
    Err(why)
}

/// 读进来的输出最多这么多字节（16 MiB），多出来的不读：剪贴板里塞了几百兆的东西，不能全读进内存。
pub(super) const READ_LIMIT: u64 = 16 << 20;

/// 跑一个剪贴板命令的结局。
pub(super) enum Ran {
    /// 成功退出，这是它的输出。
    Out(Vec<u8>),
    /// 输出超过了 [`READ_LIMIT`]。
    Over,
    /// 没装（起不来）。
    Missing,
    /// 失败了，或者到点还没完。
    Failed,
}

/// 跑一个命令，读它的输出；没装、失败、到点还没完、超过 [`READ_LIMIT`] 的交回 `None`（[`run`]）。
pub(super) fn run_bytes(cmd: &str, args: &[&str], wait: Duration) -> Option<Vec<u8>> {
    match run(cmd, args, wait) {
        Ran::Out(out) => Some(out),
        Ran::Over | Ran::Missing | Ran::Failed => None,
    }
}

/// 跑一个命令，读它的输出，照结局交回（到点的杀掉）。输出边跑边在另一个
/// 线程里读：剪贴板里东西多时（超过管道的缓冲），命令要等人读走才写得完、才会退出。读的线程也只等到点：命令起的后台进程
/// 继承了标准输出、还拿着管道时，读的线程读不到头，到点就不管它（它读到头自己退出），不把界面卡住（审计 #055）。
pub(super) fn run(cmd: &str, args: &[&str], wait: Duration) -> Ran {
    let spawned = Command::new(cmd)
        .env_remove(crate::rescue::API_KEY)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();
    let mut child = match spawned {
        Ok(child) => child,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ran::Missing,
        Err(_) => return Ran::Failed,
    };
    let Some(mut stdout) = child.stdout.take() else {
        return Ran::Failed;
    };
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
    if !status.is_some_and(|status| status.success()) {
        return Ran::Failed;
    }
    // 命令退出到读的线程读到头之间还差一点：快到点才退出的，也给它留这一点。读不到头的（后台进程还拿着管道）到点不等，
    // 读的线程不管它（读到头自己退出）。
    let until = (start + wait).max(Instant::now() + TAIL);
    while !reader.is_finished() {
        if Instant::now() >= until {
            return Ran::Failed;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    let Ok(Ok(out)) = reader.join() else {
        return Ran::Failed;
    };
    match u64::try_from(out.len()).is_ok_and(|len| len <= READ_LIMIT) {
        true => Ran::Out(out),
        false => Ran::Over,
    }
}

/// 命令退出以后，读的线程最少再等这么久读完管道里剩下的。
const TAIL: Duration = Duration::from_millis(100);

#[cfg(test)]
mod tests;
