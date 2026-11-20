//! 屏幕：把一帧画进缓冲，只把变了的行写给终端。
//!
//! 不用 ratatui 的 `Terminal`：它留着这一帧、上一帧两整份屏幕来逐格比，一格约五十字节，160×45 的终端就是
//! 0.75 MB，跟着终端大小涨，是空闲时匿名内存里最大的一块（`docs/00-设计.md` 第七节「量过的地板」第 3 条）。
//! 这里上一帧只留每一行的指纹，一行八个字节；这一帧的缓冲画完就放掉。哪一行的指纹变了，整行重写。
//!
//! 一帧攒成一份字节，套着同步输出（DEC 2026）的首尾一次写出去，终端不会画出半帧。
//!
//! 光标（同 miyu-tui `caret.rs`）：不每帧先藏再显，只在显不显示变了时写藏、显（输入法的预编辑挂在光标上，跟着藏/显会和
//! 输入框里的提示来回闪，Miyu 2026-10-02 项目主人报的 fcitx5）；藏着时也挪到插入点（不挪的话停在这一帧最后写的格子，
//! 转圈、数字变时跟着跳，开了拖尾的终端照样画拖尾）。

use std::hash::{BuildHasher, Hasher};
use std::io::{self, Write};

use ratatui::buffer::{Buffer, Cell};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier};
use unicode_width::UnicodeWidthStr;

/// 同步输出的开头：终端攒着，到结尾再一起画。
const BEGIN: &[u8] = b"\x1b[?2026h";
/// 同步输出的结尾。
const END: &[u8] = b"\x1b[?2026l";
/// 属性全部还原。
const RESET: &[u8] = b"\x1b[0m";
/// 藏起光标。
const HIDE: &[u8] = b"\x1b[?25l";
/// 显示光标。
const SHOW: &[u8] = b"\x1b[?25h";

/// 一格的样子：前景、背景、修饰。
type Look = (Color, Color, Modifier);

/// 屏幕。`W` 是往哪写：真的终端是标准输出，测试里是一块内存。
pub struct Screen<W> {
    /// 往哪写。
    out: W,
    /// 上一帧每一行的指纹；空的就是还没画过，下一帧每一行都写。
    rows: Vec<u64>,
    /// 宽，格。
    width: u16,
    /// 高，格。
    height: u16,
    /// 终端上光标现在显不显示；`None` 是不知道（还没画过、换过大小、让出过终端），下一帧照要的写一次。
    cursor: Option<bool>,
}

impl<W: Write> Screen<W> {
    /// 造一块屏幕，还没画过。
    pub fn new(out: W, width: u16, height: u16) -> Self {
        Self {
            out,
            rows: Vec::new(),
            width,
            height,
            cursor: None,
        }
    }

    /// 终端换了大小：记下新的，下一帧每一行都重写。
    pub fn resize(&mut self, width: u16, height: u16) {
        self.width = width;
        self.height = height;
        self.rows.clear();
        self.cursor = None;
    }

    /// 画一帧。`paint` 往缓冲里画，交回这一帧的光标：停在哪、显不显示。
    ///
    /// # Errors
    ///
    /// 往终端写不进去。
    pub fn draw(
        &mut self,
        paint: impl FnOnce(&mut Buffer) -> crate::caret::Caret,
    ) -> io::Result<()> {
        let mut buffer = Buffer::empty(Rect::new(0, 0, self.width, self.height));
        let caret = paint(&mut buffer);
        let width = usize::from(self.width);
        let fresh = self.rows.len() != usize::from(self.height);
        if fresh {
            self.rows = vec![0; usize::from(self.height)];
        }
        let mut bytes = Vec::new();
        bytes.extend_from_slice(BEGIN);
        if !caret.shown && self.cursor != Some(false) {
            bytes.extend_from_slice(HIDE);
        }
        for (y, cells) in buffer.content.chunks(width.max(1)).enumerate() {
            let print = fingerprint(cells);
            if let Some(last) = self.rows.get_mut(y)
                && (fresh || *last != print)
            {
                *last = print;
                row(&mut bytes, y, cells)?;
            }
        }
        let (x, y) = (caret.at.x, caret.at.y);
        write!(bytes, "\x1b[{};{}H", u32::from(y) + 1, u32::from(x) + 1)?;
        if caret.shown && self.cursor != Some(true) {
            bytes.extend_from_slice(SHOW);
        }
        self.cursor = Some(caret.shown);
        bytes.extend_from_slice(END);
        self.out.write_all(&bytes)?;
        self.out.flush()
    }
}

/// 算行指纹的哈希：foldhash 的快速版，种子固定（同样的一行每帧算出来一样）。没用标准库默认的 SipHash：140×40 的屏一帧要喂
/// 两万多次，转圈、流式那种帧里它一项占界面线程的三分之一（占用线量的）；这里只比这一行和上一帧
/// 同一行变没变，不用防人造碰撞。foldhash 是 ratatui 经 hashbrown 本来就带进来的同一版。
const PRINT: foldhash::fast::FixedState = foldhash::fast::FixedState::with_seed(0);

/// 一行的指纹：每一格的字、前景、背景、修饰。
///
/// 一格拼成两个数喂进去（前景和背景；修饰、字长和字的头四个字节），字长过四个字节的再喂后面那截：不同的格喂进去的一定
/// 不同。没照 `Hash` 一样样喂：那样一格要喂六七次、颜色每次都是一趟不内联的调用，换了快哈希以后它还占转圈那种帧的
/// 两成（占用线量的）。
fn fingerprint(cells: &[Cell]) -> u64 {
    let mut hasher = PRINT.build_hasher();
    for cell in cells {
        let symbol = cell.symbol().as_bytes();
        hasher.write_u64(code(cell.fg) | code(cell.bg) << 32);
        let mut head = [0u8; 4];
        let shown = symbol.len().min(head.len());
        head[..shown].copy_from_slice(&symbol[..shown]);
        let length = u64::try_from(symbol.len()).map_or(0xffff, |n| n.min(0xffff));
        hasher.write_u64(
            u64::from(cell.modifier.bits())
                | length << 16
                | u64::from(u32::from_le_bytes(head)) << 32,
        );
        if let Some(rest) = symbol.get(head.len()..).filter(|rest| !rest.is_empty()) {
            hasher.write(rest);
        }
    }
    hasher.finish()
}

/// 一种颜色记成一个数，不同的颜色不同（25 位以内）：默认 0，十六色 1–16，256 色 `0x100` 加序号，真彩色 `0x100_0000` 加 RGB。
fn code(color: Color) -> u64 {
    match color {
        Color::Reset => 0,
        Color::Black => 1,
        Color::Red => 2,
        Color::Green => 3,
        Color::Yellow => 4,
        Color::Blue => 5,
        Color::Magenta => 6,
        Color::Cyan => 7,
        Color::Gray => 8,
        Color::DarkGray => 9,
        Color::LightRed => 10,
        Color::LightGreen => 11,
        Color::LightYellow => 12,
        Color::LightBlue => 13,
        Color::LightMagenta => 14,
        Color::LightCyan => 15,
        Color::White => 16,
        Color::Indexed(index) => 0x100 | u64::from(index),
        Color::Rgb(r, g, b) => 0x100_0000 | u64::from(r) << 16 | u64::from(g) << 8 | u64::from(b),
    }
}

/// 把一整行写进这一帧的字节：挪到行首，一格格写，样子变了才换属性，末尾还原。
fn row(bytes: &mut Vec<u8>, y: usize, cells: &[Cell]) -> io::Result<()> {
    write!(bytes, "\x1b[{};1H", y + 1)?;
    bytes.extend_from_slice(RESET);
    let mut look: Look = (Color::Reset, Color::Reset, Modifier::empty());
    // 宽字占两格：写了它，后面那一格不写。
    let mut covered = 0usize;
    for cell in cells {
        if covered > 0 {
            covered -= 1;
            continue;
        }
        let next = (cell.fg, cell.bg, cell.modifier);
        if next != look {
            attributes(bytes, next)?;
            look = next;
        }
        let symbol = cell.symbol();
        bytes.extend_from_slice(symbol.as_bytes());
        covered = symbol.width().saturating_sub(1);
    }
    bytes.extend_from_slice(RESET);
    Ok(())
}

/// 换成这一格的样子：先全部还原，再一样样设上。
fn attributes(bytes: &mut Vec<u8>, (fg, bg, modifier): Look) -> io::Result<()> {
    bytes.extend_from_slice(b"\x1b[0");
    const MODIFIERS: [(Modifier, u8); 9] = [
        (Modifier::BOLD, 1),
        (Modifier::DIM, 2),
        (Modifier::ITALIC, 3),
        (Modifier::UNDERLINED, 4),
        (Modifier::SLOW_BLINK, 5),
        (Modifier::RAPID_BLINK, 6),
        (Modifier::REVERSED, 7),
        (Modifier::HIDDEN, 8),
        (Modifier::CROSSED_OUT, 9),
    ];
    for (flag, code) in MODIFIERS {
        if modifier.contains(flag) {
            write!(bytes, ";{code}")?;
        }
    }
    color(bytes, fg, 30)?;
    color(bytes, bg, 40)?;
    bytes.push(b'm');
    Ok(())
}

/// 一种颜色的属性码。`base` 是 30（前景）或 40（背景）。
fn color(bytes: &mut Vec<u8>, color: Color, base: u8) -> io::Result<()> {
    let plain = |index: u8| base + index;
    let bright = |index: u8| base + 60 + index;
    let code = match color {
        Color::Reset => return Ok(()),
        Color::Black => plain(0),
        Color::Red => plain(1),
        Color::Green => plain(2),
        Color::Yellow => plain(3),
        Color::Blue => plain(4),
        Color::Magenta => plain(5),
        Color::Cyan => plain(6),
        Color::Gray => plain(7),
        Color::DarkGray => bright(0),
        Color::LightRed => bright(1),
        Color::LightGreen => bright(2),
        Color::LightYellow => bright(3),
        Color::LightBlue => bright(4),
        Color::LightMagenta => bright(5),
        Color::LightCyan => bright(6),
        Color::White => bright(7),
        Color::Indexed(index) => return write!(bytes, ";{};5;{index}", base + 8),
        Color::Rgb(r, g, b) => return write!(bytes, ";{};2;{r};{g};{b}", base + 8),
    };
    write!(bytes, ";{code}")
}

#[cfg(test)]
mod tests;
