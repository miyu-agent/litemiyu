//! 侧边栏一段一段往下排（Miyu 终端 `ui/layout/slot.rs` 的 `stack`，蓝图「挂载位」第 4 条、排期第 14 条）：几段照交进来的先后
//! 从顶上往下排，段和段之间空 `gap` 行，没东西的段不占行、也不留空行；比侧边栏高的从第 `top` 行露起（像正文那样滚）。
//!
//! 照字画出来的每一行连同它在屏幕上的位置一并交回（Miyu 的 `written`），侧边栏选字用（`side_select.rs`）；路径这类放不下
//! 折下来的行段自己说（Miyu 的 `continues`），复制时接回上一行。
//!
//! 和 Miyu 不一样的：LiteMiyu 没有挂载位的配置（排先后、藏、截行数，Miyu `config/slots.rs`）、没有自己画的段（头像）、后面不接
//! 扩展包的界面块（Miyu 的 `next`），这几样裁掉。

use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;

use crate::ui::Frame;

/// 侧边栏的一段。`C` 是画的时候要用的东西（`parts::Side`）。
pub trait Part<C> {
    /// 照宽度排出这一段的几行；空的是这一回没东西画。
    fn lay(&mut self, ctx: &C, width: u16) -> Vec<Line<'static>>;
    /// 画好了：`area` 是给它露出来的那一块，`first` 是这一块的第一行是它自己的第几行（往下滚过、上面几行滚出去的不是 0），
    /// `shown` 是它自己那几行露出来几行。点击区照这里的位置记。
    fn placed(&mut self, _area: Rect, _rows: (usize, usize), _ctx: &mut C) {}
    /// 第 `row` 行是不是上一行放不下折下来的（软折行）：侧边栏复制时接回上一行，不换行（Miyu 发版前 check 29）。
    fn continues(&self, _row: usize) -> bool {
        false
    }
}

/// 照字画出来的一行：（行、列、字、是不是上一行折下来的），侧边栏选字用。
pub type Written = (u16, u16, String, bool);

/// 排完了：照字画出来的那几行、限过的偏移、一共多高。
pub struct Stacked {
    /// 照字画出来的每一行。
    pub written: Vec<Written>,
    /// 从第几行露起：交进来的限在 0 到「一共多高减一窗」之间。
    pub top: u16,
    /// 排好的一共多高（照它画滚动条）。
    pub total: usize,
}

/// 把 `parts` 照先后从 `area` 顶上往下排，段和段之间空 `gap` 行，从第 `top` 行露起；露不出来的截掉。先把每段排好、算出一共
/// 多高、限住 `top`，再画露出来的那一截。
pub fn stack<C>(
    frame: &mut Frame,
    area: Rect,
    parts: &mut [&mut dyn Part<C>],
    (gap, top): (u16, u16),
    ctx: &mut C,
) -> Stacked {
    // 先排：（第几段、排好的）。
    let mut laid = Vec::new();
    for (at, part) in parts.iter_mut().enumerate() {
        let lines = part.lay(ctx, area.width);
        if !lines.is_empty() {
            laid.push((at, lines));
        }
    }
    let gap = usize::from(gap);
    let total: usize = laid.iter().map(|(_, lines)| lines.len()).sum::<usize>()
        + gap * laid.len().saturating_sub(1);
    let room = usize::from(area.height);
    let top = usize::from(top).min(total.saturating_sub(room));
    let mut written = Vec::new();
    // 照排好的算的那一行（不看滚了多少）。
    let mut y = 0;
    for (k, (at, lines)) in laid.into_iter().enumerate() {
        if k > 0 {
            y += gap;
        }
        let height = lines.len();
        // 这一段露出来的是它自己的第 `from` 到 `to` 行。
        let from = top.saturating_sub(y).min(height);
        let to = (top + room).saturating_sub(y).min(height);
        if from < to {
            let row = area.y + u16::try_from(y + from - top).unwrap_or(u16::MAX);
            let tall = u16::try_from(to - from).unwrap_or(u16::MAX);
            let rect = Rect::new(area.x, row, area.width, tall);
            let shown: Vec<Line<'static>> = lines[from..to].to_vec();
            let part = &parts[at];
            written.extend(
                shown
                    .iter()
                    .zip(rect.y..)
                    .zip(from..)
                    .map(|((line, row), own)| (row, rect.x, line.to_string(), part.continues(own))),
            );
            frame.render_widget(Paragraph::new(shown), rect);
            parts[at].placed(rect, (from, to - from), ctx);
        }
        y += height;
    }
    Stacked {
        written,
        top: u16::try_from(top).unwrap_or(u16::MAX),
        total,
    }
}

#[cfg(test)]
mod tests;
