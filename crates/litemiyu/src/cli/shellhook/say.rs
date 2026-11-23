//! `shellhook say`：前台的那个进程。shell 的脚本把那句话从标准输入交来，这里起一个后台的进程（`worker.rs`）去办，自己只
//! 做三件事：把终端切到原始模式并在任何退出的路上还原、把那句话交过去、等后台的进程说办完了。它一退出，shell 就回到提示符。
//!
//! 为什么分两个进程（施工线 shellhook 第三节「后台、循环和到点继续」）：后台命令、循环、到点继续都活在拿着会话的那个进程里，
//! 一轮结束以后提示符要马上回来、这些又不能跟着没了，所以拿着会话的进程要能比这一句活得久。这个仓库禁用 `unsafe`，不能
//! `fork`，后台的进程是再启动一次自己（`shellhook worker`）。现在后台的进程还不留下来，这一句办完就一起退出。
//!
//! 终端直接交给后台的进程用（它的标准输入、标准输出就是终端这个设备），不经这里转交字节：读按键用的是全屏界面那一套
//! （crossterm），它只认「0 号描述符是终端就读它」，接成套接字的话它会绕过这里自己去开终端。后台的进程和这个进程在同一个
//! 进程组里（shell 给这条命令的前台进程组），读得到按键、收得到终端变大小的信号。
//!
//! 终端的规矩：
//!
//! - 那句话是从管道交来的，读完以后终端另开一份来用（坑 P1；开的是终端的设备本身，见 `terminal`）。
//! - 原始模式在这一头进、这一头出：只关掉输入这一边的处理（不回显、不攒整行、`Ctrl+C` 是一个字节不是信号），输出照旧把换行
//!   补成回车换行。后台的进程崩了、被杀了，终端也由这一头还原（发行版是 `panic = "abort"`，另装了一个出错时还原的钩子）。
//! - 后台的进程说办完了以后不再读终端：之后敲的字留在终端自己的缓冲里，shell 回到提示符时自己读得到，不丢（坑 P8）。

use std::fs::File;
use std::io;
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::process::{Command, ExitCode, Stdio};

use rustix::event::{PollFd, PollFlags, poll};
use rustix::termios::{
    InputModes, LocalModes, OptionalActions, OutputModes, SpecialCodeIndex, Termios, tcgetattr,
    tcsetattr,
};

use super::wire::{Hello, ToFront, ToWorker};
use crate::cli::ask::{NoMessage, message};
use crate::cli::misuse;
use crate::cli::parse::Parsed;
use crate::cli::words::Words;

/// 原样带给后台进程的选项。
const PASSED: [&str; 3] = ["shell-pid", "terminal", "leftover"];

/// 办 `shellhook say`。`stale` 是这个 shell 里的脚本是不是升级以前印的（答完说一声重开终端，坑 A2）。
pub fn run(parsed: &Parsed, stale: bool) -> ExitCode {
    let text = match message(&[]) {
        Ok(text) => text,
        // 什么都没说：不算错。
        Err(NoMessage::Missing) => return ExitCode::SUCCESS,
        Err(NoMessage::NotUtf8) => return told(misuse::USAGE, "stdin_not_utf8"),
        Err(NoMessage::TooLong) => return told(misuse::USAGE, "too_long"),
        Err(NoMessage::Unreadable(why)) => {
            let words = Words::current();
            let reason = words.why(&why);
            words.error(&words.say("stdin_unreadable", &[("reason", &reason)]), None);
            return ExitCode::FAILURE;
        }
    };
    let code = match hand_over(parsed, &text) {
        Ok(code) => code,
        Err(error) => {
            let words = Words::current();
            let reason = words.why(&error.to_string());
            words.error(
                &words.say("shellhook.say_failed", &[("reason", &reason)]),
                None,
            );
            1
        }
    };
    if stale {
        let words = Words::current();
        words.warn(words.get("shellhook.stale"), None);
    }
    ExitCode::from(code)
}

/// 说一句错，交回退出码。
fn told(code: u8, key: &str) -> ExitCode {
    let words = Words::current();
    words.error(words.get(key), None);
    ExitCode::from(code)
}

/// 同一个终端留着的进程在的话接上它，不在的起后台的进程；等到它说这一句办完了，交回退出码。
fn hand_over(parsed: &Parsed, text: &str) -> io::Result<u8> {
    // 终端：那句话是从管道交来的，标准输入不是它（坑 P1）。没有终端的（被别的程序调）照样办：后台的进程没有按键可读，
    // 印在这个进程的标准输出上。
    let tty = terminal();
    // 先进原始模式再把这一句交出去：中间没有「`Ctrl+C` 还是信号、后台的进程还没装好处理」的空当。
    let raw = tty.as_ref().and_then(|tty| Raw::enter(tty).ok());
    let (mut control, mut child) = match attach(parsed, Some(text)) {
        Some(control) => (control, None),
        None => {
            let (mut control, child) = spawn(parsed, tty.as_ref())?;
            ToWorker::Message(text.to_string()).write(&mut control)?;
            (control, Some(child))
        }
    };
    let said = wait(&mut control);
    drop(raw);
    let (said, fullscreen) = said?;
    // 后台的进程没说退出码就没了：它可能把光标藏着。没有脚本在后面亮光标的时候（直接敲这条命令）也亮回来。
    if said.is_none()
        && let Some(mut tty) = tty.as_ref()
    {
        use std::io::Write;
        drop(tty.write_all(b"\x1b[?25h"));
    }
    // 后台的进程请这一头起全屏界面（`/config`、`/tui`）：它没有开会话，终端也还原了。全屏界面直接用终端。
    if let (Some(args), Some(tty)) = (fullscreen, &tty) {
        let status = Command::new(std::env::current_exe()?)
            .args(args)
            .stdin(Stdio::from(tty.try_clone()?))
            .stdout(Stdio::from(tty.try_clone()?))
            .stderr(Stdio::from(tty.try_clone()?))
            .status()?;
        return Ok(status
            .code()
            .and_then(|code| u8::try_from(code).ok())
            .unwrap_or(1));
    }
    Ok(match (said, child.as_mut()) {
        // 说了退出码的：不等后台的进程退出（它可能留下来等后台任务、循环）。
        (Some(code), _) => code,
        // 没说退出码就关了控制线：自己起的那个是崩了、被杀了，照它的退出码，没有的算出错。
        (None, Some(child)) => child
            .wait()?
            .code()
            .and_then(|code| u8::try_from(code).ok())
            .filter(|code| *code != 0)
            .unwrap_or(1),
        (None, None) => 1,
    })
}

/// 办 `shellhook report`：脚本收到留着的进程发的信号以后调的。同一个终端留着的进程有要印的（后台任务结束了、被叫醒
/// 开了一轮、在等确认）就接上它，印出来、跟到了结。没有要印的、没有留着的进程，什么都不动，退出码 1。
///
/// `--pending` 只问有没有要印的（zsh 停在提示符上：有的才让出提示符）；`--above <行数>` 是提示符在光标那一行上面占了
/// 几行（zsh 先 `zle -I` 换到了下一行，是提示符的行数；fish 的光标还在提示符最后一行，是行数减一），有要印的才往上回
/// 这么多行、从那里往下清掉，在那里印。
pub fn report(parsed: &Parsed) -> ExitCode {
    use lm_engine::store::paths::{Env, Paths};
    let waiting = || {
        let paths = Paths::locate(&Env::current()).ok()?;
        let mark = super::terminal::Terminal::of(&paths, parsed.value("terminal")?)?.pending();
        mark.exists().then_some(mark)
    };
    let Some(mark) = waiting() else {
        return ExitCode::FAILURE;
    };
    if parsed.has("pending") {
        return ExitCode::SUCCESS;
    }
    let tty = terminal();
    let raw = tty.as_ref().and_then(|tty| Raw::enter(tty).ok());
    let Some(mut control) = attach(parsed, None) else {
        // 标记在、留着的进程却不在了（它被结束了）：标记是旧的，清掉。
        drop(std::fs::remove_file(mark));
        return ExitCode::FAILURE;
    };
    // 停在提示符上：往上擦掉提示符那几行，回报印在那里，脚本印完再把提示符画回来。
    if let (Some(mut tty), Some(rows)) = (
        tty.as_ref(),
        parsed
            .value("above")
            .and_then(|rows| rows.parse::<u16>().ok()),
    ) {
        use std::io::Write;
        let up = match rows {
            0 => String::new(),
            up => format!("\x1b[{up}A"),
        };
        drop(tty.write_all(format!("\r{up}\x1b[J").as_bytes()));
    }
    let said = wait(&mut control);
    drop(raw);
    match said {
        Ok((Some(code), _)) => ExitCode::from(code),
        _ => ExitCode::FAILURE,
    }
}

/// 留着的进程收下新接上的前台最多等多久：没回音的（它正在退出）照没有留着的办，自己起后台的进程。
const ACK: std::time::Duration = std::time::Duration::from_secs(2);

/// 同一个终端有留着的进程（上一句以后还在等后台任务、循环）：连上它的套接字，交这一句，等它说收下了，交回这条连接
/// （之后就是控制线）。没有终端的记号、没有留着的、它没回音的是 `None`。
///
/// `text` 是那句话；`None` 是印回报（不说话）。
fn attach(parsed: &Parsed, text: Option<&str>) -> Option<UnixStream> {
    use lm_engine::store::paths::{Env, Paths};
    let token = parsed.value("terminal")?;
    let paths = Paths::locate(&Env::current()).ok()?;
    let socket = super::terminal::socket(&paths, token, false)?;
    let mut control = UnixStream::connect(socket).ok()?;
    let hello = Hello {
        report: text.is_none(),
        redo: parsed.has("redo"),
        group: rustix::process::getpgrp().as_raw_nonzero().get(),
        leftover: parsed.value("leftover").unwrap_or_default().to_string(),
    };
    ToWorker::Hello(hello).write(&mut control).ok()?;
    if let Some(text) = text {
        ToWorker::Message(text.to_string())
            .write(&mut control)
            .ok()?;
    }
    control.set_read_timeout(Some(ACK)).ok()?;
    let acked = matches!(ToFront::read(&mut control), Ok(Some(ToFront::Ack)));
    control.set_read_timeout(None).ok()?;
    acked.then_some(control)
}

/// 起后台的进程：交回和它连着的控制线、它自己。
fn spawn(parsed: &Parsed, tty: Option<&File>) -> io::Result<(UnixStream, std::process::Child)> {
    let (control, control_far) = UnixStream::pair()?;
    // 后台进程的标准输入、标准输出是终端，标准错误先是控制线（它起来以后另存一份、再把标准错误接到标准输出上）。
    // 这个 `Command` 拿着控制线对面那一头，起完就放掉：不放的话后台进程退出了这里也等不到控制线关上。
    let mut command = Command::new(std::env::current_exe()?);
    command.args(["shellhook", "worker"]);
    for long in PASSED {
        if let Some(value) = parsed.value(long) {
            command.arg(format!("--{long}")).arg(value);
        }
    }
    for flag in ["redo", "reports"] {
        if parsed.has(flag) {
            command.arg(format!("--{flag}"));
        }
    }
    match tty {
        Some(tty) => command
            .stdin(Stdio::from(tty.try_clone()?))
            .stdout(Stdio::from(tty.try_clone()?)),
        None => command.stdin(Stdio::null()).stdout(Stdio::inherit()),
    };
    let child = command
        .stderr(Stdio::from(OwnedFd::from(control_far)))
        .spawn()?;
    Ok((control, child))
}

/// 等后台的进程说退出码（没说就关了控制线的是 `None`），和它要不要这一头接着起全屏界面。这期间被要求结束的（`SIGTERM`、`SIGHUP`：终端关了）当场回来：
/// 调的一方还原终端就走，后台的进程看到控制线关了自己收场。
fn wait(control: &mut UnixStream) -> io::Result<(Option<u8>, Option<Vec<String>>)> {
    let signals = Signals::listen();
    let code = None;
    let mut fullscreen = None;
    loop {
        let (told, ended) = {
            let mut fds = vec![PollFd::new(&*control, PollFlags::IN)];
            if let Some(signals) = &signals {
                fds.push(PollFd::new(&signals.ended, PollFlags::IN));
            }
            match poll(&mut fds, None) {
                Ok(_) => {}
                // 被信号打断：再等一圈。
                Err(rustix::io::Errno::INTR) => continue,
                Err(error) => return Err(error.into()),
            }
            let ready = |at: usize| fds.get(at).is_some_and(|fd| !fd.revents().is_empty());
            (ready(0), ready(1))
        };
        if ended {
            return Ok((Some(128 + 15), None));
        }
        if told {
            match ToFront::read(control) {
                // 说了退出码就走：不等它关控制线（它可能留下来）。
                Ok(Some(ToFront::Exit(exit))) => return Ok((Some(exit), fullscreen)),
                Ok(Some(ToFront::Fullscreen(args))) => fullscreen = Some(args),
                // 办完了：下一格就是退出码。
                Ok(Some(ToFront::Done | ToFront::Ack)) => {}
                Ok(None) | Err(_) => return Ok((code, fullscreen)),
            }
        }
    }
}

/// 这个进程的终端，能读能写的一份；没有终端的是 `None`。
///
/// 先照标准错误、标准输出认出终端的设备名（`/dev/pts/3`、`/dev/ttys003`）再开，认不出的才开 `/dev/tty`：macOS 的 `poll`
/// 等不了 `/dev/tty` 这个设备（当场回「这个描述符不对」），只等得了真的终端设备。shell 的脚本调过来时这两样都接着终端。
fn terminal() -> Option<File> {
    use std::os::unix::ffi::OsStrExt;
    let open = |path: &std::ffi::OsStr| File::options().read(true).write(true).open(path).ok();
    let named = |fd: rustix::fd::BorrowedFd<'_>| {
        let name = rustix::termios::ttyname(fd, Vec::new()).ok()?;
        open(std::ffi::OsStr::from_bytes(name.to_bytes()))
    };
    named(rustix::stdio::stderr())
        .or_else(|| named(rustix::stdio::stdout()))
        .or_else(|| open(std::ffi::OsStr::new("/dev/tty")))
}

/// 终端切到「只关输入处理」的原始模式；放掉时还原。
struct Raw {
    tty: File,
    saved: Termios,
}

impl Raw {
    fn enter(tty: &File) -> io::Result<Raw> {
        let saved = tcgetattr(tty)?;
        let mut raw = saved.clone();
        raw.local_modes
            .remove(LocalModes::ICANON | LocalModes::ECHO | LocalModes::ISIG | LocalModes::IEXTEN);
        raw.input_modes
            .remove(InputModes::IXON | InputModes::ICRNL | InputModes::INLCR | InputModes::IGNCR);
        // 输出这一边照旧处理：换行补成回车换行（zsh 的行编辑器里这一项可能是关着的，这里明着打开，收工时照原样还回去）。
        raw.output_modes
            .insert(OutputModes::OPOST | OutputModes::ONLCR);
        raw.special_codes[SpecialCodeIndex::VMIN] = 1;
        raw.special_codes[SpecialCodeIndex::VTIME] = 0;
        tcsetattr(tty, OptionalActions::Now, &raw)?;
        let kept = Raw {
            tty: tty.try_clone()?,
            saved,
        };
        // 发行版出错直接中止，走不到 `Drop`：出错时也先还原。
        if let Ok(tty) = tty.try_clone() {
            let saved = kept.saved.clone();
            let before = std::panic::take_hook();
            std::panic::set_hook(Box::new(move |info| {
                let _restored = tcsetattr(&tty, OptionalActions::Now, &saved);
                before(info);
            }));
        }
        Ok(kept)
    }
}

impl Drop for Raw {
    fn drop(&mut self) {
        // 用 `Now`，不清终端的输入缓冲：办完以后敲的字留给 shell（坑 P8）。
        let _restored = tcsetattr(&self.tty, OptionalActions::Now, &self.saved);
    }
}

/// 这个进程等的信号：写进一对套接字，和控制线一起等（不轮询）。
struct Signals {
    /// 被要求结束（`SIGTERM`、`SIGHUP`）。
    ended: UnixStream,
    /// `SIGINT` 写进去的那一对的这一头：留着不读（见 [`Signals::listen`]）。
    _interrupted: UnixStream,
}

impl Signals {
    /// 装上；装不上的照常等，只是收不到。`SIGINT` 也接住、什么都不做：没有终端的时候 `Ctrl+C` 是发给整个进程组的信号，
    /// 后台的进程照它打断这一轮，这一头得等到它收完场。
    fn listen() -> Option<Signals> {
        use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM};
        use signal_hook::low_level::pipe::register;
        let (ended, on_end) = UnixStream::pair().ok()?;
        let (interrupted, on_interrupt) = UnixStream::pair().ok()?;
        register(SIGTERM, on_end.try_clone().ok()?).ok()?;
        register(SIGHUP, on_end).ok()?;
        register(SIGINT, on_interrupt).ok()?;
        Some(Signals {
            ended,
            _interrupted: interrupted,
        })
    }
}
