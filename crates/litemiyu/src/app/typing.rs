//! 输入（蓝图 `app.md` 的 `typing` 块）：输入框和它上面的列表（斜杠命令、输入历史），焦点、两下 Esc 的时限、空着时的
//! 提示、框下面那一行的临时提示。

use std::time::Instant;

use super::Notice;
use crate::history::History;
use crate::input::InputBox;
use crate::menu::Menu;
use crate::tips::Tips;

/// 输入。
pub struct Typing {
    /// 输入框。
    pub input: InputBox,
    /// 斜杠命令列表。
    pub menu: Menu,
    /// 输入历史列表（`Ctrl+R`）。
    pub history: History,
    /// 在回答时第一下 `Esc` 的时刻：时限里再按一下才打断。
    pub esc_at: Option<Instant>,
    /// 输入框空着时写哪条提示。
    pub tips: Tips,
    /// 框下面那一行的临时提示。
    pub notice: Option<Notice>,
    /// 焦点在哪：输入框、框下面那一行的按钮、子代理状态行的第几行（0 是主会话，`agents.rs`）。
    pub focus: crate::focus::Focus,
    /// 切会话时输入框里没发的那句，照会话编号记着（Miyu `636b2b8d`，发版前 check 第 17 条，2026-11-09 项目主人：从侧边栏
    /// 「最近会话」切到别的会话，框里的字跟着带过去了，不该带）：切走时记在原来那个名下，切回来放回去（`sessions.rs`）。
    pub drafts: std::collections::HashMap<String, crate::input::Draft>,
    /// 剪贴板里贴进来的图片暂存在哪（LM-28，`clipboard/staging.rs`）：第一次贴图时才造，没贴过图的是 `None`。
    pub staging: Option<crate::clipboard::Staging>,
}
