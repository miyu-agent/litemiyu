//! 正文里的一条：种类、字、属于哪一轮，和各种条目自己带的东西。

use super::{Chip, Progress, Segment};

/// 正文里一条的种类，决定怎么画。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    /// 你说的话。
    User,
    /// 她的回答。
    Reply,
    /// 时间线的一段：思考、调工具。
    Steps,
    /// 打断了、撤销了这类旁白。
    Note,
    /// 出错了。
    Error,
    /// 一轮做完的收尾行：模型、用时、做完的时刻。
    Done,
    /// 撤销说明那一行（`ui/undo_rows.rs`）：字是撤掉的那一句。
    Undo,
    /// 提问答了：旧版的引用块，行首暗色竖线、整块暗色（蓝图「确认和提问的抽屉」第 6 条）。
    Answered,
}

/// 到点交进来的一句是谁定的时间（LM-41）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Woke {
    /// 循环的一次。
    Loop,
    /// AI 自己定的到点继续。
    Once,
}

/// 正文里的一条。
#[derive(Debug, Clone)]
pub struct Entry {
    /// 生出来时发的编号，这次启动里不重复：排好的行按它记（`ui/row_cache`），不按它在正文里排第几。
    pub id: u64,
    /// 种类。
    pub kind: Kind,
    /// 字；时间线那一条没有字。
    pub text: String,
    /// 时间线那一条的一段；别的是 `None`。
    pub segment: Option<Segment>,
    /// 属于哪一轮；你刚说、还没开轮的，和撤销说明这类不属于哪一轮的，是 `None`。
    pub turn: Option<u64>,
    /// 这一轮被撤掉了：不画，恢复时再画。
    pub hidden: bool,
    /// 你说的话发出去时正在回答：先排着，列在运行状态行下面；开了它那一轮才进正文（`tui.md`「运行状态行和排队的消息」第 5 条）。
    pub queued: bool,
    /// 你说的话落盘以后的序号（`message.user`）：认它开了哪一轮、被退回的是哪一句。别的条是 `None`。
    pub seq: Option<u64>,
    /// 人点开了：撤销那一行下面铺底色写全文。
    pub open: bool,
    /// 你说的话里的块：粘贴块点开看全文，附件不展开（蓝图「正文」第 2 条、「输入框」第 12 条）。别的条是空的。
    pub pasted: Vec<Chip>,
    /// 她的回答里点过的 `<details>`（第几个，从 0 数）：和它写的 `open` 反过来（蓝图「她的回答：Markdown」第 15 条）。
    pub details: Vec<usize>,
    /// 正在压缩的那一行的进度（蓝图「正文」第 9 条）。压好了、失败了是 `None`。
    pub progress: Option<Progress>,
    /// 旁白前面绿色的记号（压好了的 `● `，蓝图「正文」第 9 条）；别的是 `None`。
    pub mark: Option<String>,
    /// 那个记号的颜色：压好了、后台任务完成的绿，失败的红，停了的暗（Miyu 蓝图「后台命令、子代理和侧边栏」第 5 条）。
    pub mark_tone: MarkTone,
    /// 你说的话什么时候说的：看着发的是发的那一刻，补发来的是事件的时刻（`/jump` 的列表右边写它）。别的条是 `None`。
    pub at: Option<jiff::Timestamp>,
    /// 你用 `!` 打的命令（蓝图「人自己跑命令 `!`」第 4 条）：`text` 是命令行，跑完了带结果。别的条是 `None`。
    pub shell: Option<Shell>,
    /// 别处来的话（别的会话发来的）的来处那一句（Miyu「别处来的话」第 2 条）：照它画成别处来的样子。你说的、别的条是 `None`。
    pub from: Option<String>,
    /// 循环、定时触发到点交进来的一句（LM-41）：画成一行暗色的旁白。和别处来的话不同，撤回照样带走它。别的条是 `None`。
    pub woke: Option<Woke>,
    /// 点开看的全文（Miyu「后台命令、子代理和侧边栏」第 5 条）：子代理结束那一行带它交回来的报告。别的条是空的。
    pub detail: String,
    /// 后台任务、别的会话那几行（[`super::Transcript::marked_with`] 写的，Miyu `Kind::Job`）：一行，放不下截掉；有全文的能点开
    /// （`ui/report_rows.rs`）。
    pub report: bool,
    /// 属于第几段（`pieces.rs`）：记进正文时定。
    pub piece: u32,
    /// 撤销说明那一行：撤销的回应说撤掉了几次压缩、清空，停了几个任务（Miyu `Entry.undo` 的 `Report` 里那三个数）。别的条、
    /// 一样都没有的、补发来的（日志里没有）是 `None`。
    pub undo: Option<Box<lm_kernel::kernel::session::Undid>>,
}

/// 旁白前面那个记号的颜色。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum MarkTone {
    /// 绿：好了。
    #[default]
    Good,
    /// 红：失败了。
    Bad,
    /// 暗：停了。
    Quiet,
    /// 强调色：监视的命令出了一批行（Miyu `JobMark::Lines`）。
    Accent,
}

/// 你用 `!` 打的一条命令。
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct Shell {
    /// 跑完了的结果；还在跑、还排着的是 `None`。
    pub ran: Option<crate::core::ShellRan>,
}

impl Entry {
    /// 只有种类和字的一条，别的都空着。
    pub fn new(id: u64, kind: Kind, text: String) -> Entry {
        Entry {
            id,
            kind,
            text,
            segment: None,
            turn: None,
            hidden: false,
            queued: false,
            seq: None,
            open: false,
            pasted: Vec::new(),
            details: Vec::new(),
            progress: None,
            mark: None,
            mark_tone: MarkTone::Good,
            at: None,
            shell: None,
            from: None,
            woke: None,
            detail: String::new(),
            report: false,
            piece: 0,
            undo: None,
        }
    }

    /// 放回输入框、记进输入历史时的样子：跑命令的前面加 `!`（「人自己跑命令 `!`」第 5 条）。
    pub fn typed(&self) -> String {
        match self.shell {
            Some(_) => format!("!{}", self.text),
            None => self.text.clone(),
        }
    }

    /// `/jump` 列的样子：跑命令的写成「! 命令行」（「人自己跑命令 `!`」第 5 条）。
    pub fn listed(&self) -> String {
        match self.shell {
            Some(_) => format!("! {}", self.text),
            None => self.text.clone(),
        }
    }
}
