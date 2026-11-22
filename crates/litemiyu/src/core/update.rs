//! 引擎那边交给界面的消息。

use super::Push;

/// 引擎那边的消息，交给界面。
#[derive(Debug)]
pub enum Update {
    /// 会话开好了，带着会话编号。
    Ready(String),
    /// 引擎起不来、会话开不成（配置读不懂、没配供应商、数据目录写不进）：原因。
    Failed(String),
    /// 一条命令被拒绝：原因码（没有的是 `None`）和原话。
    Refused {
        /// 原因码，例如 `nothing_to_compact`。
        reason: Option<String>,
        /// 原话。
        message: String,
    },
    /// 说的话没发出去（会话拒了）：和 [`Update::Refused`] 一样的两样；先画进正文的那句要撤掉，字放回输入框
    /// （Miyu 蓝图「输入框」第 12、13 条）。
    Unsent {
        /// 原因码，例如 `busy`。
        reason: Option<String>,
        /// 原话。
        message: String,
    },
    /// 会话里的事。
    Push(Push),
    /// 回答确认、提问被拒（[`super::Command::Answer`]）：调用编号、原因码、原话。`not_asking` 是已经答过、了结了。
    AnswerRefused {
        /// 调用编号。
        call: String,
        /// 原因码。
        reason: Option<String>,
        /// 原话。
        message: String,
    },
    /// 撤销成了（[`super::Command::Revert`]；`restore` 是恢复）：撤掉、改回的样子照推送画，这里说成了，撤销另带撤掉了几次
    /// 压缩、几次清空、停了几个任务（Miyu 收回应的 `Undone`、回应的 `Report` 里的 `compactions`、`clears`、`jobs`）。
    Undone {
        /// 是恢复。
        restore: bool,
        /// 撤掉了什么（内核撤销的回应带的，恢复、一样都没有的是空的）。
        undid: lm_kernel::kernel::session::Undid,
    },
    /// 改好了名字（[`super::Command::Title`]）：新标题；去掉了标题的是 `None`。
    Renamed(Option<String>),
    /// 导出的结果（[`super::Command::Export`]）：写好的文件和它多大（字节），写不成的是原话。
    Exported(Result<(std::path::PathBuf, u64), String>),
    /// 删会话的结果（[`super::Command::Delete`]）：删不掉的是原话。
    Deleted(Result<(), String>),
    /// 重起标题的要求收下了（[`super::Command::Retitle`]），在等模型。
    Retitling,
    /// 重起标题没起成：给人看的那一句（`core/sessions/retitle-failed`）、那一次请求的错（原话，接在后面，同 miyu-tui
    /// `355ba92c`）。
    RetitleFailed {
        /// 给人看的那一句。
        said: lm_kernel::kernel::event::Said,
        /// 那一次请求的错的原话；空的不接。
        error: String,
    },
    /// 一条斜杠命令的回执（`/loop`）：照界面语言换成字弹提示。
    Said(lm_kernel::kernel::event::Said),
    /// 这个会话认不认 `/loop`（`Handle::loops`）：会话开好时交一次，命令列表照它列不列。
    Loops(bool),
    /// 循环这一刻的样子（`Handle::looping`）：会话推来一批以后变了才交；没开着的是 `None`。
    LoopNow(Option<lm_engine::session::Looping>),
    /// 主会话的账（`Handle::meter`，`engine/forward.rs`）：开会话、每次请求说完、一轮结束时交一次。
    Meter {
        /// 哪个会话的：换了会话以后到的旧账不要。
        session: String,
        /// 账。
        meter: Box<lm_engine::session::Meter>,
    },
    /// 开主会话以前它派过的子代理一共用掉的（`Handle::earlier_agents`，`engine/forward.rs`）：重开的会话开好时交一次，
    /// 没记着的不交。
    EarlierAgents {
        /// 哪个会话的：换了会话以后到的不要。
        session: String,
        /// 账。
        meter: Box<lm_engine::session::Meter>,
    },
    /// 主会话派的一个子会话推来的（`engine/children.rs`）：它的编号、推的什么。
    Child {
        /// 子会话的整个编号。
        session: String,
        /// 推的什么。
        push: Push,
    },
    /// 一个子会话的推送掉了队、照落了盘的也补不上（`engine/children.rs`、`engine/view.rs`，审计 #063）：界面弹一句
    /// 「子代理的消息可能缺了一段」。
    ChildBehind,
    /// 切进了子会话（[`super::Command::View`]）：它日志里落了盘的，照补发读成的推送一块块交（边读边交，同 [`Update::Replayed`]），
    /// 最后一块的最后一条是钟回到现在。头一条是 `first`、推送是空的：界面照它开始攒子会话的正文，钟回到现在那一块办完才换上屏幕。
    Viewed {
        /// 子会话的整个编号。
        session: String,
        /// 照先后的推送。
        pushes: Vec<Push>,
        /// 头一条。
        first: bool,
        /// 办完了回一声。
        done: std::sync::mpsc::Sender<()>,
    },
    /// 正在看的子会话之后推来的（在跑的接着出字）。
    ViewPush {
        /// 子会话的整个编号。
        session: String,
        /// 推的什么。
        push: Push,
    },
    /// 读到的后台命令的输出（[`super::Command::JobOutput`]）：读不到的是 `None`。
    JobOutput {
        /// 任务编号。
        job: String,
        /// 输出的末尾。
        text: Option<String>,
    },
    /// 列出来的会话（[`super::Command::Sessions`]），最近有动静的在前；放会话的目录读不了的是原因。
    Sessions {
        /// 列出来的。
        listed: Result<Vec<lm_engine::session::Listed>, String>,
        /// 这个窗口里留在后台接着跑的会话的编号（LM-45）：列表里写「在后台运行中」，不写成别的窗口开着的。
        parked: Vec<String>,
    },
    /// 正在说话的会话钉着哪个预设（LM-29，`engine/presets.rs`）：开了新会话、打开了旧会话时交一次；配置换了、预设改了删了
    /// 的再交一次（名字后面的「已修改」「已删除」照它）。
    Preset(super::Pinned),
    /// 列出来的预设（[`super::Command::Presets`]，写了默认预设、删了一个以后也交）。
    Presets(lm_engine::presets::Listed),
    /// 旧会话打开了（[`super::Command::Open`]）：会话编号。界面换一份空的正文，接着来的推送是它从头补发的。
    Opened(String),
    /// 旧会话打不开（[`super::Command::Open`]）：正在说话的会话照旧。
    OpenFailed {
        /// 别的窗口开着它（`LoadError::Taken`）。
        taken: bool,
        /// 这一刻读不了（`LoadError::passing`），过一会儿再试也许就好了：界面照 Miyu 说「会话暂时无法读取，请稍后重试」。
        unavailable: bool,
        /// 原话。
        message: String,
    },
    /// 打开的旧会话日志中间坏了一行（[`super::Command::Open`] 的 `view_broken`，同 Miyu 核心 YC-6 的 `session_read_only`）：
    /// 坏行以前的补发完了，只能看、不能接着聊。坏在这一段的第几行（从 1 数）。
    ReadOnly {
        /// 第几行。
        line: usize,
    },
    /// 被忽略的几样（`app/checkup.rs`）：第一句话发出去以后，读配置、找技能、读 MCP 服务时记下的，只有查出了才交，一次运行
    /// 只交一次（LM-3补）。
    Ignored(lm_engine::checkup::Ignored),
    /// 刚发出去的那句话附了图片，而这个会话这时的模型看不了图（LM-28，`lm_engine::session::Attached` 的 `blind`）：界面提示一句。
    ImagesUnseen,
    /// 停不了这个任务（[`super::Command::StopJob`]）：原因码（`job_ended` 已经结束了，`unknown_job`）。
    StopRefused {
        /// 原因码。
        reason: String,
    },
    /// 读回来的一串段（[`super::Command::ReadBack`]）：哪个会话的、从第几段起、几段、照补发读成的推送；读不回来的是原因。
    ReadBack {
        /// 会话编号：换了会话以后才到的不要。
        session: String,
        /// 从第几段起。
        piece: u32,
        /// 几段。
        count: u32,
        /// 推送，或者读不回来的原因。
        pushes: Result<Vec<super::Push>, String>,
    },
    /// 打开旧会话时从日志补发的一块推送（边读边交，`core/engine/sessions.rs`）：界面照先后一条条办，办完了往 `done` 回一声，
    /// 读日志的那一头才接着交下一块（最多两块在路上：不攒整份推送）。
    Replayed {
        /// 推送，照先后。
        pushes: Vec<super::Push>,
        /// 办完了回一声。
        done: std::sync::mpsc::Sender<()>,
    },
    /// 日志里你说的话（[`super::Command::Spoken`]）：哪个会话的；读不了的是原因。
    Spoken {
        /// 会话编号：换了会话以后才到的不要。
        session: String,
        /// 照先后。
        said: Result<Vec<crate::transcript::Said>, String>,
    },
    /// 日志里改过一个文件的那几步（[`super::Command::Changes`]）：哪个会话、哪个文件；读不了的是原因。
    Changes {
        /// 会话编号：换了会话以后才到的不要。
        session: String,
        /// 哪个文件。
        path: String,
        /// 照先后。
        found: Result<Vec<crate::transcript::Found>, String>,
    },
    /// `/model` 的一行行（[`super::Command::Choices`]）：池在前，再是每一家的模型；读不了配置的是空的。
    Choices(Vec<super::Choice>),
    /// 一个模型有哪几档思考程度（[`super::Command::Efforts`]）：问的是默认的那个时 `reference` 是查到的引用。
    Efforts {
        /// 引用。
        reference: String,
        /// 几档，照模型目录的先后；池、不知道的是空的。
        levels: Vec<String>,
    },
    /// 这个会话的思考程度换成了（[`super::Command::Effort`]）：`None` 是回到跟着配置。
    EffortSet(Option<String>),
}
