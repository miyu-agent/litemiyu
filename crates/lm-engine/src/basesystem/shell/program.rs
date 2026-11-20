//! 用哪个 shell、怎么起（`10-自带软件.md` 第八节、B8，施工 4-8）：bash，先找 `PATH` 里的，再看 `/bin/bash`。起来时找一次。
//! Miyu 在 macOS 上用 zsh，LiteMiyu 照旧用 bash（macOS 自带 `/bin/bash`）。BSD 默认不装 bash（LM-5）：都找不到的退回 POSIX 的
//! `sh`（`PATH` 里的，再是 `/bin/sh`），说明里、报错里跟着写 `sh`，她照它写命令。
//!
//! 不读用户的启动文件：那里可能 `export` 了密钥，去掉密钥的那一步就白做了。`PATH` 照 [`super::env`] 从主程序的环境带过去。
//!
//! 命令不写进命令行（施工 4-8 再补）：放在环境变量 [`COMMAND`] 里，`-c` 后面是一句固定的 [`RUN`]。命令行里有命令原文时，命令
//! 里的 `pkill -f <词>` 会连自己所在的 shell 一起杀掉（实测两次，只剩一句 `Killed by signal 15`）。

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Command;

#[cfg(test)]
mod tests;

/// 这台机器上用的 shell：程序在哪、是哪一种。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Program {
    /// 程序在哪。
    path: PathBuf,
    /// 是 bash 还是 POSIX 的 sh。
    kind: Kind,
}

/// 哪一种 shell。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    /// bash：带 `--noprofile --norc` 起，不读启动文件。
    Bash,
    /// POSIX 的 sh：`-c` 起的不交互，本来就不读启动文件（`ENV` 只给交互的读）。
    Sh,
}

/// 放她的命令的环境变量（施工 4-8 再补）。
const COMMAND: &str = "LITEMIYU_COMMAND";

/// `-c` 后面那一句：先展开出命令原文，再去掉 [`COMMAND`]，命令和它起的子进程都看不到这个变量；`eval` 的退出码就是命令的。
const RUN: &str = r#"eval "unset LITEMIYU_COMMAND; $LITEMIYU_COMMAND""#;

impl Program {
    /// 照这台机器找：`path` 是主程序环境里的 `PATH`。
    pub(super) fn find(path: Option<&OsStr>) -> Program {
        Program::find_at(path, Path::new("/bin/bash"), Path::new("/bin/sh"))
    }

    /// 同 [`Program::find`]，`bash`、`sh` 是 `PATH` 里没有时看的那两处（测试换成假的）：bash 在 `PATH` 里、在 `bash` 那一处的用
    /// bash；都没有的用 sh，`PATH` 里没有的照 `sh` 那一处（不在也照它，起的时候报起不来）。
    pub(super) fn find_at(path: Option<&OsStr>, bash: &Path, sh: &Path) -> Program {
        if let Some(found) =
            which(path, "bash").or_else(|| bash.is_file().then(|| bash.to_path_buf()))
        {
            return Program {
                path: found,
                kind: Kind::Bash,
            };
        }
        Program {
            path: which(path, "sh").unwrap_or_else(|| sh.to_path_buf()),
            kind: Kind::Sh,
        }
    }

    /// 写进说明里、报错里的名字：她照它写命令。
    pub(super) fn name(&self) -> &'static str {
        match self.kind {
            Kind::Bash => "bash",
            Kind::Sh => "sh",
        }
    }

    /// 在工作目录 `cwd` 里执行 `script` 的命令，只带 `env` 里的环境变量。
    pub(super) fn command(
        &self,
        script: &str,
        cwd: &Path,
        env: Vec<(OsString, OsString)>,
    ) -> Command {
        let mut command = Command::new(&self.path);
        if self.kind == Kind::Bash {
            command.args(["--noprofile", "--norc"]);
        }
        command
            .args(["-c", RUN])
            .current_dir(cwd)
            .env_clear()
            .envs(env)
            .env(COMMAND, script);
        command
    }
}

/// 在 `path` 列的目录里找叫 `name` 的程序。相对路径的那几项不看（施工 R3-1 补）：照它找到的跟着进程的工作目录变。
pub(super) fn which(path: Option<&OsStr>, name: &str) -> Option<PathBuf> {
    std::env::split_paths(path?)
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}
