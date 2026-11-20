//! 和 herdr 一起用（照 miyu-tui `notify/herdr.rs` 搬来）：在 herdr 的一个窗格里运行时，把工作中、等你回复、空闲报给 herdr，
//! 由它显示在侧栏上；主会话换了的时候把恢复命令 `<命令名> -s <编号>` 一起报上去，herdr 重启以后那个窗格接回原来的会话。
//!
//! 不在 herdr 里（[`Herdr::detect`] 交回 `None`）：除了读那三个环境变量什么都不做，不起线程，不留任何结构。
//!
//! 在 herdr 里：上报是调 herdr 自己的命令（`HERDR_BIN_PATH`），由一条平时睡着的线程挨个调、挨个等它退出（不留僵尸进程），
//! 主循环只往通道里放一份参数，不等。调的命令的输入输出都接空设备：报不上去的不往终端写字（那是全屏界面所在的终端）。
//!
//! 和 Miyu 不一样的：恢复命令是 `-s <编号>`（LiteMiyu 没有 `tui --resume`）；命令名照这个程序启动时的第一个参数取（Miyu 的
//! 终端由核心转交，照 `MIYU_PROGRAM`）；退出时等上报的线程最多等 [`CLOSING`]，herdr 的命令卡住了程序照样退出（Miyu 一直等）；
//! 没有系统通知和提示音要分工（LiteMiyu 没有这两样）。

use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// 报给 herdr 的来源、名字：herdr 照来源记序号的水位。
const SOURCE: &str = "custom:litemiyu";
const AGENT: &str = "litemiyu";

/// 上报的线程的栈：它只起命令、等命令退出，用不着默认的 2 MB。
const STACK: usize = 128 * 1024;

/// 退出时等排着的上报报完，最多等这么久：到点照样退出。
const CLOSING: Duration = Duration::from_secs(1);

/// 报的状态（herdr 的 `--state`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// 工作中：一轮在跑，或者派出去的子代理还在跑。
    Working,
    /// 等你回复：确认、提问的抽屉开着。
    Blocked,
    /// 空闲：等你打字。
    Idle,
}

impl State {
    fn word(self) -> &'static str {
        match self {
            State::Working => "working",
            State::Blocked => "blocked",
            State::Idle => "idle",
        }
    }
}

/// 在 herdr 的哪个窗格里、报到哪了。
pub struct Herdr {
    /// 交给上报线程的一头：一份是一次命令的参数。
    reports: Option<mpsc::Sender<Vec<String>>>,
    /// 上报线程退出时这一头跟着断开（它从不发东西）：退出时照它等。
    ended: Option<mpsc::Receiver<()>>,
    /// 上一次报的主会话。
    session: Option<String>,
    /// 恢复命令打头的命令名（后面接 `-s <编号>`）：PATH 上找不到的是 `None`，只登记会话编号、不报恢复命令。
    resume: Resume,
    pane: String,
    /// 下一个序号：从进程启动那一刻的毫秒数起往上加。herdr 丢掉比水位小的，每次从 1 数的话，同一个窗格里第二次打开程序以后的
    /// 上报全被丢掉（Miyu 旧版踩过）。
    next: u64,
    /// 上一次报的状态：一样的不再报。
    last: Option<State>,
}

/// 恢复命令打头的命令名，头一次要报会话时才算：拆路径、在 PATH 上找文件这几个函数排在「说过话」才用到的那一片代码里，
/// 一起来就算的话，空闲的窗口要为它多装进几片邻居页（LM-21 量的：三处各 64 kB）。
enum Resume {
    /// 还没算：启动时的第一个参数、那时的 `PATH`。
    Later {
        program: Option<String>,
        path: String,
    },
    /// 算过了。
    Known(Option<String>),
}

impl Resume {
    fn name(&mut self) -> Option<&str> {
        if let Resume::Later { program, path } = self {
            *self = Resume::Known(resume_program(program.as_deref(), path));
        }
        match self {
            Resume::Known(name) => name.as_deref(),
            Resume::Later { .. } => None,
        }
    }
}

impl Herdr {
    /// 照环境变量认：`HERDR_ENV` 是 `1`、有 `HERDR_PANE_ID` 和 `HERDR_BIN_PATH` 才算（用它注入的绝对路径，不受 PATH 影响）。
    /// `program` 交回这个程序启动时的第一个参数（人敲的那个命令），在 herdr 里才问它。不算的、起不了线程的交回 `None`。
    pub fn detect(
        var: impl Fn(&str) -> Option<String>,
        program: impl FnOnce() -> Option<String>,
    ) -> Option<Self> {
        if var("HERDR_ENV").as_deref() != Some("1") {
            return None;
        }
        let pane = var("HERDR_PANE_ID").filter(|p| !p.is_empty())?;
        let binary = var("HERDR_BIN_PATH").filter(|b| !b.is_empty())?;
        // 只有一条线程，照先后调：登记恢复命令要先取得窗格，退还不能被后面的状态越过。
        let (reports, receiver) = mpsc::channel::<Vec<String>>();
        let (alive, ended) = mpsc::channel::<()>();
        thread::Builder::new()
            .name("herdr".into())
            .stack_size(STACK)
            .spawn(move || {
                let _alive = alive;
                for args in receiver {
                    // 报不上去的不管：herdr 可有可无，下一次照报。不往终端写字，也不把应急模式的 key 带给它。
                    let reported = Command::new(&binary)
                        .args(&args)
                        .env_remove(crate::rescue::API_KEY)
                        .stdin(Stdio::null())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .status();
                    drop(reported);
                }
            })
            .ok()?;
        Some(Self {
            reports: Some(reports),
            ended: Some(ended),
            session: None,
            resume: Resume::Later {
                program: program(),
                path: var("PATH").unwrap_or_default(),
            },
            pane,
            next: first_seq(),
            last: None,
        })
    }

    /// 这一刻的状态和主会话报上去，放进通道就回，不等；都和上一次一样的不报。主会话没了（`/new`、删了当前会话）的先退还
    /// 窗格（herdr 清掉原来的恢复命令），再照现在的状态取回来。
    pub fn sync(&mut self, state: State, session: Option<&str>) {
        let same_session = self.session.as_deref() == session;
        if same_session && self.last == Some(state) {
            return;
        }
        if !same_session {
            self.session = session.map(str::to_string);
            if session.is_none() {
                let args = self.release_args();
                self.send(args);
            }
        }
        self.last = Some(state);
        let args = self.report_args(state);
        self.send(args);
    }

    /// 人真的退出了：退还这个窗格，herdr 当场清掉名字、状态和恢复命令。终端被挂断（herdr 关了、关机）退出的不调它，留着
    /// 恢复命令给 herdr 重启时用。
    pub fn release(&mut self) {
        let args = self.release_args();
        self.send(args);
    }

    /// 交给上报的线程；线程没了的丢掉。
    fn send(&self, args: Vec<String>) {
        if let Some(reports) = &self.reports {
            drop(reports.send(args));
        }
    }

    fn report_args(&mut self, state: State) -> Vec<String> {
        let seq = self.seq();
        let mut args: Vec<String> = [
            "pane",
            "report-agent",
            &self.pane,
            "--source",
            SOURCE,
            "--agent",
            AGENT,
            "--state",
            state.word(),
            "--seq",
            &seq,
        ]
        .map(str::to_string)
        .to_vec();
        // 会话编号登记成这个窗格的会话（`--agent-session-id`）：herdr 重启以后只给登记了会话的窗格跑恢复命令。
        if let Some(session) = &self.session {
            args.extend(["--agent-session-id".into(), session.clone()]);
            if let Some(program) = self.resume.name() {
                args.extend(["--".into(), program.into(), "-s".into(), session.clone()]);
            }
        }
        args
    }

    /// 退还这个窗格：带序号（不带的 herdr 不认，侧栏上一直挂着）。
    fn release_args(&mut self) -> Vec<String> {
        let seq = self.seq();
        [
            "pane",
            "release-agent",
            &self.pane,
            "--source",
            SOURCE,
            "--agent",
            AGENT,
            "--seq",
            &seq,
        ]
        .map(str::to_string)
        .to_vec()
    }

    fn seq(&mut self) -> String {
        let seq = self.next;
        self.next += 1;
        decimal(seq)
    }
}

/// `n` 写成十进制的字。不用标准库的格式化：那几个函数排在别的代码片里，空闲的窗口为第一次上报的序号要多装进一片邻居页
/// （LM-21 量的：64 kB）。
fn decimal(mut n: u64) -> String {
    let mut digits = [0u8; 20];
    let mut at = digits.len();
    loop {
        at -= 1;
        digits[at] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    digits[at..].iter().map(|d| char::from(*d)).collect()
}

/// 第一个序号：这一刻的毫秒数（见 [`Herdr`] 的 `next`）。
fn first_seq() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// 恢复命令打头的命令名：herdr 要「PATH 上的一个命令名，不是路径」。照人敲的那个命令的文件名写；PATH 上找不到这个名字的不报
/// 恢复命令（报了 herdr 重启时也跑不起来）。
fn resume_program(program: Option<&str>, path: &str) -> Option<String> {
    let name = std::path::Path::new(program?)
        .file_name()?
        .to_str()?
        .to_string();
    std::env::split_paths(path)
        .any(|dir| dir.join(&name).is_file())
        .then_some(name)
}

/// 界面退出（出错退出的也算）：等排着的报完，最多等 [`CLOSING`]；每一条命令线程都等它退出了才调下一条，不留僵尸进程。
impl Drop for Herdr {
    fn drop(&mut self) {
        self.reports.take();
        if let Some(ended) = self.ended.take() {
            // 线程退出时那一头断开（交回的是错误）；到点还没断开的不再等。
            if ended.recv_timeout(CLOSING).is_err() {}
        }
    }
}

#[cfg(test)]
mod tests;
