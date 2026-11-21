//! 贴在输入框上面的几块面板上的字（`text/zh.json`；蓝图 `tui.md`「输入历史列表」）。

use crate::config::texts;

texts! {
    /// 输入历史列表上的字。
    #[derive(Debug, Clone)]
    pub struct HistoryTexts {
        /// 标题。
        pub title: String,
        /// 标题后面的条数，`{count}`。
        pub count: String,
        /// 框里最下面搜索那一行没打字时暗色写的（「输入历史列表」第 1 条）。
        pub placeholder: String,
        /// 一条都对不上时那一行。
        pub empty: String,
        /// 好几行的，第一行后面跟着的，`{count}` 还有几行。
        pub lines: String,
        /// 预览太长时最后一行，`{count}` 还有几行。
        pub more: String,
        /// 最后一行按键提示。
        pub hints: String,
        /// 按键模式是 Vim、没在打搜索的字时的按键提示（Miyu 发版前 check 第 31 条）。
        pub hints_vim: String,
        /// 按键模式是 Vim、按了 `/` 在打搜索的字时的。
        pub searching: String,
        /// 一分钟以内发的。
        pub now: String,
        /// `{n}` 分钟前发的。
        pub minutes: String,
        /// `{n}` 小时前发的。
        pub hours: String,
        /// `{n}` 天前发的（重启以后还在，「输入历史列表」第 8 条）。
        pub days: String,
        /// 还没发过话时按 Ctrl+R 的提示。
        pub none: String,
    }
}

texts! {
    /// 斜杠命令列表上的字（蓝图「斜杠命令列表」第 3 条）。
    #[derive(Debug, Clone)]
    pub struct MenuTexts {
        /// 标题。
        pub title: String,
        /// 标题后面的条数，`{count}`。
        pub count: String,
    }
}

texts! {
    /// 框下面那一行用量那一格的字（蓝图「框下面那一行」用量那一格；2026-11-14 项目主人定照 Miyu 换成环以前的写法）。
    #[derive(Debug, Clone)]
    pub struct UsageCellTexts {
        /// 当前装了多少 / 窗口（占比）：`{used}`、`{window}`、`{percent}`（Miyu `3c94e1fc^` 的 `text.context`）。
        pub context: String,
        /// 会话累计（缓存命中率）：`{tokens}`、`{percent}`（Miyu `3c94e1fc^` 的 `text.total`）。
        pub total: String,
        /// 卡住：最近一次压缩失败、自动压缩暂停了，用量到了压缩线。
        pub failed: String,
        /// 卡住：用量大于窗口。
        pub over: String,
        /// 卡住时跟在后面那个能点的。
        pub compact: String,
        /// 正在压缩。
        pub compacting: String,
    }
}

texts! {
    /// 会话列表（`/sessions`）和侧边栏「最近会话」的字（Miyu 终端 `text/<语言>.json` 的 `sessions`，去掉 LiteMiyu 没有的
    /// 置顶、删除、勾选、终端集成、存档、总用量；加上会话开在别的窗口、没开着的几种）。
    #[derive(Debug, Clone)]
    pub struct SessionTexts {
        /// 上边框写的。
        pub title: String,
        /// 几个，`{count}`。
        pub count: String,
        /// 打了字时接在后面的。
        pub query: String,
        /// 下边框的按键提示。
        pub hint: String,
        /// 按键模式是 Vim、没在打搜索的字时的按键提示。
        pub hint_vim: String,
        /// 按键模式是 Vim、按了 `/` 在打搜索的字时的。
        pub searching: String,
        /// 列表还没交回来。
        pub loading: String,
        /// 一个都对不上。
        pub empty: String,
        /// 一分钟以内有过动静。
        pub now: String,
        /// `{n}` 分钟前。
        pub minutes: String,
        /// `{n}` 小时前。
        pub hours: String,
        /// `{n}` 天前。
        pub days: String,
        /// 侧边栏那一段的小标题。
        pub side_recent: String,
        /// 还没起标题的会话。
        pub untitled: String,
        /// 别的窗口开着它、闲着。
        pub open_elsewhere: String,
        /// 别的窗口开着它、有一轮在跑。
        pub busy_elsewhere: String,
        /// 没有窗口开着它（引擎的 `closed`）。
        pub closed: String,
        /// 要切过去的会话别的窗口开着：不硬开。
        pub taken: String,
        /// 打不开，`{reason}` 是原话。
        pub open_failed: String,
        /// 打开的会话日志中间坏了一行、只能看（Miyu `unread.read_only`），正文末尾写：`{line}` 是第几行。
        pub read_only: String,
        /// 同上，输入框空着时写的那一行（Miyu `unread.read_only_placeholder`）。
        pub read_only_placeholder: String,
        /// 一时读不了、过一会儿自动再试（Miyu `unread.unavailable_retry`）：`{wait}` 是几秒。
        pub unavailable_retry: String,
        /// 试满了不再试（Miyu `unread.unavailable_gave_up`）。
        pub unavailable_gave_up: String,
        /// 列不出来，`{reason}` 是原话。
        pub list_failed: String,
        /// `/delete`、会话列表里删好了，`{title}`（Miyu）。
        pub deleted: String,
        /// 会话列表上边框：勾了几个，`{count}`（Miyu「会话列表」第 2 条）。
        pub ticked: String,
        /// 会话列表里按了一次 `Ctrl+D`，`{title}`。
        pub delete_again: String,
        /// 会话列表里按了一次 `Ctrl+D`、要删好几个，`{count}`。
        pub delete_many: String,
        /// 会话列表里删了好几个，`{count}`。
        pub deleted_many: String,
        /// 会话列表的段头（Miyu「会话列表」第 10 条）：最近、近 7 天、近 30 天、30 天前（Miyu 还有「存档」，LiteMiyu 没有存档）。
        pub groups: [String; 4],
        /// `/delete` 时回答进行中（Miyu）。
        pub delete_running: String,
        /// `/delete` 删不掉，`{reason}` 是原话。
        pub delete_failed: String,
    }
}
