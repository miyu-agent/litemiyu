//! 空会话的首页（蓝图 `tui.md`「空会话的首页」）：输入框 · 框下面那一行 · 空一行 · 工作目录，整组上下居中。Miyu 的首页
//! 上面还有吉祥物（LiteMiyu 不要）。

use crate::ui::Frame;
use ratatui::layout::Rect;

use super::Areas;
use crate::app::Seen;
use crate::config::Layout;

/// 首页输入框里放字的宽度：框收窄到 `home_max_width`（「空会话的首页」第 2 条）。输入框上面那一块照它排，和框里的字一样宽。
pub fn text_width(area_width: u16, layout: &Layout) -> u16 {
    let width = super::box_width(area_width, layout).min(layout.home_max_width);
    let m = super::margins::margins(layout, area_width);
    width.saturating_sub(2 + m.pad_left + m.pad_right).max(1)
}

/// 首页各块的位置。`menu_rows` 是列表要露出几行（贴在输入框上面，盖住上面的东西，输入框不挪）；`above_rows` 是输入框上面那一块占几行。
pub fn areas(
    area: Rect,
    rows: &dyn Fn(u16) -> u16,
    layout: &Layout,
    menu_rows: u16,
    above_rows: u16,
) -> Areas {
    // 首页的输入框收窄（「空会话的首页」第 2 条）。
    let width = super::box_width(area.width, layout).min(layout.home_max_width);
    let x = area.x + (area.width - width) / 2;
    let m = super::margins::margins(layout, area.width);
    let text_width = text_width(area.width, layout);
    let rows = rows(text_width);
    // 输入框（上下两条边和字）和框下面那一行一直在；上面那一块连下面的空行。
    let above_h = if above_rows > 0 { above_rows + 1 } else { 0 };
    // 框下面那一行下面：空一行、工作目录（第 2 条）。
    let total = rows + 2 + 1 + 2 + above_h;
    let mut y = area.y + area.height.saturating_sub(total) / 2;
    // 开着列表、输入框上面放不下：整组往下挪，挪到底为止（「窗口小的时候」第 1 条）。
    let lowest = area.bottom().saturating_sub(total);
    y = y.max((area.y + menu_rows).min(lowest));
    let frame = Rect::new(x, y + above_h, width, rows + 2).intersection(area);
    let inner_x = frame.x + 1 + m.pad_left;
    let text = Rect::new(inner_x, frame.y + 1, text_width, rows).intersection(frame);
    let footer = Rect::new(inner_x, frame.bottom(), text_width, 1).intersection(area);
    let cwd = Rect::new(inner_x, footer.bottom() + 1, text_width, 1).intersection(area);
    // 列表只用输入框上面剩下的行（「窗口小的时候」第 1 条）。
    let (menu_y, menu_rows) = super::above(frame.y, area.y, menu_rows);
    let menu = Rect::new(frame.x, menu_y, frame.width, menu_rows);
    // 上面那一块在列表（没开时是输入框）上面，下面空一行。
    let (above_y, above_rows) = super::above(menu_y.saturating_sub(1), area.y, above_rows);
    let above_y = if above_rows > 0 { above_y } else { menu_y };
    let above = Rect::new(inner_x, above_y, text_width, above_rows);
    // 没有正文：高是 0，底边在输入框（开着列表时是列表）上面一行，提示照它浮着（「提示」）。
    let body = Rect::new(frame.x, above_y.saturating_sub(1), frame.width, 0);
    Areas {
        body,
        menu,
        menu_text: super::list_text(menu, inner_x, text_width, layout),
        frame,
        text,
        footer,
        cwd,
        above,
        ..Areas::default()
    }
}

/// 画工作目录：暗，放不下从前面截掉（第 2 条）。输入框、框下面那一行照平常画。
pub fn draw(frame: &mut Frame, areas: Areas, seen: &Seen) {
    let cwd = super::panel::tail(seen.cwd, usize::from(areas.cwd.width));
    let line = ratatui::text::Line::from(ratatui::text::Span::styled(cwd, crate::theme::dim()));
    frame.render_widget(ratatui::widgets::Paragraph::new(line), areas.cwd);
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use ratatui::layout::Rect;

    use super::areas;
    use crate::config::Config;
    use crate::input::InputBox;

    fn input() -> InputBox {
        InputBox::new(8, Duration::from_millis(400))
    }

    #[test]
    fn the_whole_group_is_centred_vertically() {
        let layout = Config::builtin().unwrap().layout;
        let a = areas(
            Rect::new(0, 0, 100, 40),
            &|w| input().rows(w),
            &layout,
            0,
            0,
        );
        // 输入框 3 · 框下面 1 · 空 · 工作目录：共 6 行，上面空 (40 − 6) ÷ 2 = 17 行。
        assert_eq!(a.frame.y, 17);
        assert_eq!(a.frame.height, 3);
        assert_eq!(a.footer.y, a.frame.bottom());
        assert_eq!(a.cwd.y, a.footer.bottom() + 1, "框下面那一行下面空一行");
        assert_eq!((a.cwd.x, a.cwd.height), (a.footer.x, 1), "和框里的字左对齐");
        assert_eq!(40 - a.cwd.bottom(), 17);
    }

    #[test]
    fn the_home_box_is_at_most_home_max_width() {
        let layout = Config::builtin().unwrap().layout;
        let a = areas(
            Rect::new(0, 0, 200, 40),
            &|w| input().rows(w),
            &layout,
            0,
            0,
        );
        assert_eq!(a.frame.width, layout.home_max_width, "宽窗口里收窄");
        assert_eq!(a.frame.x, (200 - layout.home_max_width) / 2, "左右居中");
        let b = areas(Rect::new(0, 0, 60, 40), &|w| input().rows(w), &layout, 0, 0);
        assert!(b.frame.width < layout.home_max_width, "窄窗口照平常的宽度");
    }

    #[test]
    fn the_home_block_above_is_laid_out_as_wide_as_the_home_box() {
        // 首页的输入框收窄了，上面那一块照首页框里的字宽排（2026-09-30 查到：原来照正文的宽度排，长的一行被框的宽度裁掉）。
        let layout = Config::builtin().unwrap().layout;
        let area = Rect::new(0, 0, 140, 40);
        let a = areas(area, &|w| input().rows(w), &layout, 0, 6);
        assert_eq!(a.above.width, super::text_width(area.width, &layout));
        assert!(
            super::text_width(area.width, &layout) < super::super::text_width(area.width, &layout),
            "宽窗口下首页比正文窄"
        );
    }

    #[test]
    fn the_block_above_sits_above_the_box() {
        let layout = Config::builtin().unwrap().layout;
        let a = areas(
            Rect::new(0, 0, 100, 40),
            &|w| input().rows(w),
            &layout,
            0,
            6,
        );
        // 上面那一块 6 · 空 · 输入框 3 · 框下面 1 · 空 · 工作目录：共 13 行。
        assert_eq!(a.above.y, (40 - 13) / 2);
        assert_eq!((a.above.height, a.above.bottom() + 1), (6, a.frame.y));
        assert_eq!(a.above.x, a.text.x, "和框里的字左对齐");
        // 开着列表：上面那一块在列表上面，输入框不挪。
        let b = areas(
            Rect::new(0, 0, 100, 40),
            &|w| input().rows(w),
            &layout,
            4,
            6,
        );
        assert_eq!(b.frame, a.frame);
        assert_eq!(b.above.bottom() + 1, b.menu.y);
    }

    #[test]
    fn a_long_directory_is_cut_from_the_front() {
        assert_eq!(
            crate::ui::panel::tail("~/workspace/source/miyu", 10),
            "…urce/miyu"
        );
        assert_eq!(crate::ui::panel::tail("~/src", 10), "~/src");
    }

    #[test]
    fn a_list_leaves_the_box_alone() {
        let layout = Config::builtin().unwrap().layout;
        let closed = areas(
            Rect::new(0, 0, 100, 40),
            &|w| input().rows(w),
            &layout,
            0,
            0,
        );
        let open = areas(
            Rect::new(0, 0, 100, 40),
            &|w| input().rows(w),
            &layout,
            5,
            0,
        );
        assert_eq!(open.frame, closed.frame, "输入框不挪");
        assert_eq!((open.menu.y, open.menu.height), (closed.frame.y - 5, 5));
    }
}
