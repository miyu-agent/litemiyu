//! 主程序没了，它起的命令跟着没（施工 7-8，从 Miyu `miyu-sandbox` 的 `lifeline` 搬来，设计第四节第 4 条「用 Miyu 现成的退路：
//! 一行 `/bin/sh`」）。有计划地停下时引擎自己整组杀掉命令；这里管的是主程序崩了、被 `SIGKILL` 的时候：没人来杀，命令得自己
//! 知道主程序没了。
//!
//! 一根「生命线」管道，写端只在主程序手里（`O_CLOEXEC`，别的子进程拿不到），主程序怎么没的，内核都会关掉它。每条命令的进程组
//! 里先起一个看门的 `/bin/sh`，标准输入是读端：读到结尾（主程序没了），杀掉自己所在的整个组（`kill(0, SIGKILL)`）。命令起在
//! 这个组里（[`Watcher`]）。看门的只认自己的组，组里还有它就不会被别的组占了编号，所以不会杀错。Miyu 照沙盒的助手起看门的
//! （每条命令省约 0.3 ms 的 CPU），LiteMiyu 没有那个助手，只用 `/bin/sh`。
//!
//! 没用 Linux 的 `PR_SET_PDEATHSIG`：它盯的是起子进程的那个线程，阻塞线程一退，命令就被误杀；也只管直接的子进程，组里别的
//! 照样活。

use std::io;
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::sync::OnceLock;

/// 看门的那一段：读到结尾（生命线的写端全关了）就杀掉自己所在的整个组，自己也在里面。`read` 和 `kill` 都是内建的，不经
/// `PATH`；没人往管道里写，`read` 一直等到结尾。
const SCRIPT: &str = "while read -r _; do :; done; kill -s KILL 0";

/// 看门的 shell：不经 `PATH` 找。
const SHELL: &str = "/bin/sh";

/// 一根生命线：写端拿在手里，谁都不给；读端交给每个看门的。
#[derive(Debug)]
struct Lifeline {
    reader: io::PipeReader,
    /// 只拿着，不写：它关了（这个进程没了），看门的读到结尾。
    _writer: io::PipeWriter,
}

impl Lifeline {
    /// 一根新的生命线。两头都带 `O_CLOEXEC`（标准库建的管道都是）：起别的子进程时不会漏给它们。
    fn new() -> io::Result<Lifeline> {
        let (reader, writer) = io::pipe()?;
        Ok(Lifeline {
            reader,
            _writer: writer,
        })
    }

    /// 起一个看门的，自成一组（组号就是它的进程号）：环境变量清空、工作目录是 `/`、标准输入是生命线的读端、标准输出和错误
    /// 接空的，别的文件描述符一个都不带（标准库建的都带 `O_CLOEXEC`）。
    fn watcher(&self) -> io::Result<Watcher> {
        let child = Command::new(SHELL)
            .args(["-c", SCRIPT])
            .env_clear()
            .current_dir("/")
            .stdin(Stdio::from(self.reader.try_clone()?))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()?;
        Ok(Watcher { child })
    }
}

/// 这个进程的那一根：第一次要用时建，一直拿到进程结束。建不起来的，之后每次都说建不起来。
fn lifeline() -> io::Result<&'static Lifeline> {
    static LIFELINE: OnceLock<Result<Lifeline, String>> = OnceLock::new();
    LIFELINE
        .get_or_init(|| Lifeline::new().map_err(|error| error.to_string()))
        .as_ref()
        .map_err(|error| io::Error::other(error.clone()))
}

/// 在这个进程的生命线上起一个看门的。
///
/// # Errors
///
/// 生命线建不起来，或者看门的起不来。
pub(crate) fn watcher() -> io::Result<Watcher> {
    lifeline()?.watcher()
}

/// 一个组里看门的。整组杀的时候它跟着没；丢掉它时杀掉它、收掉它，不留僵尸进程。
#[derive(Debug)]
pub(crate) struct Watcher {
    child: Child,
}

impl Watcher {
    /// 它的组：命令起在这个组里（`process_group`），整组杀也照它（`kill(-组号)`）。
    pub(crate) fn group(&self) -> u32 {
        self.child.id()
    }
}

impl Drop for Watcher {
    /// 杀掉、收掉。它还没被收过，编号还是它的，按编号杀不会杀错；已经跟着整组没了的，杀它是空的，收它马上回来。
    #[expect(
        clippy::let_underscore_must_use,
        reason = "已经没了的杀不了、收不了都不要紧：只为不留僵尸进程"
    )]
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(test)]
mod tests;
