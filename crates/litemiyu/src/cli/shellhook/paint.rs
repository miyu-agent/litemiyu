//! 印在 shell 里（施工线 shellhook，底子是 Miyu 终端的 `shellhook/say/paint.rs`）：不进全屏，光标下面一块原地刷新的活动区，
//! 定下来的行印上去就留在终端的滚动历史里（坑 P3）。
//!
//! 每一帧先画进一块和终端一样宽的画布，照全屏那一套收拾（宽字后面那格、照色深换色），再一行行写成带颜色的字；不铺底色
//! （滚动历史里铺一块底很怪，同 Miyu）；
//! 擦上一帧和画这一帧包在同一段同步输出里，不闪（坑 P2）。活动区有多高由调的一方管（最多终端高减一行，坑 P4）。

use std::io::{self, Write};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::Line;

use crate::theme;

/// 同步输出的开头、结尾：终端攒着，到结尾再一起画。
const BEGIN: &str = "\x1b[?2026h";
const END: &str = "\x1b[?2026l";
/// 藏起、显示光标。
const HIDE: &str = "\x1b[?25l";
const SHOW: &str = "\x1b[?25h";

/// 活动区里写字的那一格：第几行、第几列，光标亮在那里（答话时的输入行、抽屉里写字的地方：输入法的候选框跟着它，
/// 坑 P9、P10）。没有的光标停在活动区最后一行、不亮。
pub type Caret = Option<(usize, u16)>;

/// 往终端写的那一头：活动区现在占了几行、光标停在它的第几行。
pub struct Painter<W: Write> {
    out: W,
    /// 活动区占的行数；没有活动区是 0。
    live: u16,
    /// 光标停在活动区的第几行。
    at: u16,
    /// 写字的那一格亮着光标。
    shown: bool,
    /// 上一帧的活动区和光标：一样的不再画（重画时光标要回上面去，输入法的候选框跟着跳）。
    last: Option<(Vec<String>, Caret)>,
}

impl<W: Write> Painter<W> {
    /// 往 `out` 写。
    pub fn new(out: W) -> Painter<W> {
        Painter {
            out,
            live: 0,
            at: 0,
            shown: false,
            last: None,
        }
    }

    /// 换掉活动区：擦掉上一帧的，`settled` 印上去留下，`live` 画在下面、下一帧再换；`caret` 是活动区里亮光标的那一格。
    ///
    /// # Errors
    ///
    /// 写终端失败。
    pub fn frame(&mut self, settled: &[String], live: &[String], caret: Caret) -> io::Result<()> {
        if settled.is_empty()
            && self
                .last
                .as_ref()
                .is_some_and(|(lines, at)| lines.as_slice() == live && *at == caret)
        {
            return Ok(());
        }
        self.last = Some((live.to_vec(), caret));
        let mut bytes = String::from(BEGIN);
        // 重画时先藏起光标，画完再亮在写字的那一格：中途挪来挪去不露出来。
        if self.shown {
            bytes.push_str(HIDE);
        }
        self.erase(&mut bytes);
        for line in settled {
            bytes.push_str(line);
            bytes.push_str("\r\n");
        }
        for (index, line) in live.iter().enumerate() {
            if index > 0 {
                bytes.push_str("\r\n");
            }
            bytes.push_str(line);
        }
        self.live = u16::try_from(live.len()).unwrap_or(u16::MAX);
        self.at = self.live.saturating_sub(1);
        match caret.and_then(|(row, column)| Some((u16::try_from(row).ok()?, column))) {
            Some((row, column)) if row < self.live => {
                if self.at > row {
                    bytes.push_str(&format!("\x1b[{}A", self.at - row));
                }
                bytes.push_str(&format!("\x1b[{}G", u32::from(column) + 1));
                self.at = row;
                bytes.push_str(SHOW);
                self.shown = true;
            }
            _ if self.shown => {
                bytes.push_str(HIDE);
                self.shown = false;
            }
            _ => {}
        }
        bytes.push_str(END);
        self.out.write_all(bytes.as_bytes())?;
        self.out.flush()
    }

    /// 收尾：擦掉活动区，光标停在新的一行开头，交还给 shell。光标亮不亮由脚本画提示符时定（坑 S12）。
    ///
    /// # Errors
    ///
    /// 写终端失败。
    pub fn finish(&mut self) -> io::Result<()> {
        let mut bytes = String::from(BEGIN);
        self.erase(&mut bytes);
        self.last = None;
        if std::mem::take(&mut self.shown) {
            bytes.push_str(HIDE);
        }
        bytes.push_str("\x1b[0m");
        bytes.push_str(END);
        self.out.write_all(bytes.as_bytes())?;
        self.out.flush()
    }

    /// 擦掉活动区：回到它的第一行开头，往下全清。
    fn erase(&mut self, bytes: &mut String) {
        if self.live == 0 {
            return;
        }
        bytes.push('\r');
        if self.at > 0 {
            bytes.push_str(&format!("\x1b[{}A", self.at));
        }
        bytes.push_str("\x1b[J");
        self.live = 0;
        self.at = 0;
    }
}

/// 画好的这几行照全屏那一套收拾成带颜色的字：画进一块 `width` 宽的画布，宽字后面那格清空、照色深换色，再一行行
/// 写出来（宽字只写一次，行尾没颜色的空格不写）。
pub fn lines(width: u16, lines: &[Line<'_>]) -> Vec<String> {
    let height = u16::try_from(lines.len()).unwrap_or(u16::MAX);
    if width == 0 || height == 0 {
        return Vec::new();
    }
    let mut buffer = Buffer::empty(Rect::new(0, 0, width, height));
    for (y, line) in (0..height).zip(lines) {
        buffer.set_line(0, y, line, width);
    }
    rendered(&mut buffer)
}

/// 画好的一块画布收拾成带颜色的字，一行一份。
pub fn rendered(buffer: &mut Buffer) -> Vec<String> {
    crate::ui::wide::tidy(buffer);
    theme::degrade(buffer);
    let width = usize::from(buffer.area.width).max(1);
    buffer
        .content
        .chunks(width)
        .map(|cells| {
            let mut bytes = Vec::new();
            // 往内存里写不会错。
            drop(crate::screen::inline_row(&mut bytes, cells));
            String::from_utf8_lossy(&bytes).into_owned()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use ratatui::style::{Color, Style};
    use ratatui::text::{Line, Span};

    use super::{Painter, lines};

    /// 去掉颜色的控制序列，只看字。
    fn plain(text: &str) -> String {
        let mut out = String::new();
        let mut chars = text.chars();
        while let Some(c) = chars.next() {
            if c == '\u{1b}' {
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            } else {
                out.push(c);
            }
        }
        out
    }

    #[test]
    fn wide_characters_are_written_once_and_trailing_blanks_are_dropped() {
        let lines = lines(20, &[Line::from("你好 ok"), Line::default()]);
        assert_eq!(lines.len(), 2);
        assert_eq!(
            plain(&lines[0]),
            "你好 ok",
            "宽字后面那格不写，行尾的空格不写"
        );
        assert_eq!(plain(&lines[1]), "", "空行什么都不写");
    }

    #[test]
    fn colours_are_kept_per_span_and_reset_at_the_end() {
        let line = Line::from(vec![
            Span::styled("红", Style::default().fg(Color::Red)),
            Span::raw("字"),
        ]);
        let lines = lines(10, &[line]);
        assert!(lines[0].starts_with("\u{1b}[0;31m红"), "{:?}", lines[0]);
        assert!(
            lines[0].ends_with('字'),
            "回到没有颜色以后不用再还原：{:?}",
            lines[0]
        );
        assert_eq!(plain(&lines[0]), "红字");
    }

    #[test]
    fn a_frame_replaces_the_live_area_and_settled_lines_stay() {
        // 坑 P2、P3：擦和画包在同一段同步输出里；定下来的印上去留下。
        let mut painter = Painter::new(Vec::new());
        painter
            .frame(&[], &["a".into(), "b".into(), "c".into()], None)
            .unwrap();
        painter
            .frame(&["done".into()], &["x".into()], None)
            .unwrap();
        let out = String::from_utf8(std::mem::take(&mut painter.out)).unwrap();
        // 第二帧：回到活动区第一行（往上两行）、往下全清，定下的一行接着换行，再画新的活动区。
        let second = &out[out.find('c').unwrap()..];
        assert!(second.contains("\u{1b}[2A"), "往上回两行：{second:?}");
        assert!(second.contains("\u{1b}[J"), "往下全清：{second:?}");
        assert!(second.contains("done\r\nx"), "{second:?}");
        assert_eq!(
            out.matches("\u{1b}[?2026h").count(),
            2,
            "每一帧包在同步输出里"
        );
        assert_eq!(out.matches("\u{1b}[?2026l").count(), 2);
    }

    #[test]
    fn the_same_frame_is_not_painted_twice_and_the_caret_sits_where_asked() {
        let mut painter = Painter::new(Vec::new());
        let live = ["一".to_string(), "二".to_string(), "三".to_string()];
        painter.frame(&[], &live, Some((1, 4))).unwrap();
        let first = String::from_utf8(std::mem::take(&mut painter.out)).unwrap();
        // 光标从最后一行回到第 1 行（往上一行）、第 5 列，亮着（坑 P9、P10）。
        assert!(
            first.ends_with("\u{1b}[1A\u{1b}[5G\u{1b}[?25h\u{1b}[?2026l"),
            "{first:?}"
        );
        painter.frame(&[], &live, Some((1, 4))).unwrap();
        assert!(painter.out.is_empty(), "一样的一帧不再画");
        // 下一帧擦的时候从光标停的那一行往上回（一行），不是从最后一行。
        painter.frame(&[], &["四".into()], None).unwrap();
        let next = String::from_utf8(std::mem::take(&mut painter.out)).unwrap();
        assert!(next.contains("\r\u{1b}[1A\u{1b}[J四"), "{next:?}");
        assert!(
            next.contains("\u{1b}[?25l"),
            "不写字了，光标藏起来：{next:?}"
        );
        painter.finish().unwrap();
        let last = String::from_utf8(std::mem::take(&mut painter.out)).unwrap();
        assert!(last.contains("\r\u{1b}[J"), "收尾擦掉活动区：{last:?}");
    }
}
