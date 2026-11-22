//! `shellhook worker`：后台的那个进程，`say`（`say.rs`）起的，不是给人敲的。它拿着会话：载入这个终端的会话（没有的新开）、
//! 说那一句、跟到这一轮了结。它的标准输入、标准输出就是终端这个设备（原始模式是前台进的），另有一条控制线（`wire.rs`，
//! 起来时在标准错误的位置上）。读按键用全屏界面的那条线程（`reader.rs`）。
//!
//! 现在的最小版本：回答用 `ask` 现成的文字输出、流式地印；没有抽屉，要人确认的高危命令照 `ask` 不加 `--yes` 时那样
//! 跳过；这一轮派出去的后台命令、子代理像 `ask` 一样等它们报完；办完就退出，不留下来。画法、抽屉、留下来是后面几步。

use std::io::{IsTerminal, Write};
use std::os::unix::net::UnixStream;
use std::process::ExitCode;
use std::sync::{Arc, Mutex, mpsc};

use lm_engine::store::paths::{Env, Paths};

use super::terminal::{self, Terminal};
use super::wire::{ToFront, ToWorker};
use crate::cli::ask::{Event, Follow, Out, Output, Plan, Sink, Text, drive_with, model_ready};
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
    let typed = Arc::new(Mutex::new(String::new()));
    let mut reader = None;
    let mut output = Kept {
        // 标准输出、标准错误都是终端：回答、思考里的终端控制序列照 `ask` 接着终端时那样洗掉（审计 #064）。
        text: Text::new(words),
        session: None,
    };
    let ended = drive_with(plan, words, &mut output, |events| {
        reader = keys(events.clone(), Arc::clone(&typed));
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
    // 办完了：先让读按键的线程停下（它停好了才回，读走的按键都收下了，坑 P8），之后敲的字留在终端的缓冲里给 shell；
    // 没交出去的字写给脚本放回命令行（坑 S13）。
    if let Some(reader) = &reader {
        reader.pause();
    }
    let mut control = control;
    // 写不进的（前台不在了）没处可报。
    drop(ToFront::Done.write(&mut control));
    if let Some(path) = parsed.value("leftover") {
        let left = typed.lock().map(|typed| typed.clone());
        if let Ok(left) = left
            && !left.is_empty()
        {
            drop(std::fs::write(path, left));
        }
    }
    ended.exit
}

/// 起读按键的线程（全屏界面的那一条）：`Ctrl+C` 打断这一轮，别的字收成提前敲的一行，退格删一个；方向键这类不算，回车
/// 不算（提前敲的命令不替人执行）。终端读不了了（终端关了）照 `SIGTERM` 收场。标准输入不是终端的不起。
///
/// 这是最小的版本：做了答话时的输入行以后换成全屏界面的那个输入框。
fn keys(events: mpsc::Sender<Event>, typed: Arc<Mutex<String>>) -> Option<crate::reader::Reader> {
    use crossterm::event::{Event as Terminal, KeyCode, KeyEventKind, KeyModifiers};
    if !std::io::stdin().is_terminal() {
        return None;
    }
    crate::reader::spawn(move |got| {
        let key = match got {
            crate::reader::Got::Event(Ok(Terminal::Key(key))) => key,
            crate::reader::Got::Event(Ok(Terminal::Paste(text))) => {
                if let Ok(mut typed) = typed.lock() {
                    typed.push_str(&text);
                }
                return true;
            }
            // 终端读不了了：主线程不等了的交不进，线程照样结束。
            crate::reader::Got::Event(Err(_)) => return events.send(Event::Signal(true)).is_ok(),
            _ => return true,
        };
        if key.kind == KeyEventKind::Release {
            return true;
        }
        let plain = !key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
        match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                return events.send(Event::Signal(false)).is_ok();
            }
            KeyCode::Char(c) if plain => {
                if let Ok(mut typed) = typed.lock() {
                    typed.push(c);
                }
            }
            KeyCode::Backspace => {
                if let Ok(mut typed) = typed.lock() {
                    typed.pop();
                }
            }
            _ => {}
        }
        true
    })
    .ok()
}

/// 等控制线关上：那就是前台不在了（终端关了、被杀了），照 `SIGTERM` 收场。
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
