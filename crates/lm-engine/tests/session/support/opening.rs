//! 造会话时可以换的几样（Miyu 从 `mod.rs` 挪出来的 `Opening`、`Lines` 并成一份）：有没有人能确认、工作目录（施工 4-3 下），
//! 编号、一次性、模型、无功能。权限、沙盒、场所、父会话、记忆、预设、群会话那些线删了（L1、L3 二）。

use lm_kernel::kernel::id::{CommandId, SessionId};

use super::environment;

/// 造会话时可以换的几样。
pub struct Opening {
    /// 有没有人能确认。
    pub attended: bool,
    /// 工作目录。
    pub cwd: String,
    /// 加进来的目录（施工 5-10 上）。
    pub dirs: Vec<String>,
    /// 会话编号：默认照这一刻新编一个。
    pub id: Option<SessionId>,
    /// 造会话的命令编号：默认 `cmd-0`。
    pub command: Option<CommandId>,
    /// 一次性的（施工 7-9）：默认不是。
    pub oneshot: bool,
    /// 用哪个模型（施工 8-8）：默认没有，照这时的 `models.chat`。
    pub model: Option<String>,
    /// 无功能的一档（设计第五节第 14 条）：默认不是。
    pub no_tools: bool,
    /// 自定义的预设（LM-29）：默认没有。
    pub preset: Option<lm_kernel::policy::PresetPin>,
}

impl Default for Opening {
    /// 有人能确认，工作目录照 [`environment`]，平常的一档。
    fn default() -> Opening {
        Opening {
            attended: true,
            cwd: environment().cwd,
            dirs: Vec::new(),
            id: None,
            command: None,
            oneshot: false,
            model: None,
            no_tools: false,
            preset: None,
        }
    }
}
