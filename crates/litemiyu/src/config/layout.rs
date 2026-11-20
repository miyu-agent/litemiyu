//! 布局的数值（`resources/layout.json`）：宽度、留白、列表几行、提示停多久、压缩进度条怎么动这些（蓝图 `tui.md`
//! 「资源文件」）。

use crate::config::texts;

use super::CompactionMotion;

texts! {
    /// 布局的数值。
    #[derive(Debug, Clone)]
    pub struct Layout {
        /// 制表符展开成几格（代码块、命令的输出）。
        pub tab: usize,
        /// 输入框和正文占窗口宽度的百分之几，跟着窗口一起变宽。
        pub width_percent: u16,
        /// 终端窄到这个宽度以下，输入框不留边、占满整行。
        pub narrow_below: u16,
        /// 空会话的首页上，输入框（连边框）最宽几列（`tui.md`「空会话的首页」第 2 条）。
        pub home_max_width: u16,
        /// 窗口至少这么宽才有侧边栏（Miyu 蓝图「后台命令、子代理和侧边栏」第 7 条）。
        pub sidebar_from: u16,
        /// 侧边栏多宽（不算和主列之间那根竖线）。
        pub sidebar_width: u16,
        /// 侧边栏「跳转消息」列几条，0 是不画。
        pub sidebar_jumps: usize,
        /// 侧边栏「最近会话」列几个（算上当前这个），0 是不画（Miyu `tui.sidebar_sessions`）。
        pub sidebar_sessions: usize,
        /// 侧边栏「最近会话」里当前这个会话的记号（Miyu 出厂主题的 `side_here_mark`）。
        pub side_here_mark: String,
        /// 会话列表最多露几行（Miyu `session_rows`）。
        pub session_rows: usize,
        /// 会话列表右边那一格的工作目录长过几列只写最后两层（Miyu `session_cwd_width`，「会话列表」第 1 条）。
        pub session_cwd_width: usize,
        /// 会话列表标题一列最宽几列（Miyu `session_title_width`）。
        pub session_title_width: usize,
        /// 会话列表里勾上的记号，连同它后面的空格（Miyu 出厂主题的 `tick_mark`，「会话列表」第 2 条）。
        pub tick_mark: String,
        /// `/model`、`/effort` 框里现在用的那一行行首的记号（Miyu `current_mark`）。
        pub current_mark: String,
        /// 会话标题最多几个字（`/rename`，Miyu `title_max`）。
        pub title_max: usize,
        /// 撤销说明那一行打头的记号（Miyu 出厂主题的 `undo_icon`）。
        pub undo_icon: String,
        /// 侧边栏上下文那一段的进度条：占了的、没占的。压缩那一行的进度条也照它。
        pub bar: Bar,
        /// 压缩那一行的进度条怎么动（蓝图「正文」第 9 条）。
        pub compaction: CompactionMotion,
        /// 按键模式是 Vim（蓝图「按键」表「按键模式」那一行）：会话列表、`/model` 里 `j`、`k` 挪，`/` 才搜；`false` 是方向键。
        pub vim: bool,
        /// 金额前面写的符号，照币种代码（Miyu `layout.json` 的 `currencies`）；表里没有的币种写在数后面（`1.30 HKD`）。
        pub currencies: std::collections::HashMap<String, String>,
        /// 输入框和屏幕左右边之间至少留几列。
        pub side_gap: u16,
        /// 正文上面空几行，不贴着屏幕顶。
        pub top_gap: u16,
        /// 文字和框的左边之间留几列。
        pub pad_left: u16,
        /// 文字和框的右边之间留几列。至少要一列，给行尾的光标。
        pub pad_right: u16,
        /// 窗口少于这么多列换紧凑版面：框贴着两边，框里只留提示符和光标的地方（蓝图「输入框」第 10 条）。
        pub compact_below: u16,
        /// 紧凑版面框里左边几列（提示符的宽度）。
        pub compact_pad_left: u16,
        /// 紧凑版面框里右边几列（光标的一列）。
        pub compact_pad_right: u16,
        /// 紧凑版面窗口两边各留几列：框、正文、框外面那几行都在里面（2026-10-27 项目主人：「左右至少还是要给一列的边距」）。
        pub compact_side_gap: u16,
        /// 一次粘贴超过这么多行，输入框里收成一块（蓝图「输入框」第 11 条）。
        pub paste_fold_lines: usize,
        /// 一次粘贴超过这么多字，收成一块。
        pub paste_fold_chars: usize,
        /// 输入框最多长到几行，再多就在框里滚。
        pub max_rows: u16,
        /// 两次点击隔多久以内算双击，毫秒。
        pub double_click_ms: u64,
        /// 按了第一下 `Esc`，多久以内再按一下才打断（抽屉开着时是取消），毫秒。
        pub esc_window_ms: u64,
        /// 子代理状态行最多列几个子代理，多的收成「还有 N 个」一行（Miyu `layout.agent_rows`，`ui/agents.rs`）。
        pub agent_rows: usize,
        /// 抽屉带文字画时可以高过半屏，但屏幕顶上至少留这么多行（蓝图「确认和提问的抽屉」第 2 条）。
        pub drawer_keep_rows: u16,
        /// 提问抽屉里选项预览的面板怎么摆（蓝图「确认和提问的抽屉」第 3 条）。
        pub drawer_preview: crate::ui::PreviewLook,
        /// 输入框左上方的提示停多久，毫秒。
        pub notice_ms: u64,
        /// 最多每多少毫秒画一帧：这中间来的推送攒着，到点一起画（蓝图「每一帧」）。
        pub frame_ms: u64,
        /// 按了键、粘贴的不等 `frame_ms`，只再等这么多毫秒把一串一起到的收齐（输入法上屏一次好几个字）就画。
        pub key_burst_ms: u64,
        /// 她的回答排好的 Markdown 行留最近用过的几篇（蓝图「正文」第 8 条）。
        pub markdown_cache: usize,
        /// 整份重排时一帧最多花多少毫秒，没排完的下几帧接着排（蓝图「正文」第 8 条）。
        pub relayout_budget_ms: u64,
        /// 运行状态行词后面的三个点：一直在，和词一起被流光扫（`tui.md`「运行状态行和排队的消息」第 2 条）。
        pub dots: Dots,
        /// 运行状态行、压缩那一行的流光（`tui.md`「运行状态行和排队的消息」第 1 条）。
        pub shimmer: Shimmer,
        /// 用哪套图标（`resources/icons/` 里的名字，蓝图「图标」）；没有这一套的用出厂的第一套。
        pub icons: String,
        /// 输入框第一行文字前面的提示符，连同它后面的空格；颜色跟着模式。它住在 `pad_left` 那几列里。
        pub prompt: String,
        /// 排队的消息前面的记号，连同它后面的空格。
        pub queued_mark: String,
        /// 斜杠命令列表最多露出几行。取单数，选中的那一行才停得在正中间。
        pub menu_rows: usize,
        /// 输入历史列表那一段几条，0 是跟窗口高（Miyu `tui.history_rows`，`tui.md`「输入历史列表」第 1 条「高度」，发版前 check
        /// 第 52 条）。
        pub history_rows: usize,
        /// `/jump` 跳到的那一条竖线亮多久，毫秒（`tui.md`「跳到说过的话」第 3 条）。
        pub jump_flash_ms: u64,
        /// 输入历史列表上下排时预览固定几行（Miyu `tui.history_preview_rows`，「输入历史列表」第 1 条「高度」）。
        pub history_preview_rows: usize,
        /// 正文里用户说的话前面那根竖线，连同它后面的空格。
        pub user_bar: String,
        /// 一轮做完的收尾行前面的符号，连同它后面的空格。
        pub done_icon: String,
        /// 收尾行里图标后面多空的（Miyu 照权限级别写，LiteMiyu 只有它「开放权限」那一种）。
        pub done_gap: String,
    }
}

texts! {
    /// 运行状态行词后面的点（`layout.json` 的 `dots`）。
    #[derive(Debug, Clone)]
    pub struct Dots {
        /// 点是哪个字。
        pub mark: String,
        /// 几个点。
        pub count: usize,
    }
}

texts! {
    /// 流光（`layout.json` 的 `shimmer`）：一道亮光从左往右扫过，没扫到的压暗。真彩色才照字节算，别的加粗、变暗。
    #[derive(Debug, Clone)]
    pub struct Shimmer {
        /// 亮光从头扫到尾要几秒。点的个数也照它轮一圈。
        pub sweep_seconds: f64,
        /// 亮光宽几个字。
        pub band: f64,
        /// 没扫到的字亮度乘几。
        pub dim: f64,
        /// 扫到正中的字往白里偏多少，0 到 1。
        pub lift: f64,
    }
}

texts! {
    /// 进度条的两种格（`layout.json` 的 `bar`）。
    #[derive(Debug, Clone)]
    pub struct Bar {
        /// 几格。
        pub width: usize,
        /// 占了的。
        pub full: String,
        /// 没占的。
        pub empty: String,
    }
}
