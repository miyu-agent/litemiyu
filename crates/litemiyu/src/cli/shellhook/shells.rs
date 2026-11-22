//! 认得的 shell：叫什么、装在哪、调程序的那一行怎么加引号。帮助、报错、文档里写的三种 shell 都照 [`ALL`] 这一张表
//! （施工线 shellhook 的坑 A8）。

use std::path::{Path, PathBuf};

/// 认得的 shell。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shell {
    Fish,
    Bash,
    Zsh,
}

/// 都在这：`status` 照这个顺序一行一种。
pub const ALL: [Shell; 3] = [Shell::Fish, Shell::Bash, Shell::Zsh];

impl Shell {
    /// 照名字认：`fish`、`bash`、`zsh`，也认带路径的（`/usr/bin/fish`）和登录 shell 的 `-fish`。
    pub fn named(name: &str) -> Option<Shell> {
        let base = Path::new(name).file_name()?.to_str()?;
        ALL.into_iter()
            .find(|shell| shell.name() == base.trim_start_matches('-'))
    }

    /// 名字。
    pub fn name(self) -> &'static str {
        match self {
            Shell::Fish => "fish",
            Shell::Bash => "bash",
            Shell::Zsh => "zsh",
        }
    }

    /// 配置目录（`XDG_CONFIG_HOME`，没设的是 `~/.config`）下 fish 的 `conf.d`。
    fn fish_conf(env: &impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
        let config = env("XDG_CONFIG_HOME")
            .filter(|dir| !dir.is_empty())
            .map(PathBuf::from)
            .or_else(|| home(env).map(|home| home.join(".config")))?;
        Some(config.join("fish/conf.d"))
    }

    /// 装在哪：fish 是 `conf.d` 下自己的一个文件；bash、zsh 是启动文件里的一段。`env` 读环境变量（测试里换掉）。
    pub fn target(self, env: &impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
        match self {
            Shell::Fish => Some(Shell::fish_conf(env)?.join("litemiyu-shellhook.fish")),
            Shell::Bash => Some(home(env)?.join(".bashrc")),
            Shell::Zsh => {
                let dir = env("ZDOTDIR")
                    .filter(|dir| !dir.is_empty())
                    .map(PathBuf::from)
                    .or_else(|| home(env))?;
                Some(dir.join(".zshrc"))
            }
        }
    }

    /// 别家的集成整个文件是它自己的那几份（只有 fish 有）：Miyu 的、旧版 Miyu 0.7.x 的。
    pub fn other_files(self, env: &impl Fn(&str) -> Option<String>) -> Vec<(PathBuf, Other)> {
        match (self, Shell::fish_conf(env)) {
            (Shell::Fish, Some(dir)) => vec![
                (dir.join("miyu-shellhook.fish"), Other::Miyu),
                (dir.join("miyu.fish"), Other::OldMiyu),
            ],
            _ => Vec::new(),
        }
    }

    /// 把一个路径写成这种 shell 里的一个词：单引号包起来。
    pub fn quote(self, path: &Path) -> String {
        let text = path.to_string_lossy();
        match self {
            // fish 的单引号里只有 \' 和 \\ 是转义。
            Shell::Fish => format!("'{}'", text.replace('\\', "\\\\").replace('\'', "\\'")),
            // POSIX 的单引号里什么都不转义：引号先关上，转义的引号，再打开。
            Shell::Bash | Shell::Zsh => format!("'{}'", text.replace('\'', "'\\''")),
        }
    }
}

/// 别家的集成（都接管回车：两份都在，一句话会被发送两遍，坑 A7）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Other {
    /// Miyu 的 shell 集成（`miyu shellhook install` 写的）。
    Miyu,
    /// 旧版 Miyu 0.7.x 的终端集成。
    OldMiyu,
    /// 启动文件里看起来像 Miyu 集成的一行：认不准，只提醒。
    Maybe,
}

fn home(env: &impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
    env("HOME")
        .filter(|home| !home.is_empty())
        .map(PathBuf::from)
}

/// 照上一层进程认是哪种 shell（`litemiyu shellhook install` 是在 shell 里敲的），认不出的照 `SHELL`。
pub fn detect() -> Option<Shell> {
    parent_name().as_deref().and_then(Shell::named).or_else(|| {
        std::env::var("SHELL")
            .ok()
            .as_deref()
            .and_then(Shell::named)
    })
}

/// 上一层进程叫什么：Linux 上读 `/proc`，别的系统不认（照 `SHELL`）。
fn parent_name() -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        let parent = std::os::unix::process::parent_id();
        let name = std::fs::read_to_string(format!("/proc/{parent}/comm")).ok()?;
        Some(name.trim().to_string())
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}
