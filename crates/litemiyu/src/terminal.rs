//! 进出全屏：原始模式、备用屏、粘贴括起来、鼠标；终端认得 kitty 键盘协议的另打开它（Shift+Enter 才分得出来，同 miyu-tui
//! `main.rs`）。出去的时候（正常退、出错、崩了）都要把终端还回原样。

use std::io::{self, Write};
use std::panic::PanicHookInfo;
use std::sync::atomic::{AtomicBool, Ordering};

use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::terminal::{self, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::{cursor, execute};

/// kitty 键盘协议开着（[`keyboard_on`]）：退出、挂起、让出终端时照它关，回来再开。崩的时候 panic 钩子里也照它关。
static KEYBOARD: AtomicBool = AtomicBool::new(false);

/// 终端答了认得 kitty 键盘协议：打开（只要分得出 Shift+Enter 这类，同 miyu-tui 的 `DISAMBIGUATE_ESCAPE_CODES`）。
///
/// # Errors
///
/// 写不进终端。
pub fn keyboard_on() -> io::Result<()> {
    KEYBOARD.store(true, Ordering::Relaxed);
    push_keyboard(&mut io::stdout())
}

/// 开着键盘协议的：再开一次（挂起、让出终端回来以后）。
fn push_keyboard(out: &mut impl Write) -> io::Result<()> {
    if KEYBOARD.load(Ordering::Relaxed) {
        execute!(
            out,
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        )?;
    }
    Ok(())
}

/// 开着键盘协议的：关掉（交还终端以前）。
fn pop_keyboard(out: &mut impl Write) -> io::Result<()> {
    if KEYBOARD.load(Ordering::Relaxed) {
        execute!(out, PopKeyboardEnhancementFlags)?;
    }
    Ok(())
}

/// 全屏期间拿着它；放掉的时候把终端还回原样。
pub struct Fullscreen;

impl Fullscreen {
    /// 进全屏。崩的时候也先把终端还回去，再照原来的样子报。
    ///
    /// # Errors
    ///
    /// 标准输出不是终端，或者终端不让改模式。
    pub fn enter() -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        execute!(
            io::stdout(),
            EnterAlternateScreen,
            EnableBracketedPaste,
            EnableMouseCapture
        )?;
        let report = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| on_panic(info, report.as_ref())));
        Ok(Self)
    }
}

/// 崩了的时候（[`Fullscreen::enter`] 装的钩子）：先把终端还回原样，再记下崩在哪（`crash.rs`），终端里照旧用原来的钩子
/// （`report`）打出报错，另说一句记在哪。
pub fn on_panic(info: &PanicHookInfo<'_>, report: &(dyn Fn(&PanicHookInfo<'_>) + Send + Sync)) {
    restore();
    let kept = crate::crash::record(info);
    report(info);
    if let Some(path) = kept {
        eprintln!("litemiyu: crash recorded in {}", path.display());
    }
}

/// 挂起到后台（`Ctrl+Z`，Miyu 蓝图「按键」）：把终端还原（鼠标、粘贴、全屏、原始模式都退掉），给自己发 `SIGTSTP`；`fg` 回来
/// 以后重新进全屏。引擎线程跟着整个进程停，回来接着走。交回以后下一帧要整屏重画（`Screen::resize`）。
///
/// # Errors
///
/// 发不了信号、回来以后进不了全屏。
pub fn suspend() -> io::Result<()> {
    restore();
    rustix::process::kill_process(rustix::process::getpid(), rustix::process::Signal::TSTP)?;
    // 到这里已经 `fg` 回来了（没有作业控制的会话里信号被丢掉，当场回来）。
    terminal::enable_raw_mode()?;
    execute!(
        io::stdout(),
        EnterAlternateScreen,
        EnableBracketedPaste,
        EnableMouseCapture,
        terminal::Clear(terminal::ClearType::All)
    )?;
    push_keyboard(&mut io::stdout())
}

/// 让出终端给编辑器（Miyu 终端 `main.rs` 的 `edit`）：读按键的停下，关掉鼠标、粘贴和原始模式；不离开备用屏（离开会露出
/// 底下 shell 的内容，光标先飞到左下角再飞进编辑器），光标藏起来等编辑器自己画上来。编辑器退出了回全屏（它自己离开了
/// 备用屏的马上进回去）、接着读。交回编辑器跑没跑起来；回全屏回不去的是错误。
///
/// # Errors
///
/// 回不去全屏。
pub fn edit(
    keys: &crate::reader::Reader,
    editor: &str,
    file: &std::path::Path,
) -> io::Result<io::Result<()>> {
    keys.pause();
    let mut out = io::stdout();
    if let Err(error) = crate::pointer::reset(&mut out) {
        eprintln!("litemiyu: pointer not restored: {error}");
    }
    // 像素坐标关掉（Miyu 终端 `main.rs` 的 `leave`）。
    crate::pixel_mouse::PixelMouse::default().reset(&mut out)?;
    pop_keyboard(&mut out)?;
    execute!(
        out,
        DisableMouseCapture,
        DisableBracketedPaste,
        cursor::Hide
    )?;
    terminal::disable_raw_mode()?;
    let ran = crate::editor::run(editor, file);
    terminal::enable_raw_mode()?;
    execute!(
        io::stdout(),
        EnterAlternateScreen,
        EnableBracketedPaste,
        EnableMouseCapture,
        terminal::Clear(terminal::ClearType::All)
    )?;
    push_keyboard(&mut io::stdout())?;
    keys.resume();
    Ok(ran)
}

impl Drop for Fullscreen {
    fn drop(&mut self) {
        restore();
    }
}

/// 把终端还回原样。还不回去也没处可报（正在退出），照做能做的。
fn restore() {
    let mut out = io::stdout();
    // 鼠标指针变过手的，还原成终端默认的样子。
    if let Err(error) = crate::pointer::reset(&mut out) {
        eprintln!("litemiyu: pointer not restored: {error}");
    }
    // 拖滚动条时开的像素坐标关掉（Miyu 终端 `main.rs` 的 `leave`）。
    if let Err(error) = crate::pixel_mouse::PixelMouse::default().reset(&mut out) {
        eprintln!("litemiyu: pixel mouse not left: {error}");
    }
    if let Err(error) = pop_keyboard(&mut out) {
        eprintln!("litemiyu: keyboard protocol not left: {error}");
    }
    if let Err(error) = execute!(
        out,
        DisableMouseCapture,
        DisableBracketedPaste,
        LeaveAlternateScreen,
        cursor::Show
    ) {
        // 备用屏没退出来，这一行人多半看不到，但错误不吞。
        eprintln!("litemiyu: terminal not restored: {error}");
    }
    if let Err(error) = terminal::disable_raw_mode() {
        eprintln!("litemiyu: raw mode not left: {error}");
    }
    if let Err(error) = out.flush() {
        eprintln!("litemiyu: terminal not flushed: {error}");
    }
}
