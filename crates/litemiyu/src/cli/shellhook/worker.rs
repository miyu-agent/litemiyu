//! `shellhook worker`：后台的那个进程，`say`（`say.rs`）起的，不是给人敲的。它拿着会话：认出这个终端的会话（没有的新开）、
//! 交给 `inline.rs` 去说那一句、跟到这一轮了结，办完记下这个终端的会话、把提前敲的字写给脚本。它的标准输入、标准输出就是
//! 终端这个设备（原始模式是前台进的），另有一条控制线（`wire.rs`，起来时在标准错误的位置上）。

use std::io::Write;
use std::os::unix::net::UnixStream;
use std::process::ExitCode;

use lm_engine::store::paths::{Env, Paths};

use super::inline::{Ask, Linger, Session};
use super::terminal::{self, Terminal};
use super::wire::{ToFront, ToWorker};
use crate::cli::ask::model_ready;
use crate::cli::misuse;
use crate::cli::parse::Parsed;
use crate::cli::words::Words;

/// 办 `shellhook worker`。
pub fn run(parsed: &Parsed) -> ExitCode {
    // 控制线在标准错误的位置上：另存一份，再把标准错误接到标准输出（终端）上，之后报错的字照常印得出来。
    let Ok(mut control) = rustix::io::dup(rustix::stdio::stderr()).map(UnixStream::from) else {
        return ExitCode::from(misuse::USAGE);
    };
    if rustix::stdio::dup2_stderr(rustix::stdio::stdout()).is_err() {
        return ExitCode::from(misuse::USAGE);
    }
    // 头一格是那句话；不是的（有人手敲了这条命令）不办。
    let Ok(Some(ToWorker::Message(text))) = ToWorker::read(&mut control) else {
        return ExitCode::from(misuse::USAGE);
    };
    let words = Words::current();
    let first = Ask {
        text,
        report: false,
        redo: parsed.has("redo"),
        leftover: parsed.value("leftover").map(str::to_string),
        control,
        group: None,
    };
    said(parsed, first, &words);
    ExitCode::SUCCESS
}

/// 让前台走：说办完了、交退出码（开这一份状态以前就办不下去的那几样用；办下去了的由 `inline.rs` 交）。
fn dismiss(control: &mut UnixStream, code: u8) {
    drop(std::io::stdout().flush());
    drop(ToFront::Done.write(control));
    drop(ToFront::Exit(code).write(control));
}

/// 办那一件，办完（连同留下来以后接着办的几件）才回来。
fn said(parsed: &Parsed, mut first: Ask, words: &Words) {
    let paths = match Paths::locate(&Env::current()) {
        Ok(paths) => paths,
        Err(error) => {
            words.error(&words.why(&error.to_string()), None);
            return dismiss(&mut first.control, 1);
        }
    };
    terminal::sweep(&paths);
    let token = parsed.value("terminal");
    let terminal = token.and_then(|token| Terminal::of(&paths, token));
    let session = terminal
        .as_ref()
        .and_then(|terminal| terminal.session(&paths));
    // `/config`、`/tui`：请前台在这个进程退出以后起全屏界面（这个进程不开会话，全屏界面打开它时不会撞上）。
    if let Some(args) = super::inline::fullscreen(&first.text, session.as_deref()) {
        drop(ToFront::Fullscreen(args).write(&mut first.control));
        return dismiss(&mut first.control, 0);
    }
    // 说话要先有能用的模型；斜杠命令不用（`/help`、`/status` 没配模型也看得了）。
    if !first.text.trim_start().starts_with('/')
        && let Err(code) = model_ready(words)
    {
        return dismiss(&mut first.control, code);
    }
    // 有终端的记号、放得下套接字的才留得下来（等同一个终端的下一句话）。
    let linger = token
        .and_then(|token| terminal::socket(&paths, token, true))
        .map(|socket| Linger {
            socket,
            shell: parsed.value("shell-pid").and_then(|pid| pid.parse().ok()),
            // 脚本带了 `--reports` 才给 shell 发信号（它接了 `SIGUSR1`）。
            notify: terminal
                .as_ref()
                .filter(|_| parsed.has("reports"))
                .map(Terminal::pending),
        });
    let mut kept = |session: &Session| {
        let saved = match (&terminal, session) {
            (Some(terminal), Session::Is(id)) => terminal.remember(id),
            (Some(terminal), Session::None) => terminal.forget(),
            _ => Ok(()),
        };
        if let Err(error) = saved {
            let reason = words.why(&error.to_string());
            words.warn(
                &words.say("shellhook.not_remembered", &[("reason", &reason)]),
                None,
            );
        }
    };
    // 配置读不懂的：这一件办不了。控制线已经交给了 `inline::run`，它没交退出码就回来，前台照后台的进程没了办。
    if let Err(why) = super::inline::run(first, session, linger, words, &mut kept) {
        let reason = words.why(&why);
        words.error(
            &words.say("config_unreadable", &[("reason", &reason)]),
            None,
        );
    }
}
