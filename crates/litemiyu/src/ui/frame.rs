//! 一帧的画布：往缓冲里画部件。Miyu 用 ratatui 的 `Frame`（它只有 `Terminal` 造得出来）；LiteMiyu 不用 `Terminal`
//! （`screen.rs`），画的函数照旧拿一个 `Frame`，底下是这一帧的缓冲。

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::Widget;

/// 这一帧：整块缓冲。
pub struct Frame<'a> {
    /// 往这里画。
    buffer: &'a mut Buffer,
}

impl<'a> Frame<'a> {
    /// 往 `buffer` 里画的一帧。
    pub fn new(buffer: &'a mut Buffer) -> Self {
        Self { buffer }
    }

    /// 整屏多大。
    pub fn area(&self) -> Rect {
        self.buffer.area
    }

    /// 把部件 `widget` 画在 `area` 里。
    pub fn render_widget<W: Widget>(&mut self, widget: W, area: Rect) {
        widget.render(area, self.buffer);
    }

    /// 直接改格子。
    pub fn buffer_mut(&mut self) -> &mut Buffer {
        self.buffer
    }
}
