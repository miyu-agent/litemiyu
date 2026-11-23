//! 在 shell 里跑一轮（施工线 shellhook，SH-2）：后台的进程里起全屏界面的那一份状态（[`App`]），不画整屏，只把「这一句
//! 以后的正文」照全屏时间线的样子排成行，交给 `paint.rs` 印在光标下面。
//!
//! 引擎的消息照全屏交给 `App::core`，所以正文、任务、抽屉、输入框都是全屏的那一套；这里只管几件事：说那一句、每一帧
//! 把排好的行分成「定下来的」（印进滚动历史）和「还会变的」（留在活动区，最多终端高减一行）、把按键分给抽屉或者输入框、
//! 认出了结。
//!
//! 活动区从上到下：还会变的那几行正文；空一行；开着的抽屉（一个圆角框，最多占终端一半高），没有抽屉的时候是排着的插话
//! 和输入那几行。AI 一开始答输入那一行就在、光标亮着停在那里（坑 P9）。
//!
//! 现在还没有的：斜杠命令的框、留下来；kitty 键盘协议没开，`Shift+回车`、`Ctrl+回车` 认不出来（换行用 `Alt+回车`、`Ctrl+J`）。

use std::io::{self, IsTerminal, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};

use super::paint::{self, Painter};
use super::wire::{ToFront, ToWorker};
use crate::app::App;
use crate::app::inline::{Laying, Shown, Slash};
use crate::cli::words::Words;
use crate::config::Config;
use crate::core::{Command, Core, Update};
use crate::incoming::Incoming;
use crate::reader::{self, Got};

/// 收尾时等会话停下（日志落好盘、放锁）最多多久：停不下的（卡在写盘上）不一直等。
const CLOSING: Duration = Duration::from_secs(30);

/// 过一道栅栏（引擎把交给它的都办完了）最多等多久。
const BARRIER: Duration = Duration::from_secs(30);

/// 被打断的退出码（同 `ask`）。
const INTERRUPTED: u8 = 3;

/// 一个抽屉开着满这么久才画（坑 P7）：接上会话时补发来的、以前问过又答了的，开一下马上收起，画出来会一闪。
const DRAWER_SETTLE: Duration = Duration::from_millis(150);

/// 留着的时候多久看一眼 shell 还在不在。
const SHELL_CHECK: Duration = Duration::from_secs(5);

/// 新接上的前台交完它那几样最多等多久。
const HELLO: Duration = Duration::from_secs(5);

/// 主循环等的东西。
enum In {
    /// 终端的事件、引擎的消息（同全屏的主循环）。
    App(Incoming),
    /// 第几个前台的进程不在了（控制线关了：终端关了、它被杀了，或者它收到退出码走了）。
    Gone(u64),
    /// 同一个终端又来了一个前台（再说一句、印回报）。
    Front(Box<Ask>),
    /// 收到的信号：`SIGINT` 是假（没有进原始模式的时候 `Ctrl+C` 是信号，照按了一下 `Ctrl+C` 办），`SIGTERM`、`SIGHUP`
    /// 是真（被要求结束、终端关了：停下会话退出，不留）。
    Signal(bool),
}

/// 一个前台交来的一件事。
pub struct Ask {
    /// 那句话；印回报的是空的。
    pub text: String,
    /// 是印回报：不说话，把上一次印到的地方以后的正文印出来、跟到了结。
    pub report: bool,
    /// 那句话是 `/edit` 改过的上一句。
    pub redo: bool,
    /// 提前敲的字写在哪。
    pub leftover: Option<String>,
    /// 和这个前台连着的控制线。
    pub control: UnixStream,
    /// 这个前台的进程组（接上留下来的进程时给的）：挪进去才读得了终端。头一个前台起的这个进程，本来就在一起。
    pub group: Option<i32>,
}

/// 留下来要的两样。
pub struct Linger {
    /// 等同一个终端的下一个前台的套接字放在哪。
    pub socket: PathBuf,
    /// shell 的进程号：它不在了就不留了。
    pub shell: Option<i32>,
    /// 留着的时候有了新的东西就放上这个标记、给 shell 发 `SIGUSR1`（脚本收到跑 `shellhook report`）。脚本说了它接这个
    /// 信号的才有：没接的 shell 收到 `SIGUSR1` 会被系统直接结束，绝不能发。
    pub notify: Option<PathBuf>,
}

/// 办完以后这个终端的会话。
pub enum Session {
    /// 是这一个（说了话的、`/sessions` 换过去的）。
    Is(String),
    /// 没有了（`/new`、还没说过话）：下一句新开。
    None,
    /// 照原来记的（要接着说的会话没开成，什么都没变）。
    Kept,
}

/// 那一句要进全屏界面办的：交回起全屏界面用的参数（`/config` 是配置界面；`/tui` 是打开这个终端的会话 `session`，还没
/// 说过话的就是新的）。别的是 `None`。只看命令的名字：全屏界面自己起来、自己读配置，这里不起引擎、不开会话。
pub fn fullscreen(text: &str, session: Option<&str>) -> Option<Vec<String>> {
    let words = |list: &[&str]| Some(list.iter().map(|word| (*word).to_string()).collect());
    match (text.trim(), session) {
        ("/config", _) => words(&["config"]),
        ("/tui", Some(id)) => words(&["-s", id]),
        ("/tui", None) => words(&[]),
        _ => None,
    }
}

/// 办 `first` 这一件；`session` 是这个终端接着说的会话，没有的新开。办完一件就照 `kept` 记下这个终端的会话。
///
/// 给了 `linger` 的能留下来：一件办完还有要等的（后台任务、循环、到点继续），这个进程不退出，在 `linger.socket` 上等同
/// 一个终端的下一个前台，接上再办（施工线 shellhook 第三节「后台、循环和到点继续」）。没有要等的、shell 不在了才停下会话
/// 交回。
///
/// # Errors
///
/// 配置读不懂（编进程序的字表坏了、`config.toml` 读不了）：原话。
pub fn run(
    first: Ask,
    session: Option<String>,
    linger: Option<Linger>,
    words: &Words,
    kept: &mut dyn FnMut(&Session),
) -> Result<(), String> {
    let env = |name: &str| std::env::var(name).ok();
    let table = crate::language::LanguageTable::builtin()?;
    let startup = crate::settings::startup::read(&table, env);
    let mut config = Config::load(&startup.language, true)?;
    crate::settings::prefs::apply(&mut config, |key| startup.tui(key));
    // 颜色照上次全屏界面记下的终端颜色（没有的照 16 色那一套）。这里不问终端：问了要等它回，回话晚到的会漏给 shell。
    let recorded = crate::theme::record::path()
        .as_deref()
        .and_then(crate::theme::record::read);
    crate::theme::start(crate::theme::Depth::detect(env), recorded);
    let (tx, rx) = mpsc::channel();
    let keys = tx.clone();
    let reader = match io::stdin().is_terminal() {
        true => reader::spawn(move |got| {
            let message = match got {
                Got::Event(event) => Incoming::Terminal(event),
                Got::Color(which, rgb) => Incoming::Color(which, rgb),
                Got::Keyboard(ask) => Incoming::Keyboard(ask),
            };
            keys.send(In::App(message)).is_ok()
        })
        .ok(),
        false => None,
    };
    // 留得下来的：在套接字上等同一个终端的下一个前台。绑不上的（地址太长、已经有一个在听）就不留。
    let listening = linger
        .as_ref()
        .filter(|linger| listen(&linger.socket, tx.clone()))
        .map(|linger| (linger.socket.clone(), linger.shell, linger.notify.clone()));
    let events = tx.clone();
    signals(&events);
    let core = Core::new(move |update| tx.send(In::App(Incoming::Core(update))).is_ok());
    let mut run = Run {
        app: App::new(config, core),
        rx,
        painter: Painter::new(io::stdout()),
        size: (80, 24),
        started: Instant::now(),
        painted: Instant::now(),
        mark: (0, None),
        shown: None,
        laying: Laying::default(),
        printed: 0,
        asked: None,
        unopened: false,
        results: Vec::new(),
        restored: false,
        restored_text: words.get("shellhook.slash.restored").to_string(),
        interrupted: false,
        failed: false,
        gone: false,
        front: 0,
        next: None,
        wait_jobs: listening.is_none(),
        quit: false,
    };
    let mut ask = first;
    let mut session = session;
    loop {
        run.front += 1;
        orders(events.clone(), &ask.control, run.front);
        // 新接上的前台：挪进它的进程组（留着的时候不在任何前台的进程组里，读终端会被作业控制拦住），接着读按键。
        if let Some(group) = ask.group.and_then(rustix::process::Pid::from_raw) {
            let _moved = rustix::process::setpgid(None, Some(group));
        }
        if let Some(reader) = &reader {
            reader.resume();
        }
        let code = run.go(&ask, session.take(), words);
        // 先让读按键的线程停下（它停好了才回，读走的按键都收下了，坑 P8），之后敲的字留在终端的缓冲里给 shell。
        if let Some(reader) = &reader {
            reader.pause();
        }
        run.drain_keys();
        kept(
            &match (run.unopened, run.app.talk.transcript.session.clone()) {
                (true, _) => Session::Kept,
                (false, Some(id)) => Session::Is(id),
                (false, None) => Session::None,
            },
        );
        // 没交出去的字写给脚本放回命令行（坑 S13）；`/edit` 放回来的上一句旁边放一个标记（坑 C4）。
        let typed = run.app.typed_inline();
        if let Some(path) = ask.leftover.as_deref().filter(|_| !typed.is_empty()) {
            drop(std::fs::write(path, &typed));
            if run.app.editing_inline() {
                drop(std::fs::write(format!("{path}.redo"), ""));
            }
        }
        run.app.clear_typed_inline();
        let stay = listening.is_some()
            && !run.gone
            && !run.quit
            && !run.unopened
            && run.app.lingering_inline();
        if !stay {
            // 有计划地停下会话（日志落好盘、放锁，后台命令整组停掉），再让前台走：下一句话来的时候会话已经放掉了。
            run.close();
            run.dismiss(&mut ask, code);
            break;
        }
        // 留下来：前台先走（提示符回来），这里接着等后台任务的回报、循环到点、同一个终端的下一个前台。
        run.dismiss(&mut ask, code);
        drop(ask);
        run.shown = Some(run.app.last_entry_inline());
        let (shell, notify) = listening
            .as_ref()
            .map_or((None, None), |(_, shell, notify)| {
                (*shell, notify.as_deref())
            });
        let next = run.detached(shell, notify);
        // 接上了、不留了：那个标记都不要了。
        if let Some(mark) = notify {
            drop(std::fs::remove_file(mark));
        }
        match next {
            Some(mut next) => {
                drop(ToFront::Ack.write(&mut next.control));
                ask = *next;
            }
            None => {
                run.close();
                break;
            }
        }
    }
    if let Some((socket, _, _)) = &listening {
        drop(std::fs::remove_file(socket));
    }
    Ok(())
}

/// 在 `socket` 上等同一个终端的下一个前台：来一个，读完它交的那几样，交给主循环。交回绑上了没有。地址上已经有东西的：
/// 连得上的是另一个进程在听（不抢它的，这个进程不留），连不上的是上次留下的，删了重绑。
fn listen(socket: &Path, events: mpsc::Sender<In>) -> bool {
    if socket.exists() {
        if UnixStream::connect(socket).is_ok() {
            return false;
        }
        drop(std::fs::remove_file(socket));
    }
    let Ok(listener) = UnixListener::bind(socket) else {
        return false;
    };
    let spawned = std::thread::Builder::new()
        .name("fronts".into())
        .spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut control) = stream else {
                    return;
                };
                drop(control.set_read_timeout(Some(HELLO)));
                let Ok(Some(ToWorker::Hello(hello))) = ToWorker::read(&mut control) else {
                    continue;
                };
                let text = match hello.report {
                    true => String::new(),
                    false => match ToWorker::read(&mut control) {
                        Ok(Some(ToWorker::Message(text))) => text,
                        _ => continue,
                    },
                };
                drop(control.set_read_timeout(None));
                let ask = Ask {
                    text,
                    report: hello.report,
                    redo: hello.redo,
                    leftover: Some(hello.leftover).filter(|path| !path.is_empty()),
                    control,
                    group: Some(hello.group),
                };
                if events.send(In::Front(Box::new(ask))).is_err() {
                    return;
                }
            }
        });
    spawned.is_ok()
}

/// `SIGINT`、`SIGTERM`、`SIGHUP` 写进一对套接字，各一条线程读出来交给主循环（不轮询）。装不上的照常跑，只是收不到。
/// 不装的话这几个信号照系统的默认直接结束这个进程：会话没有停好，后台命令没人管、接着跑。
fn signals(events: &mpsc::Sender<In>) {
    use signal_hook::consts::{SIGHUP, SIGINT, SIGTERM};
    for (signal, quit) in [(SIGINT, false), (SIGTERM, true), (SIGHUP, true)] {
        let Ok((knock, mut listen)) = UnixStream::pair() else {
            continue;
        };
        if signal_hook::low_level::pipe::register(signal, knock).is_err() {
            continue;
        }
        let events = events.clone();
        let spawned = std::thread::Builder::new()
            .name("signals".into())
            .spawn(move || {
                use std::io::Read;
                let mut byte = [0u8; 1];
                while matches!(listen.read(&mut byte), Ok(1)) {
                    if events.send(In::Signal(quit)).is_err() {
                        return;
                    }
                }
            });
        if spawned.is_err() {}
    }
}

/// 终端报的大小 `got`（列、行）；问不到、报成 0 的（有的伪终端、串口上是这样）照 `fallback`。宽、高是 0 的话一行都
/// 排不出来（2026-11-23 量留着的进程时用的伪终端没设大小，程序在分「定下来的」和「还会变的」那一步崩了）。
fn sized(got: Option<(u16, u16)>, fallback: (u16, u16)) -> (u16, u16) {
    match got {
        Some((width, height)) if width > 0 && height > 0 => (width, height),
        _ => fallback,
    }
}

/// 等第 `front` 个前台的控制线关上：那就是它不在了。
fn orders(events: mpsc::Sender<In>, control: &UnixStream, front: u64) {
    let Ok(mut control) = control.try_clone() else {
        return;
    };
    let spawned = std::thread::Builder::new()
        .name("orders".into())
        .spawn(move || {
            while let Ok(Some(_)) = ToWorker::read(&mut control) {}
            let _told = events.send(In::Gone(front));
        });
    if spawned.is_err() {}
}

/// 跑着的这一次。
struct Run {
    app: App,
    rx: mpsc::Receiver<In>,
    painter: Painter<io::Stdout>,
    /// 终端多宽、多高。
    size: (u16, u16),
    /// 起来的时刻：转圈转到第几帧照它算。
    started: Instant,
    /// 上一帧什么时候画的。
    painted: Instant,
    /// 说话以前正文里最新一条的编号、开这一轮的那句话的编号（`App::lay_inline`）。
    mark: (u64, Option<u64>),
    /// 上一个前台走的时候正文印到了哪一条（留着的时候后来的还没印过）；头一个前台没有。
    shown: Option<u64>,
    /// 排的时候记着的（还在写的那一段回答前几行定下来了）。
    laying: Laying,
    /// 印进滚动历史的有几行。
    printed: usize,
    /// 抽屉从什么时候开着的（没开着的是 `None`）：开满 [`DRAWER_SETTLE`] 才画。
    asked: Option<Instant>,
    /// 要接着说的会话没开成。
    unopened: bool,
    /// 斜杠命令办完要留的那几行结果（换了模型、换了会话）。
    results: Vec<String>,
    /// 恢复了刚撤销的那几轮（`/restore`）：收尾时留一行结果。
    restored: bool,
    /// 那一行写什么。
    restored_text: String,
    /// 按过 `Ctrl+C`。
    interrupted: bool,
    /// 没发出去、被拒、引擎起不来。
    failed: bool,
    /// 前台不在了。
    gone: bool,
    /// 现在接着的是第几个前台（它的控制线关了才算「前台不在了」）。
    front: u64,
    /// 办着一件的时候同一个终端又来了一个前台：先收着。
    next: Option<Box<Ask>>,
    /// 了结要不要等后台任务都报了：留不下来的等，留得下来的不等。
    wait_jobs: bool,
    /// 被要求结束了（`SIGTERM`、终端关了的 `SIGHUP`）：不留。
    quit: bool,
}

impl Run {
    /// 有计划地停下会话：日志落好盘、放锁，后台命令整组停掉。
    fn close(&mut self) {
        let until = Instant::now() + CLOSING;
        self.app.finish_writes(until);
        self.app.link.core.finish(until);
    }

    /// 让前台走：说办完了、交退出码。写不进的（它已经不在了）没处可报。
    fn dismiss(&mut self, ask: &mut Ask, code: u8) {
        drop(io::stdout().flush());
        drop(ToFront::Done.write(&mut ask.control));
        drop(ToFront::Exit(code).write(&mut ask.control));
    }

    /// 留着：没有前台接着。引擎的消息照常办（后台任务的回报、循环到点开的一轮都在这时候来），等同一个终端的下一个
    /// 前台；交回它。没有要等的了、shell 不在了、引擎不在了，交回 `None`（该停下会话退出了）。
    ///
    /// 给了 `notify` 的（脚本接 `SIGUSR1`）：这期间正文里有了没印过的（后台任务结束的那一行、被叫醒开的一轮）、开了抽屉，
    /// 就放上那个标记、给 shell 发一次信号，等它跑 `shellhook report` 来接；没印过的还在的时候不退出。没给的发不了信号，
    /// 没印过的只能等下一句话来的时候一起印，没有要等的了照旧退出。
    fn detached(&mut self, shell: Option<i32>, notify: Option<&Path>) -> Option<Box<Ask>> {
        let shell = shell.and_then(rustix::process::Pid::from_raw);
        let mut notified = false;
        loop {
            let unseen = notify.is_some()
                && (self.app.last_entry_inline() > self.shown.unwrap_or(0)
                    || self.app.asking_inline());
            if let (true, false, Some(mark), Some(shell)) = (unseen, notified, notify, shell) {
                notified = true;
                // 先放标记再发信号：脚本收到信号先看标记在不在。
                if std::fs::write(mark, "").is_ok() {
                    let _sent = rustix::process::kill_process(shell, rustix::process::Signal::USR1);
                }
            }
            if self.quit {
                return None;
            }
            if let Some(next) = self.next.take() {
                return Some(next);
            }
            // 没有要等的、也没有一轮在跑、没有等着印的：过一道栅栏还是这样，就不留了。
            if !unseen && !self.app.lingering_inline() && self.app.settled_inline(true) {
                let quiet =
                    self.barrier() && !self.app.lingering_inline() && self.app.settled_inline(true);
                match self.next.take() {
                    Some(next) => return Some(next),
                    None if quiet => return None,
                    None => {}
                }
            }
            match self.rx.recv_timeout(SHELL_CHECK) {
                Ok(message) => self.take(message),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return None,
            }
            self.app.tick();
            // shell 退出了（终端关了）：不留了。没有这个进程（`ESRCH`）才算不在。
            if shell.is_some_and(|shell| {
                rustix::process::test_kill_process(shell) == Err(rustix::io::Errno::SRCH)
            }) {
                return None;
            }
        }
    }

    /// 办一件：开会话（头一回）、说那一句（印回报的不说）、跟到了结、印完，交回退出码。
    fn go(&mut self, ask: &Ask, session: Option<String>, words: &Words) -> u8 {
        let (text, redo) = (ask.text.as_str(), ask.redo);
        self.size = sized(crossterm::terminal::size().ok(), self.size);
        (self.interrupted, self.failed, self.gone, self.unopened) = (false, false, false, false);
        (self.restored, self.asked) = (false, None);
        self.results.clear();
        if ask.report {
            // 印回报：上一次印到的地方以后的都没印过。
            self.mark = (self.shown.unwrap_or(0), None);
            self.printed = 0;
            return self.follow(false, words);
        }

        // 在 shell 里要另外办的几条斜杠命令。进全屏的那几条、`/loop` 还没有做，先照实说。
        let slash = self.app.slash_inline(text);
        let name = text.split_whitespace().next().unwrap_or_default();
        let told = match slash {
            Some(Slash::FullscreenOnly) => Some("shellhook.slash.fullscreen_only"),
            // `/config`、`/tui` 在开这一份状态以前就交给前台去起全屏界面了（[`fullscreen`]），到这里的是 `/files`、`/jump`。
            Some(Slash::Fullscreen) => Some("shellhook.slash.fullscreen"),
            // `/loop` 要这个进程留得下来才到得了点。
            Some(Slash::Loop) if self.wait_jobs => Some("shellhook.slash.loop"),
            Some(Slash::Loop | Slash::New) | None => None,
        };
        if let Some(key) = told {
            self.unopened = true;
            return self.told(&words.say(key, &[("name", name)]));
        }
        if let Some(id) = session {
            self.app.open_session(id.clone());
            while self.app.opening_inline() && !self.stopping() {
                self.wait(None);
            }
            if self.app.talk.transcript.session.as_deref() != Some(id.as_str()) {
                // 开不了（开在别的窗口里、读不了）：原因在提示里。
                self.failed = true;
                self.unopened = true;
                return self.end();
            }
            // 人在两句话之间可能 `cd` 过：这一句在 shell 现在的目录里办。
            self.app.link.core.send(Command::Workspace);
        }
        let before = self.app.last_entry_inline();
        match redo {
            true => self.app.redo_inline(text),
            false => self.app.say_inline(text),
        }
        if slash == Some(Slash::New) {
            return self.told(words.get("shellhook.slash.new"));
        }
        let said = Some(self.app.last_entry_inline()).filter(|id| *id > before);
        // 留着的时候来的（后台任务结束的那一行、被叫醒开的一轮）还没印过：连同这一句的一起印。
        self.mark = (self.shown.take().unwrap_or(before).min(before), said);
        self.printed = 0;
        self.follow(said.is_none(), words)
    }

    /// 跟到了结、印完，交回退出码。`command` 是这一件是斜杠命令（没有说出去的话）。
    fn follow(&mut self, command: bool, words: &Words) -> u8 {
        let mut current = self.app.talk.transcript.session.clone();
        let mut opened = false;
        // 斜杠命令换了模型、换了会话的，收工时各留一行结果（坑 C3）。
        let model = self.app.model_inline();
        loop {
            // 换了会话（`/sessions` 里选的另一个、`/new`）：补发来的旧正文不印，从这以后的才印。新开的会话刚拿到编号
            // （原来没有、也没有在开旧会话）不算换：它的正文就是这一句的回答。
            opened |= self.app.opening_inline();
            if !self.app.opening_inline() && self.app.talk.transcript.session != current {
                let fresh = current.is_none() && !opened;
                current = self.app.talk.transcript.session.clone();
                opened = false;
                if !fresh {
                    self.mark = (self.app.last_entry_inline(), None);
                    self.printed = 0;
                    if let Some(id) = &current {
                        let title = self.app.title_inline().unwrap_or_else(|| id.clone());
                        self.results
                            .push(words.say("shellhook.slash.session", &[("title", &title)]));
                    }
                }
            }
            // 开着的旧会话还在收补发：不画（同全屏）。
            if !self.app.opening_inline() {
                self.paint(false);
            }
            if self.stopping() {
                return self.end();
            }
            match self.app.panel_inline() {
                // 没有要选的：等引擎把这个面板要的交齐，整个印出来就完。
                Some(Shown::Printed) => {
                    self.barrier();
                    self.print_panel();
                    self.app.close_panel_inline();
                    continue;
                }
                Some(Shown::Picked) => {}
                None if !self.app.opening_inline() && self.settled() => {
                    if let Some(now) = self
                        .app
                        .model_inline()
                        .filter(|now| command && current.is_some() && model.as_ref() != Some(now))
                    {
                        self.results
                            .push(words.say("shellhook.slash.model", &[("name", &now)]));
                    }
                    return self.end();
                }
                None => {}
            }
            // 最晚到界面自己要醒的时刻（转圈、用时跳秒），和刚开的抽屉该画出来的时刻。
            let drawer = self
                .asked
                .map(|since| since + DRAWER_SETTLE)
                .filter(|due| *due > Instant::now());
            let until = [self.app.deadline(), drawer].into_iter().flatten().min();
            self.wait(until);
            self.app.tick();
            self.asked = match (self.app.asking_inline(), self.asked) {
                (true, since) => since.or(Some(Instant::now())),
                (false, _) => None,
            };
        }
    }

    /// 留一行结果就收工（斜杠命令办完了、办不了的说明）：前面空一行，和提示符那一行隔开。
    fn told(&mut self, line: &str) -> u8 {
        drop(
            self.painter
                .frame(&[String::new(), format!("  {line}")], &[], None),
        );
        self.end()
    }

    /// 不等了：前台不在了，或者按了第二下 `Ctrl+C`（坑 P15）。
    fn stopping(&self) -> bool {
        self.gone
    }

    /// 了结了没有：界面那一份状态说了结了，再过一道栅栏（引擎把交给它的都办完了，接着开的一轮不会还在路上）还是这样。
    fn settled(&mut self) -> bool {
        if !self.app.settled_inline(self.wait_jobs) {
            return false;
        }
        if !self.barrier() {
            return true;
        }
        self.app.settled_inline(self.wait_jobs)
            && self.app.panel_inline().is_none()
            && !self.app.opening_inline()
    }

    /// 过一道栅栏：等引擎把交给它的都办完，它这期间交来的都办掉。交回等到了没有（引擎不在了、太久的是假）。
    fn barrier(&mut self) -> bool {
        let (done, back) = mpsc::channel();
        self.app.link.core.send(Command::Barrier(done));
        if back.recv_timeout(BARRIER).is_err() {
            return false;
        }
        while let Ok(message) = self.rx.try_recv() {
            self.take(message);
        }
        true
    }

    /// 没有要选的面板（`/help`、`/status`）：整个印进滚动历史，前面空一行。
    fn print_panel(&mut self) {
        let (width, _) = self.size;
        let Some(mut drawn) = self.app.panel_block_inline(width, u16::MAX) else {
            return;
        };
        let mut lines = vec![String::new()];
        lines.extend(paint::rendered(&mut drawn));
        drop(self.painter.frame(&lines, &[], None));
    }

    /// 等下一样（最晚到 `until`），连同已经到了的一起办；一帧的间隔里再到的也收齐（一段段推来的字不是每段画一帧）。
    fn wait(&mut self, until: Option<Instant>) {
        let limit = until.map_or(Duration::from_secs(3600), |until| {
            until.saturating_duration_since(Instant::now())
        });
        match self.rx.recv_timeout(limit) {
            Ok(message) => self.take(message),
            Err(RecvTimeoutError::Timeout) => return,
            Err(RecvTimeoutError::Disconnected) => {
                self.gone = true;
                return;
            }
        }
        let beat = Duration::from_millis(self.app.config.layout.frame_ms);
        let next = self.painted + beat;
        loop {
            let left = next.saturating_duration_since(Instant::now());
            let got = match left.is_zero() {
                true => self.rx.try_recv().ok(),
                false => self.rx.recv_timeout(left).ok(),
            };
            match got {
                Some(message) => self.take(message),
                None => return,
            }
            if left.is_zero() && self.gone {
                return;
            }
        }
    }

    /// 办一样。
    fn take(&mut self, message: In) {
        match message {
            In::Signal(true) => (self.quit, self.gone) = (true, true),
            In::Signal(false) => self.interrupt(),
            // 以前的前台收到退出码走了，它的控制线跟着关：不算。
            In::Gone(front) => self.gone |= front == self.front,
            // 办着一件的时候又来了一个前台：先收着，这一件办完再接它（已经收着一个的不要，它等不到回音会自己另起）。
            In::Front(ask) => {
                if self.next.is_none() {
                    self.next = Some(ask);
                }
            }
            In::App(Incoming::Terminal(Ok(event))) => self.key(event),
            // 终端读不了了：照前台不在了收场。
            In::App(Incoming::Terminal(Err(_))) => self.gone = true,
            In::App(Incoming::Core(update)) => {
                self.failed |= matches!(
                    update,
                    Update::Unsent { .. } | Update::Failed(_) | Update::Refused { .. }
                );
                self.restored |= matches!(update, Update::Undone { restore: true, .. });
                self.app.core(update);
            }
            In::App(Incoming::Color(which, rgb)) => self.app.terminal_color(which, rgb),
            // 这里不开 kitty 键盘协议（做答话时的输入行那一步再开）：照不认答。
            In::App(Incoming::Keyboard(_)) => {}
        }
    }

    /// 终端的一个事件。`Ctrl+C` 打断这一轮（照全屏：排着的话退回输入框），没有一轮在跑的、已经按过一下的不等了（坑
    /// P15）；抽屉开着的时候别的键归抽屉；没有抽屉的时候回车是插话，别的键归输入框。
    fn key(&mut self, event: Event) {
        let key = match event {
            Event::Key(key) if key.kind != KeyEventKind::Release => key,
            Event::Paste(text) => return self.app.paste_inline(&text),
            Event::Resize(width, height) => {
                return self.size = sized(Some((width, height)), self.size);
            }
            _ => return,
        };
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        if control && key.code == KeyCode::Char('c') {
            self.interrupt();
        } else if self.app.panel_inline().is_some() {
            self.app.panel_key_inline(key);
        } else if self.app.asking_inline() {
            // 抽屉还没画出来（刚开、没满那一小会儿）的时候按的键不算：人还没看到问的是什么。
            if self
                .asked
                .is_some_and(|since| since.elapsed() >= DRAWER_SETTLE)
            {
                self.app.drawer_key_inline(key);
            }
        } else if key.code == KeyCode::Enter && key.modifiers.is_empty() {
            self.app.interject_inline();
        } else {
            self.app.type_inline(key);
        }
    }

    /// 按了一下 `Ctrl+C`：面板开着的只是不选了、关上它；在回答的打断这一轮（照全屏：排着的话退回输入框）；没有一轮
    /// 在跑的、已经按过一下的不等了（坑 P15）。
    fn interrupt(&mut self) {
        if self.app.panel_inline().is_some() {
            return self.app.close_panel_inline();
        }
        let again = std::mem::replace(&mut self.interrupted, true);
        match again || !self.app.answering_inline() {
            true => self.gone = true,
            false => self.app.interrupt_inline(),
        }
    }

    /// 读按键的线程停下以后，它已经交过来、还没办的按键收下。
    fn drain_keys(&mut self) {
        while let Ok(message) = self.rx.try_recv() {
            if let In::App(Incoming::Terminal(Ok(event))) = message {
                self.key(event);
            }
        }
    }

    /// 画一帧：定下来的印进滚动历史，还会变的画在活动区（最多终端高减一行，露最新的那一截，坑 P4），最下面是抽屉或者
    /// 输入那几行。`last` 是收尾的那一帧：都印上去，不留活动区。写不进终端的没处可报。
    fn paint(&mut self, last: bool) {
        let (width, height) = self.size;
        let beat = u128::from(self.app.config.timeline.spinner_ms.max(1));
        let frame = usize::try_from(self.started.elapsed().as_millis() / beat).unwrap_or(0);
        // 每行前面两格槽，内容照终端宽减两格排（坑 P5）。
        let laid = self
            .app
            .lay_inline(&self.laying, self.mark, width.saturating_sub(2), frame);
        let total = laid.lines.len();
        let from = self.printed.min(total);
        let settled = match last {
            true => total,
            false => laid.settled.clamp(from, total),
        };
        let text = paint::lines(width, &laid.lines[from..]);
        let (now, changing) = text.split_at((settled - from).min(text.len()));
        let room = usize::from(height.saturating_sub(1)).max(1);
        let (below, caret) = match last {
            true => (Vec::new(), None),
            false => self.below(width, height),
        };
        // 最下面那一块先占地方（太高的只露它的上半截），剩下的给正文，露最新的那一截。
        let below = &below[..below.len().min(room)];
        let keep = room - below.len();
        let changing = &changing[changing.len().saturating_sub(keep)..];
        let live = [changing, below].concat();
        let caret = caret
            .filter(|(row, _)| *row < below.len())
            .map(|(row, column)| (changing.len() + row, column));
        drop(self.painter.frame(now, &live, caret));
        self.printed = settled;
        self.painted = Instant::now();
    }

    /// 活动区最下面的那一块：前面空一行；抽屉开着（开满了那一小会儿）的是抽屉，没开着、在回答的是排着的插话和输入那几行。
    /// 另交回光标停在这一块的第几行、第几列。什么都不用画的是空的。
    fn below(&mut self, width: u16, height: u16) -> (Vec<String>, paint::Caret) {
        let shown = self
            .asked
            .is_some_and(|since| since.elapsed() >= DRAWER_SETTLE);
        if shown && let Some((mut drawn, at)) = self.app.drawer_inline(width, (height / 2).max(5)) {
            let mut lines = vec![String::new()];
            lines.extend(paint::rendered(&mut drawn));
            return (lines, at.map(|(row, column)| (row + 1, column)));
        }
        // 斜杠命令开的面板：画在这里等人选。
        if self.app.panel_inline() == Some(Shown::Picked) {
            let Some(mut drawn) = self.app.panel_block_inline(width, (height / 2).max(8)) else {
                return (Vec::new(), None);
            };
            let mut lines = vec![String::new()];
            lines.extend(paint::rendered(&mut drawn));
            return (lines, None);
        }
        // 抽屉刚开、还没画出来：输入那几行也不画（按键这时归抽屉）。
        if self.app.asking_inline() || !self.app.answering_inline() {
            return (Vec::new(), None);
        }
        let (rows, (row, column)) = self.app.input_inline(self.mark.0, width);
        let mut lines = vec![String::new()];
        lines.extend(paint::lines(width, &rows));
        (lines, Some((row + 1, column)))
    }

    /// 收尾：没印过的照最后的样子印出来，擦掉活动区，界面要说的那一句提示印在后面；交回退出码。
    fn end(&mut self) -> u8 {
        self.paint(true);
        if std::mem::take(&mut self.restored) {
            self.results.push(self.restored_text.clone());
        }
        if !self.results.is_empty() {
            let mut lines = vec![String::new()];
            lines.extend(self.results.drain(..).map(|line| format!("  {line}")));
            drop(self.painter.frame(&lines, &[], None));
        }
        drop(self.painter.finish());
        if let Some(notice) = &self.app.typing.notice {
            let mut out = io::stdout().lock();
            drop(writeln!(out, "  {}{}", notice.text, notice.tail));
            drop(out.flush());
        }
        match (self.interrupted || self.gone, self.failed) {
            (true, _) => INTERRUPTED,
            (false, true) => 1,
            (false, false) => 0,
        }
    }
}
