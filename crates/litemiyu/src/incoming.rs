//! 主循环等的东西（从 `main.rs` 分出来）：终端的事件、引擎的消息；一条条交给界面。

use std::io;

use crossterm::event::Event;

use crate::app::App;
use crate::core::Update;
use crate::screen::Screen;

/// 主循环等的东西：终端的事件，或者引擎的消息。
pub enum Incoming {
    /// 终端的事件；读不了终端的是错误。
    Terminal(io::Result<Event>),
    /// 引擎的消息。
    Core(Update),
    /// 终端回了它的字色或者底色（`theme/answer.rs`）。
    Color(crate::theme::Which, crate::theme::Rgb),
    /// 要问终端认不认 kitty 键盘协议了（`reader.rs`）：这一刻主线程不在写终端。
    Keyboard(crate::reader::Ask),
}

impl Incoming {
    /// 按了键、粘贴的：不等这一拍就画（`pacing.rs`）。
    pub fn urgent(&self) -> bool {
        matches!(self, Incoming::Terminal(Ok(event)) if crate::pacing::urgent(event))
    }
}

/// 收一条：终端的事件、引擎的消息。终端换了大小的，屏幕先记下新的，下一帧整屏重写。
///
/// # Errors
///
/// 终端读不了了：没法再交互，主循环退出。
pub fn dispatch<W: io::Write>(
    app: &mut App,
    screen: &mut Screen<W>,
    message: Incoming,
) -> io::Result<()> {
    match message {
        Incoming::Terminal(Ok(event)) => {
            if let Event::Resize(width, height) = event {
                screen.resize(width, height);
                // 换了大小、缩放了字体：一格多少像素重新问（拖滚动条按像素算，审计 #067）。
                app.screen.pixel.cell = crate::pixel_mouse::cell();
            }
            app.handle(event);
        }
        Incoming::Terminal(Err(error)) => return Err(error),
        Incoming::Core(update) => app.core(update),
        Incoming::Color(which, rgb) => app.terminal_color(which, rgb),
        // 认得的打开（同 miyu-tui `main.rs` 的 `modes_on`）；退出、挂起、让出终端时关掉（`terminal.rs`）。
        Incoming::Keyboard(ask) => {
            if ask.answer() {
                crate::terminal::keyboard_on()?;
            }
        }
    }
    Ok(())
}
