//! 每个终端各一个会话（坑 P11、P12）：这个终端说过话的会话记在状态目录的 `shellhook/<终端的记号>` 里，一行，会话编号。
//! 只打开终端不记，说了话才记。
//!
//! 终端的记号是脚本起来时定的（`<shell 的进程号>-<随机数>`，`resources/shellhook/hook.*`）：只照进程号认的话，进程号被以后的
//! shell 重用时会接上别人的会话。shell 退出了的那几份在下一次说话时顺手清掉。

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use lm_engine::store::paths::Paths;
use lm_kernel::kernel::id::SessionId;

/// 一个终端的那一份记录。
#[derive(Debug)]
pub struct Terminal {
    path: PathBuf,
}

impl Terminal {
    /// 记号是 `token` 的终端。记号只许字母、数字、`-`（它要当文件名）；不是的、空的认不出。
    pub fn of(paths: &Paths, token: &str) -> Option<Terminal> {
        let plain = !token.is_empty()
            && token.len() <= 64
            && token.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
        plain.then(|| Terminal {
            path: dir(paths).join(token),
        })
    }

    /// 这个终端的会话：记着、会话的目录还在的才算（删掉了的照没说过话）。
    pub fn session(&self, paths: &Paths) -> Option<String> {
        let text = fs::read_to_string(&self.path).ok()?;
        let id = SessionId::parse(text.trim()).ok()?;
        paths.session(&id).is_dir().then(|| text.trim().to_string())
    }

    /// 记下这个终端的会话是 `id`。
    ///
    /// # Errors
    ///
    /// 写不进。
    pub fn remember(&self, id: &str) -> io::Result<()> {
        if let Some(dir) = self.path.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(&self.path, format!("{id}\n"))
    }

    /// 「有要印的」那个标记在哪：留着的进程在没有前台接着的时候有了新的东西（后台任务结束了、被叫醒开了一轮、在等
    /// 确认）就放上它、给 shell 发信号；脚本跑 `shellhook report` 时先看它在不在。名字开头也是 shell 的进程号，shell 退出
    /// 了的一样被清掉。
    pub fn pending(&self) -> PathBuf {
        let mut name = self.path.file_name().unwrap_or_default().to_os_string();
        name.push(".pending");
        self.path.with_file_name(name)
    }

    /// 这个终端没有会话了（`/new`）：下一句新开。本来就没记的不算错。
    ///
    /// # Errors
    ///
    /// 删不掉。
    pub fn forget(&self) -> io::Result<()> {
        match fs::remove_file(&self.path) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        }
    }
}

/// 这个终端留下来的进程的套接字放在哪（`wire.rs`：同一个终端再说一句、印回报时先连它）。放在只有自己进得去的目录里
/// （同窗口之间的门铃那两处，`lm_engine::session::private_spot`）；`create` 的没有那个目录就建。记号不合规矩、两处都不
/// 合用、地址太长的是 `None`：那样的终端不留下来。
pub fn socket(paths: &Paths, token: &str, create: bool) -> Option<PathBuf> {
    Terminal::of(paths, token)?;
    lm_engine::session::private_spot(&paths.bell, &format!("shellhook-{token}"), create)
}

/// 记录都在哪。
fn dir(paths: &Paths) -> PathBuf {
    paths.state.join("shellhook")
}

/// 清掉 shell 已经退出的那几份记录（文件名开头是 shell 的进程号），连同它们留下的套接字文件。读不了目录的不清。
pub fn sweep(paths: &Paths) {
    if let Ok(entries) = fs::read_dir(dir(paths)) {
        for entry in entries.flatten() {
            if gone(&entry.path()) {
                drop(fs::remove_file(entry.path()));
            }
        }
    }
    sweep_sockets(paths);
}

/// 清掉 shell 已经退出的那几个套接字文件：拿着会话的进程平常退出时自己删，崩了、被强行结束的来不及删，文件就一直
/// 留在运行时目录里（2026-11-23 看到：崩过两次，留下两个）。那个目录里还有别的程序的文件：只认名字是这里起的样子、
/// 记号开头认得出进程号、那个进程又确实不在了的。
fn sweep_sockets(paths: &Paths) {
    // 照一个假的记号问出套接字放在哪个目录、名字是什么样。
    const SAMPLE: &str = "0-0";
    let Some(sample) = socket(paths, SAMPLE, false) else {
        return;
    };
    let (Some(folder), Some(name)) = (
        sample.parent(),
        sample.file_name().and_then(|name| name.to_str()),
    ) else {
        return;
    };
    let Some((before, after)) = name.split_once(SAMPLE) else {
        return;
    };
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let token = name
            .to_str()
            .and_then(|name| name.strip_prefix(before))
            .and_then(|name| name.strip_suffix(after));
        if token.and_then(dead) == Some(true) {
            drop(fs::remove_file(entry.path()));
        }
    }
}

/// 这份记录的 shell 不在了：文件名认不出进程号的也算（不是这里写的样子）。
fn gone(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .and_then(dead)
        .unwrap_or(true)
}

/// 记号 `token`（开头是 shell 的进程号）的那个 shell 不在了没有；认不出进程号的是 `None`。
fn dead(token: &str) -> Option<bool> {
    let pid = token
        .split('-')
        .next()
        .and_then(|pid| pid.parse::<i32>().ok())
        .and_then(rustix::process::Pid::from_raw)?;
    // 没有这个进程（`ESRCH`）才算不在；没权限发信号的（别人的进程）是在。
    Some(rustix::process::test_kill_process(pid) == Err(rustix::io::Errno::SRCH))
}
