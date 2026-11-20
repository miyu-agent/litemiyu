//! 人用 `!` 打的命令（蓝图 `tui.md`「人自己跑命令 `!`」第 4 条）：`message.user` 写了 `shell`，跑完一条 `shell.ran`
//! （`message` 是那句话的序号）。

use lm_kernel::kernel::event::ShellRan as Ran;

/// 跑完了（`shell.ran`）。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ShellRan {
    /// 那句话的序号。
    pub message: u64,
    /// 退出码；被信号杀掉的、没跑完的是 `None`。
    pub exit: Option<i64>,
    /// 杀掉它的信号。
    pub signal: Option<i64>,
    /// 跑了多久（毫秒）。
    pub duration_ms: Option<u64>,
    /// 输出（标准输出和标准错误合在一起，引擎截过的中间有一行写省了多少）。
    pub text: String,
    /// 异常结束的：`timed_out`、`interrupted`、`aborted`。
    pub reason: Option<String>,
}

impl ShellRan {
    /// 照内核的事件读。
    pub(super) fn of(ran: &Ran) -> Self {
        Self {
            message: ran.message.get(),
            exit: ran.exit.map(i64::from),
            signal: ran.signal.map(i64::from),
            duration_ms: Some(ran.duration_ms),
            text: ran.text.clone(),
            reason: ran.reason.as_ref().map(|r| r.as_str().to_string()),
        }
    }
}
