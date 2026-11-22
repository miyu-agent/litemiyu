//! `shellhook worker`：后台的那个进程，`say`（`say.rs`）起的，不是给人敲的。它拿着会话：载入这个终端的会话（没有的新开）、
//! 说那一句、跟到这一轮了结。它不碰终端：标准输入是前台转交来的按键，标准输出、标准错误是要印的字，另有一条控制线
//! （`wire.rs`，起来时在标准错误的位置上）。
//!
//! 施工 SH-1 的最小版本：回答用 `ask` 现成的文字输出、流式地印；没有抽屉，要人确认的高危命令照 `ask` 不加 `--yes` 时那样
//! 跳过；这一轮派出去的后台命令、子代理像 `ask` 一样等它们报完；办完就退出，不留下来。画法、抽屉、留下来是后面几步。

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::process::ExitCode;
use std::sync::{Arc, Mutex, mpsc};
use std::time::Duration;

use lm_engine::store::paths::{Env, Paths};

use super::terminal::{self, Terminal};
use super::typed::Typed;
use super::wire::{ToFront, ToWorker};
use crate::cli::ask::{Event, Follow, Out, Output, Plan, Sink, Text, drive_with, model_ready};
use crate::cli::misuse;
use crate::cli::parse::Parsed;
use crate::cli::words::Words;

/// 前台说了不再转交按键以后，最多等多久把路上的按键收齐。
const DRAIN: Duration = Duration::from_secs(2);

/// `Ctrl+C` 在原始模式的终端里敲出来的那个字节。
const INTERRUPT: u8 = 3;

/// 办 `shellhook worker`。
pub fn run(parsed: &Parsed) -> ExitCode {
    // 控制线在标准错误的位置上：另存一份，再把标准错误接到数据线（标准输出）上，之后报错的字照常印得出来。
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
    if let Err(code) = model_ready(words) {
        return code;
    }
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
    let plan = Plan {
        session: terminal
            .as_ref()
            .and_then(|terminal| terminal.session(&paths)),
        message: text,
        yes: false,
        preset: None,
        rescue: None,
        timeout: None,
        max_steps: None,
        model: None,
        elsewhere: false,
        // 人在两句话之间可能 `cd` 过：这一句在 shell 现在的目录里办。
        here: true,
    };
    let typed = Arc::new(Mutex::new(Typed::default()));
    let (drained, all_typed) = mpsc::channel();
    let mut output = Kept {
        // 印的字经前台的进程到终端：照接着终端办（控制序列洗掉、上色）。
        text: Text::relayed(words),
        session: None,
    };
    let ended = drive_with(plan, words, &mut output, |events| {
        keys(events.clone(), Arc::clone(&typed), drained);
        orders(events.clone(), control);
    });
    if let (Some(terminal), Some(id)) = (&terminal, &output.session)
        && let Err(error) = terminal.remember(id)
    {
        let reason = words.why(&error.to_string());
        words.warn(
            &words.say("shellhook.not_remembered", &[("reason", &reason)]),
            None,
        );
    }
    // 办完了：请前台不再转交按键，等路上的收齐，没交出去的字写给脚本放回命令行（坑 S13）。
    let mut control = control;
    if ToFront::Done.write(&mut control).is_ok() {
        let _waited = all_typed.recv_timeout(DRAIN);
    }
    if let Some(path) = parsed.value("leftover") {
        let left = typed.lock().map(|typed| typed.text().to_string());
        if let Ok(left) = left
            && !left.is_empty()
        {
            drop(std::fs::write(path, left));
        }
    }
    ended.exit
}

/// 读前台转交来的按键（标准输入）：`Ctrl+C` 打断这一轮，别的收成提前敲的字；读到头了说一声。
fn keys(events: mpsc::Sender<Event>, typed: Arc<Mutex<Typed>>, drained: mpsc::Sender<()>) {
    let spawned = std::thread::Builder::new()
        .name("keys".into())
        .spawn(move || {
            let mut input = std::io::stdin().lock();
            let mut buffer = [0u8; 4096];
            loop {
                let count = match input.read(&mut buffer) {
                    Ok(0) | Err(_) => break,
                    Ok(count) => count,
                };
                for piece in buffer[..count].split_inclusive(|byte| *byte == INTERRUPT) {
                    let (text, interrupted) = match piece.split_last() {
                        Some((&INTERRUPT, text)) => (text, true),
                        _ => (piece, false),
                    };
                    if let Ok(mut typed) = typed.lock() {
                        typed.push(text);
                    }
                    // 主线程不等了的（已经在收尾）交不进，不算错。
                    if interrupted && events.send(Event::Signal(false)).is_err() {}
                }
            }
            let _told = drained.send(());
        });
    if spawned.is_err() {}
}

/// 读控制线上前台交来的：终端的大小先收下不用（SH-2 的画法才用）；控制线关了就是前台不在了（终端关了、被杀了），照
/// `SIGTERM` 收场。
fn orders(events: mpsc::Sender<Event>, control: &UnixStream) {
    let Ok(mut control) = control.try_clone() else {
        return;
    };
    let spawned = std::thread::Builder::new()
        .name("orders".into())
        .spawn(move || {
            while let Ok(Some(_)) = ToWorker::read(&mut control) {}
            drop(events.send(Event::Signal(true)));
        });
    if spawned.is_err() {}
}

/// 文字输出外面套一层：记下开好的是哪个会话（办完记给这个终端）。
struct Kept<'w> {
    text: Text<'w>,
    session: Option<String>,
}

impl Sink for Kept<'_> {
    fn put(&mut self, out: Out<'_>) {
        self.text.put(out);
    }
}

impl Output for Kept<'_> {
    fn session(&mut self, id: &str, new: bool) {
        self.session = Some(id.to_string());
        self.text.session(id, new);
    }

    fn waiting(&mut self, agents: usize, commands: usize) {
        self.text.waiting(agents, commands);
    }

    fn finish(&mut self, ending: &crate::cli::ask::Ending, follow: &Follow, session: Option<&str>) {
        self.text.finish(ending, follow, session);
    }
}
