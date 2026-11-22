//! `shellhook say`：前台的那个进程。shell 的脚本把那句话从标准输入交来，这里起一个后台的进程（`worker.rs`）去办，自己只
//! 管终端：把敲的字转交过去、把要印的字印出来、终端变了大小说一声。它一退出，shell 就回到提示符。
//!
//! 为什么分两个进程（施工线 shellhook 第三节「后台、循环和到点继续」）：后台命令、循环、到点继续都活在拿着会话的那个进程里，
//! 一轮结束以后提示符要马上回来、这些又不能跟着没了，所以拿着会话的进程要能比这一句活得久。这个仓库禁用 `unsafe`，不能
//! `fork`，后台的进程是再启动一次自己（`shellhook worker`）。施工 SH-1 里后台的进程还不留下来，这一句办完就一起退出。
//!
//! 终端的规矩都在这一头：
//!
//! - 那句话是从管道交来的，读完以后终端另开 `/dev/tty` 来用（坑 P1）。
//! - 只关掉输入这一边的处理（不回显、不攒整行、`Ctrl+C` 是一个字节不是信号），输出照旧把换行补成回车换行：后台的进程印的是
//!   平常的字。
//! - 任何退出的路上先把终端还原（发行版是 `panic = "abort"`，另装了一个出错时还原的钩子）。
//! - 后台的进程说办完了（`D`）以后不再读终端：之后敲的字留在终端自己的缓冲里，shell 回到提示符时自己读得到，不丢（坑 P8）。

use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::process::{Command, ExitCode, Stdio};

use rustix::event::{PollFd, PollFlags, poll};
use rustix::termios::{
    InputModes, LocalModes, OptionalActions, OutputModes, SpecialCodeIndex, Termios, tcgetattr,
    tcgetwinsize, tcsetattr,
};

use super::wire::{ToFront, ToWorker};
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
    let code = match relay(parsed, &text) {
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

/// 起后台的进程，转交到它办完，交回退出码。
fn relay(parsed: &Parsed, text: &str) -> io::Result<u8> {
    let (data, data_far) = UnixStream::pair()?;
    let (mut control, control_far) = UnixStream::pair()?;
    let mut child = {
        // 后台进程的标准输入、标准输出是数据线，标准错误先是控制线（它起来以后另存一份、再把标准错误接到数据线上）。
        // 这个 `Command` 拿着对面那几头，起完就放掉：不放的话后台进程退出了这里也等不到数据线关上。
        let mut command = Command::new(std::env::current_exe()?);
        command.args(["shellhook", "worker"]);
        for long in PASSED {
            if let Some(value) = parsed.value(long) {
                command.arg(format!("--{long}")).arg(value);
            }
        }
        if parsed.has("redo") {
            command.arg("--redo");
        }
        command
            .stdin(Stdio::from(OwnedFd::from(data_far.try_clone()?)))
            .stdout(Stdio::from(OwnedFd::from(data_far)))
            .stderr(Stdio::from(OwnedFd::from(control_far)))
            .spawn()?
    };
    // 终端：那句话是从管道交来的，标准输入不是它（坑 P1）。没有终端的（测试、被别的程序调）照样办，只是没有按键可转交。
    let tty = File::options().read(true).write(true).open("/dev/tty").ok();
    let raw = tty.as_ref().and_then(|tty| Raw::enter(tty).ok());
    ToWorker::Message(text.to_string()).write(&mut control)?;
    if let Some(size) = tty.as_ref().and_then(size) {
        size.write(&mut control)?;
    }
    let outcome = Relay {
        tty: tty.as_ref(),
        data,
        control: Some(control),
        typing: tty.is_some(),
    }
    .run();
    drop(raw);
    let status = child.wait()?;
    Ok(match outcome? {
        Some(code) => code,
        // 后台的进程没说退出码就没了（崩了、被杀了）：照它自己的退出码，没有的算出错。
        None => status
            .code()
            .and_then(|code| u8::try_from(code).ok())
            .filter(|code| *code != 0)
            .unwrap_or(1),
    })
}

/// 终端现在多大。
fn size(tty: &File) -> Option<ToWorker> {
    let size = tcgetwinsize(tty).ok()?;
    Some(ToWorker::Size(size.ws_col, size.ws_row))
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

/// 转交的那一圈。
struct Relay<'a> {
    tty: Option<&'a File>,
    data: UnixStream,
    /// 控制线；对面关了以后不再等它。
    control: Option<UnixStream>,
    /// 还在把终端敲的字转交过去。
    typing: bool,
}

/// 这一圈里哪几样有动静。
#[derive(Default)]
struct Ready {
    printed: bool,
    told: bool,
    resized: bool,
    ended: bool,
    typed: bool,
}

impl Relay<'_> {
    /// 等到有动静。
    fn wait(&self, signals: Option<&Signals>) -> io::Result<Ready> {
        let mut fds = vec![PollFd::new(&self.data, PollFlags::IN)];
        let control = self.control.as_ref().map(|control| {
            fds.push(PollFd::new(control, PollFlags::IN));
            fds.len() - 1
        });
        let signal = signals.map(|signals| {
            fds.push(PollFd::new(&signals.resized, PollFlags::IN));
            fds.push(PollFd::new(&signals.ended, PollFlags::IN));
            fds.len() - 2
        });
        let keys = self.tty.filter(|_| self.typing).map(|tty| {
            fds.push(PollFd::new(tty, PollFlags::IN));
            fds.len() - 1
        });
        match poll(&mut fds, None) {
            Ok(_) => {}
            // 被信号打断：照没动静再等一圈。
            Err(rustix::io::Errno::INTR) => return Ok(Ready::default()),
            Err(error) => return Err(error.into()),
        }
        let ready = |at: Option<usize>| {
            at.and_then(|at| fds.get(at)).is_some_and(|fd| {
                fd.revents()
                    .intersects(PollFlags::IN | PollFlags::HUP | PollFlags::ERR)
            })
        };
        Ok(Ready {
            printed: ready(Some(0)),
            told: ready(control),
            resized: ready(signal),
            ended: ready(signal.map(|at| at + 1)),
            typed: ready(keys),
        })
    }

    /// 转到后台的进程把数据线关上；交回它说的退出码（没说的是 `None`）。
    fn run(mut self) -> io::Result<Option<u8>> {
        let signals = Signals::listen();
        let mut code = None;
        let mut buffer = [0u8; 8192];
        if self.tty.is_none() {
            self.stop_typing();
        }
        loop {
            let ready = self.wait(signals.as_ref())?;
            // 被要求结束（`SIGTERM`、`SIGHUP`：终端关了）：还原终端就走，后台的进程看到控制线关了自己收场。
            if ready.ended {
                return Ok(Some(128 + 15));
            }
            if ready.resized {
                if let Some(signals) = &signals {
                    signals.drain();
                }
                if let (Some(size), Some(control)) = (self.tty.and_then(size), &mut self.control) {
                    // 后台的进程正在退出的写不进，不算错。
                    drop(size.write(control));
                }
            }
            if ready.told
                && let Some(control) = &mut self.control
            {
                match ToFront::read(control) {
                    Ok(Some(ToFront::Done)) => self.stop_typing(),
                    Ok(Some(ToFront::Exit(exit))) => code = Some(exit),
                    // 控制线关了：不再等它，等数据线也关上（剩下要印的印完）。
                    Ok(None) | Err(_) => {
                        self.control = None;
                        self.stop_typing();
                    }
                }
            }
            if ready.printed {
                match self.data.read(&mut buffer) {
                    Ok(0) => return Ok(code.or_else(|| self.last_word())),
                    Ok(count) => self.print(&buffer[..count]),
                    Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                    Err(_) => return Ok(code.or_else(|| self.last_word())),
                }
            }
            if ready.typed
                && let Some(mut tty) = self.tty
            {
                match tty.read(&mut buffer) {
                    // 终端没了：不再读它。
                    Ok(0) | Err(_) => self.stop_typing(),
                    Ok(count) => {
                        if self.data.write_all(&buffer[..count]).is_err() {
                            self.stop_typing();
                        }
                    }
                }
            }
        }
    }

    /// 不再转交按键：数据线自己写的那一头关上，后台的进程读到头就知道提前敲的字都到了。
    fn stop_typing(&mut self) {
        self.typing = false;
        // 关过了的再关一次不算错。
        drop(self.data.shutdown(std::net::Shutdown::Write));
    }

    /// 数据线关了以后控制线上可能还有一格没读的退出码。
    fn last_word(&mut self) -> Option<u8> {
        let control = self.control.as_mut()?;
        drop(control.set_nonblocking(true));
        loop {
            match ToFront::read(control) {
                Ok(Some(ToFront::Exit(code))) => return Some(code),
                Ok(Some(ToFront::Done)) => {}
                Ok(None) | Err(_) => return None,
            }
        }
    }

    /// 印一截：有终端的印在终端上，没有的印在标准输出上。写不进的没处可报。
    fn print(&self, bytes: &[u8]) {
        match self.tty {
            Some(mut tty) => drop(tty.write_all(bytes)),
            None => {
                let mut out = io::stdout().lock();
                drop(out.write_all(bytes).and_then(|()| out.flush()));
            }
        }
    }
}

/// 这个进程等的几种信号：各写进一对套接字，转交的那一圈一起等（不轮询）。
struct Signals {
    /// 终端变了大小（`SIGWINCH`）。
    resized: UnixStream,
    /// 被要求结束（`SIGTERM`、`SIGHUP`）。
    ended: UnixStream,
    /// `SIGINT` 写进去的那一对的这一头：留着不读（见 [`Signals::listen`]）。
    _interrupted: UnixStream,
}

impl Signals {
    /// 装上；装不上的照常转交，只是收不到。`SIGINT` 也接住、什么都不做：没有终端的时候 `Ctrl+C` 是发给整个进程组的信号，
    /// 后台的进程照它打断这一轮，这一头得等到它收完场。
    fn listen() -> Option<Signals> {
        use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM, SIGWINCH};
        use signal_hook::low_level::pipe::register;
        let (resized, on_resize) = UnixStream::pair().ok()?;
        let (ended, on_end) = UnixStream::pair().ok()?;
        let (interrupted, on_interrupt) = UnixStream::pair().ok()?;
        resized.set_nonblocking(true).ok()?;
        register(SIGWINCH, on_resize).ok()?;
        register(SIGTERM, on_end.try_clone().ok()?).ok()?;
        register(SIGHUP, on_end).ok()?;
        register(SIGINT, on_interrupt).ok()?;
        Some(Signals {
            resized,
            ended,
            _interrupted: interrupted,
        })
    }

    /// 变大小的那几下读掉。
    fn drain(&self) {
        let mut buffer = [0u8; 64];
        while matches!((&self.resized).read(&mut buffer), Ok(count) if count > 0) {}
    }
}
