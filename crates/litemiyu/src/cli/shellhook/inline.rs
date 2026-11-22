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
use std::os::unix::net::UnixStream;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::{Duration, Instant};

use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers};

use super::paint::{self, Painter};
use crate::app::App;
use crate::app::inline::Laying;
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

/// 主循环等的东西。
enum In {
    /// 终端的事件、引擎的消息（同全屏的主循环）。
    App(Incoming),
    /// 前台的进程不在了（控制线关了：终端关了、它被杀了）。
    Gone,
}

/// 跑完的样子。
pub struct Outcome {
    /// 退出码。
    pub code: u8,
    /// 说话的那个会话（开好了的）。
    pub session: Option<String>,
    /// 答话的时候提前敲的字，没交出去的。
    pub typed: String,
}

/// 说 `text`；`session` 是这个终端接着说的会话，没有的新开。`control` 是和前台连着的控制线：它关了就收场。
///
/// # Errors
///
/// 配置读不懂（编进程序的字表坏了、`config.toml` 读不了）：原话。
pub fn run(text: &str, session: Option<String>, control: &UnixStream) -> Result<Outcome, String> {
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
    orders(tx.clone(), control);
    let core = Core::new(move |update| tx.send(In::App(Incoming::Core(update))).is_ok());
    let mut run = Run {
        app: App::new(config, core),
        rx,
        painter: Painter::new(io::stdout()),
        size: crossterm::terminal::size().unwrap_or((80, 24)),
        started: Instant::now(),
        painted: Instant::now(),
        mark: (0, None),
        laying: Laying::default(),
        printed: 0,
        asked: None,
        interrupted: false,
        failed: false,
        gone: false,
    };
    let code = run.go(text, session);
    // 先让读按键的线程停下（它停好了才回，读走的按键都收下了，坑 P8），之后敲的字留在终端的缓冲里给 shell。
    if let Some(reader) = &reader {
        reader.pause();
    }
    run.drain_keys();
    let session = run.app.talk.transcript.session.clone();
    let typed = run.app.typed_inline();
    // 有计划地停下会话：日志落好盘、放锁，后台命令整组停掉。
    let until = Instant::now() + CLOSING;
    run.app.finish_writes(until);
    run.app.link.core.finish(until);
    Ok(Outcome {
        code,
        session,
        typed,
    })
}

/// 等控制线关上：那就是前台不在了。
fn orders(events: mpsc::Sender<In>, control: &UnixStream) {
    let Ok(mut control) = control.try_clone() else {
        return;
    };
    let spawned = std::thread::Builder::new()
        .name("orders".into())
        .spawn(move || {
            while let Ok(Some(_)) = super::wire::ToWorker::read(&mut control) {}
            let _told = events.send(In::Gone);
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
    /// 排的时候记着的（还在写的那一段回答前几行定下来了）。
    laying: Laying,
    /// 印进滚动历史的有几行。
    printed: usize,
    /// 抽屉从什么时候开着的（没开着的是 `None`）：开满 [`DRAWER_SETTLE`] 才画。
    asked: Option<Instant>,
    /// 按过 `Ctrl+C`。
    interrupted: bool,
    /// 没发出去、被拒、引擎起不来。
    failed: bool,
    /// 前台不在了。
    gone: bool,
}

impl Run {
    /// 开会话、说那一句、跟到了结、印完，交回退出码。
    fn go(&mut self, text: &str, session: Option<String>) -> u8 {
        if let Some(id) = session {
            self.app.open_session(id.clone());
            while self.app.opening_inline() && !self.stopping() {
                self.wait(None);
            }
            if self.app.talk.transcript.session.as_deref() != Some(id.as_str()) {
                // 开不了（开在别的窗口里、读不了）：原因在提示里。
                self.failed = true;
                return self.end();
            }
            // 人在两句话之间可能 `cd` 过：这一句在 shell 现在的目录里办。
            self.app.link.core.send(Command::Workspace);
        }
        let before = self.app.last_entry_inline();
        self.app.say_inline(text);
        let said = Some(self.app.last_entry_inline()).filter(|id| *id > before);
        self.mark = (before, said);
        loop {
            self.paint(false);
            if self.stopping() || self.settled() {
                return self.end();
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

    /// 不等了：前台不在了，或者按了第二下 `Ctrl+C`（坑 P15）。
    fn stopping(&self) -> bool {
        self.gone
    }

    /// 了结了没有：界面那一份状态说了结了，再过一道栅栏（引擎把交给它的都办完了，接着开的一轮不会还在路上）还是这样。
    fn settled(&mut self) -> bool {
        if !self.app.settled_inline() {
            return false;
        }
        let (done, back) = mpsc::channel();
        self.app.link.core.send(Command::Barrier(done));
        if back.recv_timeout(BARRIER).is_err() {
            return true;
        }
        while let Ok(message) = self.rx.try_recv() {
            self.take(message);
        }
        self.app.settled_inline()
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
            In::Gone => self.gone = true,
            In::App(Incoming::Terminal(Ok(event))) => self.key(event),
            // 终端读不了了：照前台不在了收场。
            In::App(Incoming::Terminal(Err(_))) => self.gone = true,
            In::App(Incoming::Core(update)) => {
                self.failed |= matches!(
                    update,
                    Update::Unsent { .. } | Update::Failed(_) | Update::Refused { .. }
                );
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
            Event::Resize(width, height) => return self.size = (width, height),
            _ => return,
        };
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        if control && key.code == KeyCode::Char('c') {
            let again = std::mem::replace(&mut self.interrupted, true);
            match again || !self.app.answering_inline() {
                true => self.gone = true,
                false => self.app.interrupt_inline(),
            }
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
        let (now, changing) = text.split_at(settled - from);
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
