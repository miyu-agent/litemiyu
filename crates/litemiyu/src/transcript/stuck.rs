//! 会话卡住了没有（蓝图 `tui.md`「框下面那一行」用量那一格，反馈 1102 第 22 条）：框下面那一格照它变红、给「压缩」。用了多少、
//! 线和窗口写在上下文那一段里（2026-11-14 改回 Miyu 换成环以前的写法），这里只分是哪一种。

use super::Transcript;

/// 卡住的两种。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stuck {
    /// 最近一次压缩失败了、或者自动压缩暂停了，后来没压好、没清空过，用量到了压缩线。
    Failed,
    /// 用量大于窗口（换到了窗口小的模型这类）。
    Over,
}

impl Transcript {
    /// 卡住了是哪一种；两样都是的照压缩失败。没卡住是 `None`。
    pub fn stuck(&self) -> Option<Stuck> {
        let used = self.context;
        let failed = self
            .limits
            .compaction_line
            .is_some_and(|line| self.compaction_trouble && used >= line);
        let over = self.limits.window.is_some_and(|window| used > window);
        if failed {
            Some(Stuck::Failed)
        } else if over {
            Some(Stuck::Over)
        } else {
            None
        }
    }
}
