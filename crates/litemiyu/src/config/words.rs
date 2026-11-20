//! 几块界面的字：换模型和冷却、`!` 跑命令（`text/<语言>.json` 里各一格）。

use crate::config::texts;

texts! {
    /// 撤销说明那一行的字（`ui/undo_rows.rs`，Miyu 的 `undone`、`undo_restore`）。
    #[derive(Debug, Clone)]
    pub struct UndoTexts {
        /// 打头那一句。
        pub undone: String,
        /// 怎么恢复。
        pub restore_hint: String,
        /// 撤掉的几轮里有压缩：头一行下面另起一行（Miyu `undo_compactions`）。
        pub undo_compactions: String,
        /// 撤掉的几轮里有清空（Miyu `undo_clears`）。
        pub undo_clears: String,
        /// 点开以后最后一行，停了几个任务：`{count}`（Miyu `stopped_jobs`）。
        pub stopped_jobs: String,
    }
}

texts! {
    /// 改名的字（`/rename`，Miyu `RenameTexts`）。
    #[derive(Debug, Clone)]
    pub struct RenameTexts {
        /// 改成了，`{title}` 是新标题。
        pub done: String,
        /// 去掉了标题（`/rename -`）。
        pub removed: String,
        /// 太长不发，`{max}` 是上限。
        pub too_long: String,
        /// 只打 `/rename`、重起标题的要求收下了。
        pub retitling: String,
    }
}

texts! {
    /// `/export` 的字（Miyu 终端 `text` 的 `export`，只留存文件那一种）。
    #[derive(Debug, Clone)]
    pub struct ExportTexts {
        /// 写好了：`{path}`、`{size}`。
        pub saved: String,
        /// 没写成：`{reason}` 是原话。
        pub failed: String,
    }
}

texts! {
    /// 换模型的字（蓝图「配置与模型」第 2 条，核心 8-10）。
    #[derive(Debug, Clone)]
    pub struct ModelTexts {
        /// 会话钉着的模型用不了了、内核退回默认的，`{from}` → `{to}`。
        pub replaced: String,
    }
}

texts! {
    /// 人用 `!` 跑命令的字（蓝图「人自己跑命令 `!`」）。
    #[derive(Debug, Clone)]
    pub struct ShellTexts {
        /// 框空着时写的。
        pub placeholder: String,
        /// 带了文件块不发（Miyu `shell.no_attachments`）。
        pub no_attachments: String,
        /// 跑着。
        pub running: String,
        /// 跑完了：`{code}` 退出码、`{elapsed}` 用时。
        pub exited: String,
        /// 被信号杀掉：`{signal}`、`{elapsed}`。
        pub signal: String,
        /// 跑满 120 秒被杀。
        pub timed_out: String,
        /// 你打断的。
        pub interrupted: String,
        /// 程序退出过，没跑完。
        pub aborted: String,
    }
}

texts! {
    /// 「文件修改」的字（Miyu `text` 的 `files`，蓝图 `tui.md`「文件修改」）。
    #[derive(Debug, Clone)]
    pub struct FilesTexts {
        /// 侧边栏那一段的小标题、那一页列表上面的标题。
        pub title: String,
        /// 那一页标题后面：`{n}` 几个文件。
        pub count: String,
        /// 侧边栏放不下的：`{n}` 还有几个。
        pub more: String,
        /// `/files` 时这个会话还没改过文件。
        pub none: String,
        /// 那一页里还在从会话记录读的那一处改动（LiteMiyu 自己的：正文只留几屏，较早的改动点开时才读）。
        pub loading: String,
        /// 那一页里读不出来的那一处改动。
        pub unreadable: String,
        /// 那一页下面的按键提示：一条一个 `[键, 做什么]`。
        pub hints: Vec<[String; 2]>,
    }
}

texts! {
    /// 后台任务的字（Miyu `text` 的 `jobs`，蓝图「后台命令、子代理和侧边栏」）。
    #[derive(Debug, Clone)]
    pub struct JobTexts {
        /// 别的主会话发来的话那一行写的来处（Miyu「别处来的话」第 2 条）：`{id}` 短编号、`{title}` 标题。
        pub from_session: String,
        /// 同上，没有标题的。
        pub from_session_untitled: String,
        /// 来处：这个会话派的子代理，`{job}` 任务编号。
        pub from_agent: String,
        /// 来处：切进子会话看时，主会话发来的交代、留言（Miyu「别处来的话」第 2 条）。
        pub from_main: String,
        /// 别处来的话收着时那一行：`{from}` 来处，`{text}` 话的预览。
        pub received: String,
        /// 「空了告诉我」那一行里的会话：`{id}` 短编号、`{title}` 标题。
        pub peer_who: String,
        /// 同上，没有标题的。
        pub peer_who_untitled: String,
        /// 那个会话干完活空下来了，写成它回复了，`{who}`。
        pub peer_idle: String,
        /// 等了 12 小时作废了，`{who}`。
        pub peer_expired: String,
        /// 那个会话没了，`{who}`。
        pub peer_gone: String,
        /// 那个会话没有窗口开着（LiteMiyu 的 `closed`：订的时候就没开着，或者没等到空下来就关了），`{who}`。
        pub peer_closed: String,
        /// 认不得的原因，`{who}`、`{reason}`。
        pub peer_other: String,
        /// 停的那一条已经结束了（引擎回「停不了」）。
        pub already_ended: String,
        /// 子代理的推送掉了队、补不上（审计 #063）：提示框里的一句。
        pub fell_behind: String,
        /// 底栏的按钮，`{count}` 在跑的后台命令。
        pub button: String,
        /// 后台面板的标题。
        pub title: String,
        /// 后台面板标题后面：`{count}` 在跑的后台命令。
        pub active: String,
        /// 后台面板的按键提示。
        pub hint: String,
        /// 面板里一条的状态：在跑，`{elapsed}`。
        pub running: String,
        /// 完成，`{elapsed}`。
        pub done: String,
        /// 失败，`{code}`。
        pub failed: String,
        /// 被信号杀掉，`{signal}`。
        pub killed: String,
        /// 停了。
        pub stopped: String,
        /// 撤销时停了。
        pub undone: String,
        /// 上次退出时停了（内核的 `restarted`、`aborted`）。
        pub restarted: String,
        /// 跑到最长时间停了。
        pub timed_out: String,
        /// 输出太大停了。
        pub too_large: String,
        /// 输出太频繁停了。
        pub too_chatty: String,
        /// 正文里结束的那一行：完成，`{title}`、`{elapsed}`。
        pub done_note: String,
        /// 失败，`{title}`、`{code}`。
        pub failed_note: String,
        /// 被信号杀掉，`{title}`、`{signal}`。
        pub killed_note: String,
        /// 停了，`{title}`、`{why}`。
        pub stopped_note: String,
        /// 子代理报回来了，`{title}`、`{elapsed}`。
        pub agent_note: String,
        /// 子代理停了，`{title}`、`{why}`。
        pub agent_stopped_note: String,
        /// `{why}` 的几种：撤销时停的。
        pub why_undone: String,
        /// 上次退出时停的。
        pub why_restarted: String,
        /// 超时停的。
        pub why_timed_out: String,
        /// 输出太大停的。
        pub why_too_large: String,
        /// 输出太频繁停的。
        pub why_too_chatty: String,
        /// 结束那一行打头的记号：完成的（绿；失败的红、停了的暗）。
        pub ok_mark: String,
        /// 失败那一行的记号。
        pub fail_mark: String,
        /// 停了那一行的记号。
        pub stop_mark: String,
        /// 监视的命令出了一批行那一行打头的记号（强调色）。
        pub lines_mark: String,
        /// 监视的命令出了一批行：`{title}` 命令，`{lines}` 几行。
        pub lines_note: String,
        /// 展开的后台命令一个字都没有输出。
        pub no_output: String,
        /// 后台面板里结束了的多于一条时收起来的那一行：`{count}` 收了几条。
        pub more_ended: String,
        /// 后台面板里收起来的展开以后最后那一行。
        pub less_ended: String,
        /// 子代理状态行里的主会话。
        pub main: String,
        /// 子代理状态行里报完了的子代理。
        pub finished: String,
        /// 子代理状态行放不下的收成一行，`{count}` 几个（Miyu `jobs.more`）。
        pub more: String,
        /// 子代理状态行「正在做什么」：在想。
        pub doing_thinking: String,
        /// 同上：在说。
        pub doing_replying: String,
        /// 切进子会话时输入框上边框右边写的，`{title}` 子代理的名字（Miyu「切进子会话」第 2 条）。
        pub child_tag: String,
    }
}

texts! {
    /// 被忽略的几样汇成的那一条提示（`app/checkup.rs`，LM-2；LM-3 改成点名；LM-3补挪进 [`super::Front`]、第一句话以后才提示）。
    #[derive(Debug, Clone)]
    pub struct IgnoredTexts {
        /// 整句：`{list}` 是下面几样连起来的。
        pub lead: String,
        /// 配置项：`{names}` 是几个键。
        pub settings: String,
        /// MCP 服务：`{names}`。
        pub mcp: String,
        /// 技能：`{names}`。
        pub skills: String,
        /// 几样之间的连接。
        pub join: String,
        /// 一样里几个名字之间的连接。
        pub names_join: String,
        /// 名字多、只写前几个时：`{names}` 写出的几个，`{count}` 一共几个，`{rest}` 没写出来的几个。
        pub more: String,
    }
}
