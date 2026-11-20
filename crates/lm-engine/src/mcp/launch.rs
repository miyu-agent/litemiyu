//! 怎么起一个服务（L5 六，Miyu `miyu-endpoint/src/mcp/launch.rs`，`mcp.md`「怎么走」第二条第 2、5 条）：程序照命令第一项，工作
//! 目录是状态那一处的 `mcp/<服务名>/`，环境是这个进程的加上配置写的，标准错误接到日志那一处的 `mcp-<服务名>.stderr`。不进沙盒
//! （LiteMiyu 没有沙盒）。网址的服务不起进程：计划里是网址和请求头，令牌换成 `Authorization`。
//!
//! 密钥（`env`、`bearer`、`headers` 里的 `{ secret }`、`{ env }`）在起它的这一刻照会话这时的配置取，取不到的那一个不设、记一行
//! `WARN`（只记名字）；取出来的值只进子进程的环境、请求头，不进日志、报错。
//!
//! 主程序没了它跟着没：服务起在一个看门的 `/bin/sh` 的进程组里（`crate::basesystem::watcher`，同 `shell` 的命令；Miyu 用沙盒的
//! 看门助手）。停的时候整组杀掉，服务自己再起的子进程（`npx` 起的 `node` 这一类）一起没。
//!
//! 和 Miyu 不一样的：命令第一项照原样交给系统找（只有名字的照 `PATH`，带路径的照工作目录算）：没有软件包，没有包目录和它的
//! `bin/`；不给服务另设状态目录的环境变量；程序正被写着的（`ExecutableFileBusy`）不重试。

use std::io;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use lm_kernel::config::mcp::{EnvValue, McpServer, Reach};
use lm_kernel::config::secret::{Reference, Secret};
use tokio::process::{Child, Command};

use crate::basesystem::Watcher;

/// 标准错误的文件拉起前超过这么大的挪走：1 MiB（同 Miyu 扩展的 `stderr::LARGEST`）。
const LARGEST: u64 = 1 << 20;

/// 怎么起一个服务。
#[derive(Debug, Clone)]
pub(crate) struct Plan {
    /// 起进程，还是连网址。
    pub(crate) how: How,
    /// 工作目录：没有的建。
    pub(crate) dir: PathBuf,
    /// 标准错误接到哪个文件；网址的服务对方回错这一类也记在这里。
    pub(crate) log: PathBuf,
}

/// 怎么到一个服务。取出来的密钥在里面：不打出来（不派生 `Debug` 的值，见下）。
#[derive(Clone)]
pub(crate) enum How {
    /// 起进程。
    Spawn {
        /// 程序：命令第一项。
        program: String,
        /// 参数：命令第一项后面的。
        args: Vec<String>,
        /// 另给的环境变量，引用的已经取出来了。
        env: Vec<(String, String)>,
    },
    /// 连网址。
    Connect {
        /// 网址。
        url: String,
        /// 每个请求都带的头，令牌已经换成 `Authorization: Bearer …`、引用的已经取出来了。
        headers: Vec<(String, String)>,
    },
}

/// 里面有密钥：只打种类。
impl std::fmt::Debug for How {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            How::Spawn { program, .. } => f
                .debug_struct("Spawn")
                .field("program", program)
                .finish_non_exhaustive(),
            How::Connect { .. } => f.debug_struct("Connect").finish_non_exhaustive(),
        }
    }
}

/// 服务 `server` 怎么起：引用的照 `secret` 这一刻取，取不到的那一个不设、记一行 `WARN`。工作目录 `dir`、标准错误 `log`。
pub(crate) fn plan(
    server: &McpServer,
    secret: &dyn Fn(&Reference) -> Option<Secret>,
    dir: PathBuf,
    log: PathBuf,
) -> Plan {
    let resolve = |pairs: &[(String, EnvValue)]| -> Vec<(String, String)> {
        pairs
            .iter()
            .filter_map(|(name, value)| {
                let resolved = match value {
                    EnvValue::Text(text) => Some(text.clone()),
                    EnvValue::Reference(reference) => {
                        secret(reference).map(|secret| secret.expose().to_string())
                    }
                };
                resolved.map(|value| (name.clone(), value))
            })
            .collect()
    };
    let how = match &server.reach {
        Reach::Command { command, env } => {
            let (first, rest) = command
                .split_first()
                .map_or(("", &[][..]), |(first, rest)| (first.as_str(), rest));
            How::Spawn {
                program: first.to_string(),
                args: rest.to_vec(),
                env: resolve(env),
            }
        }
        Reach::Url {
            url,
            bearer,
            headers,
        } => {
            // `bearer` 单独解开、单独冠上 `Bearer `：配置里自己写的 `Authorization` 照原样发（审计 #057）。
            let bearer = bearer.as_ref().and_then(|bearer| {
                resolve(&[("Authorization".to_string(), bearer.clone())])
                    .pop()
                    .map(|(name, token)| (name, format!("Bearer {token}")))
            });
            How::Connect {
                url: url.clone(),
                headers: bearer.into_iter().chain(resolve(headers)).collect(),
            }
        }
    };
    Plan { how, dir, log }
}

/// 起来的一个服务进程：进程、它的组、看门的。丢掉时整组杀掉，看门的跟着收掉（[`Watcher`] 的 `Drop`）。
pub(crate) struct Spawned {
    /// 服务进程：丢掉时 tokio 杀掉、在后台收掉（`kill_on_drop`）。
    pub(crate) child: Child,
    /// 组号：看门的进程号；看门的起不来的是服务自己的进程号。
    group: Option<u32>,
    /// 看门的：起不来的没有。
    _watcher: Option<Watcher>,
}

impl Spawned {
    /// 收尾：标准输入已经关了（客户端丢了），等它退出，最多等 `grace`，没退的杀掉；最后整组杀掉（它自己起的子进程）。
    pub(crate) async fn finish(&mut self, grace: Duration) {
        if tokio::time::timeout(grace, self.child.wait())
            .await
            .is_err()
        {
            drop(self.child.start_kill());
            drop(self.child.wait().await);
        }
        if let Some(group) = self.group.take() {
            kill_group(group);
        }
    }
}

impl Drop for Spawned {
    /// 没收尾就丢了（会话、程序在停）：整组杀掉。
    fn drop(&mut self) {
        if let Some(group) = self.group.take() {
            kill_group(group);
        }
    }
}

/// 起一次：工作目录没有的建，标准错误接到它的文件上，起在看门的组里。看门的起不来的记一行 `WARN`，照旧自成一组起。网址的
/// 服务没有进程，交回出错（调的一方先照 [`How`] 分开走，不会走到这里）。
pub(crate) fn spawn(plan: &Plan) -> io::Result<Spawned> {
    use std::os::unix::process::CommandExt;
    let How::Spawn { program, args, env } = &plan.how else {
        return Err(io::Error::other("a URL server has no process"));
    };
    std::fs::create_dir_all(&plan.dir)?;
    let errors = stderr(&plan.log)?;
    let mut command = std::process::Command::new(program);
    command
        .args(args)
        .envs(env.iter().map(|(name, value)| (name, value)))
        .current_dir(&plan.dir)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(errors);
    let watcher = match crate::basesystem::watcher() {
        Ok(watcher) => match i32::try_from(watcher.group()) {
            Ok(group) => {
                command.process_group(group);
                Some(watcher)
            }
            Err(_) => {
                command.process_group(0);
                None
            }
        },
        Err(_) => {
            command.process_group(0);
            None
        }
    };
    let mut command = Command::from(command);
    command.kill_on_drop(true);
    let child = command.spawn()?;
    let group = watcher.as_ref().map(Watcher::group).or_else(|| child.id());
    Ok(Spawned {
        child,
        group,
        _watcher: watcher,
    })
}

/// 打开 `path` 接标准错误（Miyu 扩展的 `stderr::open`）：目录没有的建，太大的先挪成 `.old`，追加。
pub(crate) fn stderr(path: &std::path::Path) -> io::Result<std::fs::File> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    if std::fs::metadata(path).is_ok_and(|meta| meta.len() > LARGEST) {
        let mut old = path.as_os_str().to_owned();
        old.push(".old");
        std::fs::rename(path, old)?;
    }
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
}

/// `kill(-pgid, SIGKILL)`（同 `shell` 的 `process/group.rs`）。组里已经没有进程的，不算出错。
fn kill_group(id: u32) {
    use rustix::process::{Pid, Signal, kill_process_group};
    let Some(pid) = i32::try_from(id).ok().and_then(Pid::from_raw) else {
        return;
    };
    match kill_process_group(pid, Signal::KILL) {
        Ok(()) => {}
        Err(error) if error == rustix::io::Errno::SRCH => {}
        Err(_) => (),
    }
}

#[cfg(test)]
mod tests;
