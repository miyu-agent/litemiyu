//! 读按键的线程：睡着等终端的事件，来一个往主循环送一个。不轮询（`docs/00-设计.md` 第四节）。
//!
//! 终端回问颜色的话也从这里进来（`theme/answer.rs`）：crossterm 把回话读成一串按键，这里认出来换成颜色交出去，不漏给
//! 输入框。认到半截时不死等：停 [`HOLD`] 还没下文的，攒着的照按键交出去（人按的 Alt+`]`）。
//!
//! 能停下（[`Reader::pause`]）：让出终端给外部编辑器时，这条线程不再读终端，编辑器退出了再接着读（[`Reader::resume`]）。
//! Miyu 终端用 crossterm 的 `EventStream` 加 tokio 的 `select!` 敲门；那一套每次等事件另起一条线程（空闲时多一条），这里
//! 换成线程自己在一个能被叫醒的口子上等：`poll(2)` 同时等终端和一对本机套接字，主线程要它停时往套接字里写一个字。终端
//! 窗口变了大小（`SIGWINCH`）也往这对套接字里写（`signal-hook` 的 `pipe`）：crossterm 自己的信号口子只在它自己等的时候
//! 看，这里先醒过来，再让 crossterm 不等地读，它照旧交出 `Resize`。空闲时这条线程照旧睡着，不定时醒（界面线第二趟
//! 量过）。

use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use crossterm::event::{self, Event};
use rustix::event::{PollFd, PollFlags, Timespec, poll};

use crate::theme::{Catch, Caught, Rgb, Which};

/// 这条线程的栈。它只做读和送，用不着默认的 2 MB。
const STACK: usize = 64 * 1024;

/// 认回话认到半截，最多等下一个字多久：终端的回话一口气到，人按的 Alt+`]` 后面不会跟着这一串。
const HOLD: Duration = Duration::from_millis(50);

/// 问键盘协议以前最多等主线程说「可以了」多久。
const GO_WAIT: Duration = Duration::from_secs(1);

/// 读到的。
pub enum Got {
    /// 终端的事件；读不了终端的是错误。
    Event(io::Result<Event>),
    /// 终端回了它的一个颜色。
    Color(Which, Rgb),
    /// 终端答过问颜色的话了，接着要问它认不认 kitty 键盘协议（[`Ask`]）：主线程停笔以后才问。
    Keyboard(Ask),
}

/// 问终端认不认 kitty 键盘协议（同 miyu-tui `main.rs` 的 `enter`：认得的打开，Shift+Enter 才分得出来）。问句由 crossterm 直接
/// 写进 `/dev/tty`，和主线程写的帧不走同一把锁：主线程收到它先 [`Ask::answer`]（期间不写终端），读按键的线程问完交回结果。
/// 启动不等它：问颜色的回话到了才问，那时第一帧早画完了；不回颜色的终端（量尺、伪终端测试）不问，一直不开。
pub struct Ask {
    go: mpsc::Sender<()>,
    answered: mpsc::Receiver<bool>,
}

impl Ask {
    /// 叫读按键的线程问，等它问完：交回终端认不认。线程不在了的交回假。终端答了颜色却不答 DA1 的，crossterm 最多等两秒。
    pub fn answer(self) -> bool {
        self.go.send(()).is_ok() && self.answered.recv().unwrap_or(false)
    }
}

/// 问认不认 kitty 键盘协议：先交给主线程 [`Got::Keyboard`]，等它说可以了再问（期间它不写终端），问完交回去。主循环不收了的不问。
fn ask_keyboard(send: &impl Fn(Got) -> bool) -> bool {
    let (go, wait) = mpsc::channel();
    let (tell, answered) = mpsc::channel();
    if !send(Got::Keyboard(Ask { go, answered })) {
        return false;
    }
    // 主线程这时正在叫它停（让出终端给编辑器、挂起）的，等不来「可以了」：等一会儿就不问了，免得两头互等。
    if wait.recv_timeout(GO_WAIT).is_ok() {
        let supported = crossterm::terminal::supports_keyboard_enhancement().unwrap_or(false);
        tell.send(supported).unwrap_or_default();
    }
    true
}

/// 主线程叫它做的。
enum Control {
    /// 停下：停好了（不再读终端）回一声。
    Pause(mpsc::Sender<()>),
    /// 接着读。
    Resume,
}

/// 拿着读按键的线程：叫它停下、接着读。
pub struct Reader {
    control: mpsc::Sender<Control>,
    /// 敲门的一头：往里写一个字，等在 `poll` 上的线程醒过来看 `control`。
    knock: UnixStream,
}

impl Reader {
    /// 停下：等线程不再读终端了才回（让出终端以前调）。线程已经不在了的当场回。
    pub fn pause(&self) {
        let (done, wait) = mpsc::channel();
        if self.control.send(Control::Pause(done)).is_err() {
            return;
        }
        // 写不进的是线程已经退了：下面的等也当场回。
        (&self.knock).write_all(b"p").unwrap_or_default();
        wait.recv().unwrap_or_default();
    }

    /// 接着读（收回终端以后调）。
    pub fn resume(&self) {
        self.control.send(Control::Resume).unwrap_or_default();
    }
}

/// 起读按键的线程：读到一个交给 `send`。`send` 交回假的（主循环那一头不收了），或者终端读不了了，线程自己结束。
///
/// # Errors
///
/// 系统不让起线程、开不了本机套接字、登记不了 `SIGWINCH`。
pub fn spawn(send: impl Fn(Got) -> bool + Send + 'static) -> io::Result<Reader> {
    let (knock, door) = UnixStream::pair()?;
    door.set_nonblocking(true)?;
    signal_hook::low_level::pipe::register(signal_hook::consts::SIGWINCH, knock.try_clone()?)?;
    let (control, orders) = mpsc::channel();
    thread::Builder::new()
        .name("keys".into())
        .stack_size(STACK)
        .spawn(move || run(&send, &door, &orders))?;
    Ok(Reader { control, knock })
}

/// 线程的主体：先把 crossterm 手里不用等就有的事件都交出去，再睡在终端和门上等。
fn run(send: &impl Fn(Got) -> bool, door: &UnixStream, orders: &mpsc::Receiver<Control>) {
    let mut catch = Catch::default();
    let mut asked = false;
    let pass = |events: Vec<Event>| events.into_iter().all(|e| send(Got::Event(Ok(e))));
    loop {
        // 不用等就读得到的。
        loop {
            match event::poll(Duration::ZERO) {
                Ok(true) => {}
                Ok(false) => break,
                Err(error) => return fail(send, &pass, &mut catch, error),
            }
            let caught = match event::read() {
                Ok(event) => catch.feed(event),
                Err(error) => return fail(send, &pass, &mut catch, error),
            };
            let alive = match caught {
                Caught::Pass(events) => pass(events),
                Caught::Hold => true,
                // 底色是问句的最后一句：答到这里，终端是会答话的，接着问键盘协议（只问一次）。
                Caught::Color(Which::Bg, rgb) if !asked => {
                    asked = true;
                    send(Got::Color(Which::Bg, rgb)) && ask_keyboard(send)
                }
                Caught::Color(which, rgb) => send(Got::Color(which, rgb)),
            };
            if !alive {
                return;
            }
        }
        // 认到半截的只等一会儿；别的一直睡着等。
        let woke = match wait(door, catch.holding().then_some(HOLD)) {
            Ok(woke) => woke,
            Err(error) => return fail(send, &pass, &mut catch, error),
        };
        if woke == Woke::Timeout && !pass(catch.flush()) {
            return;
        }
        if woke == Woke::Door && !obey(door, orders) {
            return;
        }
    }
}

/// 读不了了：攒着的先交出去，再报错；主循环不收了的就不报。
fn fail(
    send: &impl Fn(Got) -> bool,
    pass: &impl Fn(Vec<Event>) -> bool,
    catch: &mut Catch,
    error: io::Error,
) {
    if pass(catch.flush()) {
        send(Got::Event(Err(error)));
    }
}

/// 睡醒的原因。
#[derive(Debug, PartialEq, Eq)]
enum Woke {
    /// 终端有字了。
    Terminal,
    /// 有人敲门（主线程要它停、窗口变了大小）。
    Door,
    /// 等到了 `limit`。
    Timeout,
}

/// 同时等终端（标准输入）和门；`limit` 是最多等多久，没有的一直等。
///
/// 等的是 0 号文件描述符本身（占用 四 A6）：标准库的 `io::stdin()` 头一回调就给进程造一份 8 KiB 的读缓冲，这里只要它的
/// 描述符、从来不经它读（按键由 crossterm 读），那 8 KiB 白占着。
fn wait(door: &UnixStream, limit: Option<Duration>) -> io::Result<Woke> {
    let stdin = rustix::stdio::stdin();
    let mut fds = [
        PollFd::new(&stdin, PollFlags::IN),
        PollFd::new(door, PollFlags::IN),
    ];
    let limit = limit
        .map(Timespec::try_from)
        .transpose()
        .map_err(io::Error::other)?;
    loop {
        match poll(&mut fds, limit.as_ref()) {
            Ok(0) => return Ok(Woke::Timeout),
            Ok(_) => break,
            Err(rustix::io::Errno::INTR) => {}
            Err(error) => return Err(error.into()),
        }
    }
    if fds[1].revents().contains(PollFlags::IN) {
        return Ok(Woke::Door);
    }
    Ok(Woke::Terminal)
}

/// 有人敲门：门里的字读干净，看主线程有没有话。要停的：回一声，等到「接着读」（或者主线程不在了）再回。交回还接不接着跑。
fn obey(mut door: &UnixStream, orders: &mpsc::Receiver<Control>) -> bool {
    let mut drained = [0u8; 64];
    while matches!(door.read(&mut drained), Ok(n) if n > 0) {}
    while let Ok(order) = orders.try_recv() {
        let Control::Pause(done) = order else {
            continue;
        };
        done.send(()).unwrap_or_default();
        loop {
            match orders.recv() {
                Ok(Control::Resume) => break,
                Ok(Control::Pause(done)) => if done.send(()).is_err() {},
                Err(_) => return false,
            }
        }
    }
    true
}
