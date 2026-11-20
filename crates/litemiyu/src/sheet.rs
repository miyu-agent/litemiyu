//! 框线和记号：一张固定的表。Miyu 终端照主题包换（`sheet/`，读 JSON、盖几层、换了重画）；LiteMiyu 没有主题，照它出厂的默认
//! 主题包（`miyu-tui-theme-default/theme.json` 的 `frame`、`marks`）写成常量。

use ratatui::symbols::{border, line};

/// 一种框线。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Line {
    /// 圆角。
    Rounded,
    /// 直角细线。
    Plain,
}

impl Line {
    /// 画线用的那一套。
    pub const fn lines(self) -> line::Set<'static> {
        match self {
            Self::Rounded => line::ROUNDED,
            Self::Plain => line::NORMAL,
        }
    }

    /// 画框用的那一套。
    pub const fn set(self) -> border::Set<'static> {
        match self {
            Self::Rounded => border::ROUNDED,
            Self::Plain => border::PLAIN,
        }
    }
}

/// 各处的框线。
#[derive(Debug)]
pub struct Frames {
    /// 输入框。
    pub input: Line,
    /// 贴在输入框上面的那几样框。
    pub boxes: Line,
    /// 提示的小框。
    pub toast: Line,
    /// 代码块。
    pub code: Line,
    /// 表格。
    pub table: Line,
}

/// 记号。
#[derive(Debug)]
pub struct Marks {
    /// 选中那一行前面的。
    pub pointer: &'static str,
    /// 截掉的地方。
    pub ellipsis: &'static str,
    /// 插入点。
    pub caret: &'static str,
    /// 中间省掉的一段。
    pub gap: &'static str,
    /// 一行里几样东西之间。
    pub sep: &'static str,
    /// 横线。
    pub rule: &'static str,
    /// 引用的竖线。
    pub quote: &'static str,
    /// 细竖线。
    pub side: &'static str,
    /// 回车。
    pub enter: &'static str,
    /// 待办：没做的、做完的。
    pub task: Task,
    /// 实心的一格。
    pub block: &'static str,
    /// 八分之一到整格的竖条（滚动条的滑块照它画）。
    pub chart: [&'static str; 8],
}

/// 待办的两种记号。
#[derive(Debug)]
pub struct Task {
    /// 没做的。
    pub todo: &'static str,
    /// 做完的。
    pub done: &'static str,
}

/// 整张表。
#[derive(Debug)]
pub struct Sheet {
    /// 框线。
    pub frame: Frames,
    /// 记号。
    pub marks: Marks,
}

/// 出厂的那一张。
const SHEET: Sheet = Sheet {
    frame: Frames {
        input: Line::Rounded,
        boxes: Line::Rounded,
        toast: Line::Rounded,
        code: Line::Rounded,
        table: Line::Plain,
    },
    marks: Marks {
        pointer: "❯ ",
        ellipsis: "…",
        caret: "▏",
        gap: "⋯",
        sep: " · ",
        rule: "─",
        quote: "│ ",
        side: "│",
        enter: "↵",
        task: Task {
            todo: "☐ ",
            done: "☑ ",
        },
        block: "█",
        chart: ["▁", "▂", "▃", "▄", "▅", "▆", "▇", "█"],
    },
};

/// 读表里的一样东西。
pub fn read<R>(get: impl FnOnce(&'static Sheet) -> R) -> R {
    get(&SHEET)
}

/// 一个记号（`get` 从记号里挑哪一个）。
pub fn mark(get: impl FnOnce(&'static Marks) -> &'static &'static str) -> String {
    (*get(&SHEET.marks)).to_string()
}

/// 一行里几样东西之间（` · `）。
pub fn sep() -> String {
    SHEET.marks.sep.to_string()
}

/// 截掉的地方（`…`）。
pub fn ellipsis() -> String {
    SHEET.marks.ellipsis.to_string()
}
