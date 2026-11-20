//! 执行工具要的那一份（施工 4-4 上起；从 `tools.rs` 挪出来，施工 O-2 三补）：开会话、载入会话时拼好，交给 [`super::Tools::new`]。

use std::path::PathBuf;
use std::sync::Arc;

use crate::store::blob::Blobs;
use crate::tool::{Catalog, Log, Seen};
use lm_kernel::kernel::time::UtcOffset;

use crate::session::lettering::Lettering;

/// 执行工具要的：工具目录、替工具写的两句、系统的家目录（施工 4-4 上，交给每次调用）。
pub(crate) struct ToolKit {
    /// 会话日志的目录（施工 E-M）：交给命令（`MIYU_SESSION_LOG`）。
    pub(crate) session_log: Option<PathBuf>,
    /// 工具目录：造会话、载入时定，会话中途不换。
    pub(crate) catalog: Arc<Catalog>,
    /// 替工具写的两句，和权限策略同一份（施工 P-1 三补：换快照时跟着换）。
    pub(crate) lettering: Arc<Lettering>,
    /// 系统的家目录。
    pub(crate) home: Option<PathBuf>,
    /// 数据目录：交给工具（施工 4-4 下）。
    pub(crate) data_root: PathBuf,
    /// 窗口之间的门铃放在哪（LM-4）：`send_message` 发给别的窗口时敲（`crate::session::peers`）。
    pub(crate) bell: crate::store::paths::Bell,
    /// 这个会话的 blob：效果里改前改后的内容存进这里（施工 4-6 上）。
    pub(crate) blobs: Blobs,
    /// 她看过的文件：新会话是空的，载入的从日志里重建（施工 4-6 上）。
    pub(crate) seen: Seen,
    /// 这个会话日志的只读入口：交给每次调用，`history` 用（施工 6-4）。
    pub(crate) log: Log,
    /// 会话的时区：开会话时的环境里的（施工 6-4）。
    pub(crate) offset: UtcOffset,
    /// 能不能问人（施工 D-2，[`Agents::asks`]）：能的每次调用给一个提问的端口。
    pub(crate) asks: bool,
    /// 派子代理、父子之间留言要的（施工 7-5、7-7）：会话表交进来了才有。
    pub(crate) agents: Option<Arc<crate::session::agents::Agents>>,
    /// 任务编号（施工 7-5）：和任务表共用一串。
    pub(crate) job_ids: Arc<crate::session::job_ids::JobIds>,
    /// 认她读的是不是技能的 `SKILL.md` 要的（L5 五，`crate::skills::Marks`）：造会话、载入时照这一刻找到的。
    pub(crate) skills: crate::skills::Marks,
}

impl super::Tools {
    /// 系统的家目录：把头报上来的 `~/…` 换成真实的路径用（施工 P-14 的项目指令）。认不出的没有。
    pub(crate) fn home(&self) -> Option<&std::path::Path> {
        self.home.as_deref()
    }

    /// 目录里有读文本文件的那一件（职能 `ReadFile`，同技能列表的门槛）：没有的读不了记忆，不交目录（L6）。
    pub(crate) fn reads_files(&self) -> bool {
        self.catalog
            .specs()
            .any(|spec| spec.role == Some(lm_kernel::kernel::tool::Role::ReadFile))
    }

    /// 数据那一处：文件记忆的文件夹在它下面（L6，`crate::store::paths::Paths::memory`）。
    pub(crate) fn data_root(&self) -> &std::path::Path {
        &self.data_root
    }
}
