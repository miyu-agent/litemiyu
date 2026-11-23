//! `shellhook worker`：后台的那个进程，`say`（`say.rs`）起的，不是给人敲的。它拿着会话：认出这个终端的会话（没有的新开）、
//! 交给 `inline.rs` 去说那一句、跟到这一轮了结，办完记下这个终端的会话、把提前敲的字写给脚本。它的标准输入、标准输出就是
//! 终端这个设备（原始模式是前台进的），另有一条控制线（`wire.rs`，起来时在标准错误的位置上）。

use std::io::Write;
use std::os::unix::net::UnixStream;
use std::process::ExitCode;

use lm_engine::store::paths::{Env, Paths};

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
    let code = said(parsed, text, &words, &control);
    // 写不进的（前台不在了）没处可报。
    drop(std::io::stdout().flush());
    drop(ToFront::Exit(code).write(&mut control));
    ExitCode::from(code)
}

/// 说那一句，跟到了结，交回退出码。
fn said(parsed: &Parsed, text: String, words: &Words, control: &UnixStream) -> u8 {
    let paths = match Paths::locate(&Env::current()) {
        Ok(paths) => paths,
        Err(error) => {
            words.error(&words.why(&error.to_string()), None);
            return 1;
        }
    };
    terminal::sweep(&paths);
    let terminal = parsed
        .value("terminal")
        .and_then(|token| Terminal::of(&paths, token));
    let session = terminal
        .as_ref()
        .and_then(|terminal| terminal.session(&paths));
    // `/config`、`/tui`：请前台在这个进程退出以后起全屏界面（这个进程不开会话，全屏界面打开它时不会撞上）。
    if let Some(args) = super::inline::fullscreen(&text, session.as_deref()) {
        let mut control = control;
        drop(ToFront::Fullscreen(args).write(&mut control));
        drop(ToFront::Done.write(&mut control));
        return 0;
    }
    // 说话要先有能用的模型；斜杠命令不用（`/help`、`/status` 没配模型也看得了）。
    if !text.trim_start().starts_with('/')
        && let Err(code) = model_ready(words)
    {
        return code;
    }
    let outcome = match super::inline::run((&text, parsed.has("redo")), session, control, words) {
        Ok(outcome) => outcome,
        Err(why) => {
            let reason = words.why(&why);
            words.error(
                &words.say("config_unreadable", &[("reason", &reason)]),
                None,
            );
            return 1;
        }
    };
    let kept = match (&terminal, &outcome.session) {
        (Some(terminal), super::inline::Session::Is(id)) => terminal.remember(id),
        (Some(terminal), super::inline::Session::None) => terminal.forget(),
        _ => Ok(()),
    };
    if let Err(error) = kept {
        let reason = words.why(&error.to_string());
        words.warn(
            &words.say("shellhook.not_remembered", &[("reason", &reason)]),
            None,
        );
    }
    // 办完了（读按键的线程已经停下）：没交出去的字写给脚本放回命令行（坑 S13）。
    let mut control = control;
    // 写不进的（前台不在了）没处可报。
    drop(ToFront::Done.write(&mut control));
    if let Some(path) = parsed.value("leftover")
        && !outcome.typed.is_empty()
    {
        drop(std::fs::write(path, &outcome.typed));
        // `/edit` 放回来的上一句：旁边放一个标记，脚本记下「在编辑」，下一次回车带 `--redo`（坑 C4）。
        if outcome.editing {
            drop(std::fs::write(format!("{path}.redo"), ""));
        }
    }
    outcome.code
}
