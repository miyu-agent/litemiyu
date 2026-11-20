//! 数据放哪（`docs/00-设计.md` 第六节）：照 XDG 的四处，各自用到时才建。
//!
//! | 哪一处 | 默认 | 放什么 |
//! |---|---|---|
//! | 配置 | `$XDG_CONFIG_HOME/litemiyu`，没设的 `~/.config/litemiyu` | `config.toml`、`secrets.toml`、`prompt.md`、`skills/`、`lang/` |
//! | 数据 | `$XDG_DATA_HOME/litemiyu`，没设的 `~/.local/share/litemiyu` | `sessions/<编号>/`、`blobs/`、`memory/` |
//! | 缓存 | `$XDG_CACHE_HOME/litemiyu`，没设的 `~/.cache/litemiyu` | 模型目录、在用模型的小缓存、MCP 的工具目录 |
//! | 状态 | `$XDG_STATE_HOME/litemiyu`，没设的 `~/.local/state/litemiyu` | 运行日志、输入历史 |
//!
//! `LITEMIYU_HOME` 指到一个目录时四处都在它下面（`config/`、`data/`、`cache/`、`state/`）：测试用，绝不碰真实数据。XDG 的
//! 变量照规范只认绝对路径，写了相对路径的当没写。
//!
//! 另有窗口之间的门铃（LM-4，[`Bell`]）放在哪：先 `$XDG_RUNTIME_DIR`，不合用的退到 `/tmp/litemiyu-<uid>/`（设了 `LITEMIYU_HOME`
//! 的退到它下面的 `litemiyu-<uid>/`，不碰系统的 `/tmp`）。门铃不是数据，`LITEMIYU_HOME` 不挪 `$XDG_RUNTIME_DIR` 那一处。
//!
//! 只照一份环境快照（[`Env`]）算，不直接读进程的环境：测试喂一份快照就行，不用改进程的环境变量（改了会串到同时跑的别的测试）。

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

use lm_kernel::kernel::id::SessionId;

/// 程序在几处目录里的那一层名字。
const NAME: &str = "litemiyu";

/// 找目录要看的几个环境变量，从进程里读一次（[`Env::current`]）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Env {
    /// `LITEMIYU_HOME`。
    pub litemiyu_home: Option<OsString>,
    /// `HOME`。
    pub home: Option<OsString>,
    /// `XDG_CONFIG_HOME`。
    pub xdg_config_home: Option<OsString>,
    /// `XDG_DATA_HOME`。
    pub xdg_data_home: Option<OsString>,
    /// `XDG_CACHE_HOME`。
    pub xdg_cache_home: Option<OsString>,
    /// `XDG_STATE_HOME`。
    pub xdg_state_home: Option<OsString>,
    /// `XDG_RUNTIME_DIR`：门铃放在这里（LM-4，[`Bell`]）。
    pub xdg_runtime_dir: Option<OsString>,
}

impl Env {
    /// 这个进程的环境。
    pub fn current() -> Env {
        let var = |name: &str| std::env::var_os(name).filter(|value| !value.is_empty());
        Env {
            litemiyu_home: var("LITEMIYU_HOME"),
            home: var("HOME"),
            xdg_config_home: var("XDG_CONFIG_HOME"),
            xdg_data_home: var("XDG_DATA_HOME"),
            xdg_cache_home: var("XDG_CACHE_HOME"),
            xdg_state_home: var("XDG_STATE_HOME"),
            xdg_runtime_dir: var("XDG_RUNTIME_DIR"),
        }
    }
}

/// 找不到放数据的地方：没设 `LITEMIYU_HOME`，也不知道家目录在哪。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoHome;

impl std::fmt::Display for NoHome {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("no place for the data: neither LITEMIYU_HOME nor HOME is set")
    }
}

impl std::error::Error for NoHome {}

/// 四处目录。只算路径，不建；用到时由写的一方建（`crate::store::durable`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    /// 配置。
    pub config: PathBuf,
    /// 数据。
    pub data: PathBuf,
    /// 缓存：都能删了重建。
    pub cache: PathBuf,
    /// 状态。
    pub state: PathBuf,
    /// 窗口之间的门铃放在哪、多久兜底扫一次收件目录（LM-4）。
    pub bell: Bell,
}

/// 窗口之间的门铃（LM-4，`crate::session::peers` 的 `bell.rs`）：开着的窗口在这里绑一个数据报套接字，发的一方把信放进它的
/// 收件目录以后往这里敲一个字节。只是叫醒，信照旧走文件；敲不响（没开着、套接字没了）信也在盘上，收的一方定时兜底扫。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bell {
    /// 先放这里：`$XDG_RUNTIME_DIR`（要是自己的、别人进不去的目录，用时才查）。没有的、不是绝对路径的是 `None`。
    pub runtime: Option<PathBuf>,
    /// 上一处不合用时在这里建自己的 `litemiyu-<uid>/`（0700，建完照 `lstat` 核对属主、权限）：系统的 `/tmp`；设了
    /// `LITEMIYU_HOME` 的是它。
    pub tmp: PathBuf,
    /// 绑上了门铃的窗口多久兜底扫一次收件目录：门铃丢了、套接字被删了也不丢信（2026-11-18 定 30 秒）。
    pub every: Duration,
    /// 两处都绑不上门铃的窗口多久扫一次：只靠扫，扫得勤些。
    pub bare: Duration,
}

impl Bell {
    /// 绑上门铃时兜底扫的间隔。
    pub const EVERY: Duration = Duration::from_secs(30);

    /// 绑不上门铃时扫的间隔。
    pub const BARE: Duration = Duration::from_secs(5);

    /// 先 `runtime`，不合用的在 `tmp` 下建自己的目录；间隔是默认的两个。
    pub fn new(runtime: Option<PathBuf>, tmp: PathBuf) -> Bell {
        Bell {
            runtime,
            tmp,
            every: Bell::EVERY,
            bare: Bell::BARE,
        }
    }
}

impl Paths {
    /// 照环境 `env` 找四处。`LITEMIYU_HOME` 设了的全在它下面，别的照 XDG，XDG 的变量没设、不是绝对路径的照家目录下的默认。
    ///
    /// # Errors
    ///
    /// 没设 `LITEMIYU_HOME`，也没有家目录。
    pub fn locate(env: &Env) -> Result<Paths, NoHome> {
        let runtime = env
            .xdg_runtime_dir
            .as_ref()
            .map(PathBuf::from)
            .filter(|dir| dir.is_absolute());
        if let Some(home) = &env.litemiyu_home {
            let mut paths = Paths::under(Path::new(home));
            paths.bell.runtime = runtime;
            return Ok(paths);
        }
        let home = env.home.as_ref().map(PathBuf::from).ok_or(NoHome)?;
        let pick = |xdg: &Option<OsString>, fallback: &str| {
            xdg.as_ref()
                .map(PathBuf::from)
                .filter(|dir| dir.is_absolute())
                .unwrap_or_else(|| home.join(fallback))
                .join(NAME)
        };
        Ok(Paths {
            config: pick(&env.xdg_config_home, ".config"),
            data: pick(&env.xdg_data_home, ".local/share"),
            cache: pick(&env.xdg_cache_home, ".cache"),
            state: pick(&env.xdg_state_home, ".local/state"),
            bell: Bell::new(runtime, PathBuf::from("/tmp")),
        })
    }

    /// 全在一个目录 `home` 下面（同设了 `LITEMIYU_HOME`）：测试用。门铃不看 `$XDG_RUNTIME_DIR`，在 `home` 下建自己的目录。
    pub fn under(home: &Path) -> Paths {
        Paths {
            config: home.join("config"),
            data: home.join("data"),
            cache: home.join("cache"),
            state: home.join("state"),
            bell: Bell::new(None, home.to_path_buf()),
        }
    }

    /// 配置文件：`<配置>/config.toml`。
    pub fn config_file(&self) -> PathBuf {
        self.config.join("config.toml")
    }

    /// 密钥文件：`<配置>/secrets.toml`（[`crate::store::secrets`]）。
    pub fn secrets_file(&self) -> PathBuf {
        self.config.join(crate::store::secrets::FILE)
    }

    /// 配置、密钥的改动记在哪：`<状态>/journal.jsonl`（[`crate::store::journal`]）。
    pub fn journal(&self) -> PathBuf {
        self.state.join(crate::store::journal::FILE)
    }

    /// 日志的目录：`<状态>/logs`（现在只放 MCP 服务的标准错误，`mcp/servers.rs`；LM-1 #058 去掉了没人订阅的运行日志）。
    pub fn logs(&self) -> PathBuf {
        self.state.join("logs")
    }

    /// 全部会话的目录：`<数据>/sessions`。
    pub fn sessions(&self) -> PathBuf {
        self.data.join("sessions")
    }

    /// 一个会话的目录：`<数据>/sessions/<编号>`，日志、后台命令的输出、收件目录、锁都在里面。
    pub fn session(&self, id: &SessionId) -> PathBuf {
        self.sessions().join(id.as_str())
    }

    /// blob：`<数据>/blobs`。
    pub fn blobs(&self) -> PathBuf {
        self.data.join("blobs")
    }

    /// 路由用出来的模型资料（学到的上下文窗口、池的指针）：`<状态>/models`。丢了要重新学。
    pub fn model_state(&self) -> PathBuf {
        self.state.join("models")
    }

    /// 拉到的模型资料（目录、记下来的查目录的答、供应商的列表）：`<缓存>/models`。能重新拉。
    pub fn model_cache(&self) -> PathBuf {
        self.cache.join("models")
    }

    /// 记忆的文件夹：`<数据>/memory`（设计第五节第 7 条）。
    pub fn memory(&self) -> PathBuf {
        self.data.join("memory")
    }

    /// 人自己放技能的文件夹：`<配置>/skills`（设计第五节第 10 条），一个技能一个子文件夹。工具能读能写：她要能给自己写技能。
    pub fn skills(&self) -> PathBuf {
        self.config.join("skills")
    }

    /// 自带的技能写出来的地方：`<缓存>/skills`（`crate::skills::shipped`）。原文编在程序里，删了下次造会话时重写。
    pub fn shipped_skills(&self) -> PathBuf {
        self.cache.join("skills")
    }
}

#[cfg(test)]
mod tests;
