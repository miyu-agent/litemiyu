//! 宽字后面那一格清成空白（蓝图 `tui.md`「每一帧」）：画完、交给终端之前过一遍。
//!
//! ratatui 比对两帧时认定宽字后面那格是空的；`Paragraph` 画宽字却不清后面那格，列表、面板盖在别的东西上时，
//! 缓冲区里就成了「宽字 + 横线」。等宽字被盖掉，比对只重写前半格，终端上留下半格空白或残色。
//! 清成的空白带着宽字的样子（底色）：终端上那格是宽字的后半、是宽字的颜色，记成默认的，下一帧那里也是默认的
//! 空白时比对不重写，留下一格残色（2026-10-01 命令列表选中行的蓝底）。

use ratatui::buffer::{Buffer, CellDiffOption};
use unicode_width::UnicodeWidthStr;

/// 每个宽字后面那几格不是空白的，照 ratatui 自己写宽字时的做法清掉。图占着的格子不碰。
///
/// 每一帧整屏走一遍，所以按行切片走、不按坐标一格格取（取一格要算一次下标、查一次界），常见的单个可见 ASCII 字不查宽度表
/// （占用线量的）。
pub fn tidy(buf: &mut Buffer) {
    let width = usize::from(buf.area.width);
    if width == 0 {
        return;
    }
    for row in buf.content.chunks_mut(width) {
        let mut x = 0;
        while x < row.len() {
            let wide = cells(row[x].symbol());
            if wide > 1 {
                let style = row[x].style();
                for next in row.iter_mut().skip(x + 1).take(wide - 1) {
                    if next.diff_option != CellDiffOption::Skip {
                        // 终端上这格是宽字的后半，带着宽字的颜色：照它记，盖掉宽字时比对才会重写这格。
                        next.reset();
                        next.set_style(style);
                    }
                }
            }
            x += wide.max(1);
        }
    }
}

/// 一格里的字占几列：单个可见的 ASCII 字是 1，别的查宽度表。
fn cells(symbol: &str) -> usize {
    match symbol.as_bytes() {
        [b' '..=b'~'] => 1,
        _ => symbol.width(),
    }
}

#[cfg(test)]
mod tests {
    use ratatui::buffer::Buffer;
    use ratatui::layout::Rect;
    use ratatui::style::{Color, Style};
    use ratatui::text::Line;
    use ratatui::widgets::{Paragraph, Widget};

    use super::tidy;

    #[test]
    fn a_wide_character_drawn_over_a_line_owns_the_next_cell() {
        let area = Rect::new(0, 0, 6, 1);
        let mut buf = Buffer::empty(area);
        buf.set_string(0, 0, "──────", Style::new().fg(Color::Blue));
        // Paragraph 画宽字不清后面那格：盖在横线上以后，「令」后面还是横线。
        Paragraph::new(Line::raw("命令")).render(Rect::new(1, 0, 4, 1), &mut buf);
        assert_eq!(
            buf[(2, 0)].symbol(),
            "─",
            "先证明 Paragraph 会留下这种缓冲区"
        );
        tidy(&mut buf);
        let row: Vec<&str> = (0..6).map(|x| buf[(x, 0)].symbol()).collect();
        assert_eq!(row, ["─", "命", " ", "令", " ", "─"]);
        // 照这样比对，盖回横线时两格都重写。
        let mut next = Buffer::empty(area);
        next.set_string(0, 0, "──────", Style::new().fg(Color::Blue));
        let written: Vec<u16> = buf.diff(&next).iter().map(|(x, _, _)| *x).collect();
        assert_eq!(written, [1, 2, 3, 4]);
    }

    #[test]
    fn the_cell_after_a_wide_character_keeps_its_background() {
        // 2026-10-01 项目主人报：`/restore` 回车以后通知框里留下一格蓝块，通知消失了还在。命令列表选中那一行
        // 是蓝底，宽字在终端上占两格、两格都是蓝底；后面那格在缓冲区里却记成默认的空白。列表收起、那里换成
        // 窄字和默认的空白，比对认为后面那格没变不重写，终端上就留着蓝的半格。
        let area = Rect::new(0, 0, 4, 1);
        let blue = Style::new().bg(Color::Blue);
        let mut buf = Buffer::empty(area);
        buf.set_string(0, 0, "恢 ", blue);
        tidy(&mut buf);
        assert_eq!(buf[(1, 0)].bg, Color::Blue, "后面那格照宽字的底色记");
        let mut next = Buffer::empty(area);
        next.set_string(0, 0, "x", Style::new());
        let written: Vec<u16> = buf.diff(&next).iter().map(|(x, _, _)| *x).collect();
        assert!(
            written.contains(&1),
            "盖掉宽字时后面那格也重写：{written:?}"
        );
    }

    #[test]
    fn a_wide_character_at_the_end_of_a_row_leaves_the_next_row_alone() {
        // 按行切片走（`tidy`）：宽字的后半落在行外的不往下一行清，画在屏幕中间一块的缓冲照样认行。
        let mut buf = Buffer::empty(Rect::new(0, 0, 3, 2));
        buf[(2, 0)].set_symbol("命");
        buf[(0, 1)].set_symbol("x");
        tidy(&mut buf);
        assert_eq!(buf[(0, 1)].symbol(), "x");
        let mut buf = Buffer::empty(Rect::new(5, 2, 4, 2));
        buf.set_string(5, 3, "────", Style::new());
        buf[(6, 3)].set_symbol("命");
        tidy(&mut buf);
        let row: Vec<&str> = (5..9).map(|x| buf[(x, 3)].symbol()).collect();
        assert_eq!(row, ["─", "命", " ", "─"]);
        assert_eq!(buf[(5, 2)].symbol(), " ");
    }

    #[test]
    fn cells_held_by_a_picture_are_left_alone() {
        let mut buf = Buffer::empty(Rect::new(0, 0, 3, 1));
        buf.set_string(0, 0, "图", Style::new());
        buf[(1, 0)]
            .set_symbol("x")
            .set_diff_option(ratatui::buffer::CellDiffOption::Skip);
        tidy(&mut buf);
        assert_eq!(buf[(1, 0)].symbol(), "x");
    }
}
