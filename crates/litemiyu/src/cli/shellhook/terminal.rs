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

/// 记录都在哪。
fn dir(paths: &Paths) -> PathBuf {
    paths.state.join("shellhook")
}

/// 清掉 shell 已经退出的那几份记录（文件名开头是 shell 的进程号）。读不了目录的不清。
pub fn sweep(paths: &Paths) {
    let Ok(entries) = fs::read_dir(dir(paths)) else {
        return;
    };
    for entry in entries.flatten() {
        if gone(&entry.path()) {
            drop(fs::remove_file(entry.path()));
        }
    }
}

/// 这份记录的 shell 不在了：文件名认不出进程号的也算（不是这里写的样子）。
fn gone(path: &Path) -> bool {
    let pid = path
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.split('-').next())
        .and_then(|pid| pid.parse::<i32>().ok())
        .and_then(rustix::process::Pid::from_raw);
    match pid {
        // 没有这个进程（`ESRCH`）才算不在；没权限发信号的（别人的进程）是在。
        Some(pid) => rustix::process::test_kill_process(pid) == Err(rustix::io::Errno::SRCH),
        None => true,
    }
}
