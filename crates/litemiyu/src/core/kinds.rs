//! 事件里几样有固定取值的字：工具结果的状态、一轮结束的原因。界面只认它画得出的几种，内核新加的照「别的」画。

/// 工具结果的状态（`tool.result` 的 `status`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolStatus {
    /// 成了。
    Ok,
    /// 出错。
    Error,
    /// 没做：被拒了。
    Denied,
    /// 打断了。
    Cancelled,
    /// 跳过了。
    Skipped,
    /// 新版本才有的取值：照「成了」画。
    Other,
}

impl ToolStatus {
    /// 照内核的那一种读。
    pub fn of(status: &lm_kernel::kernel::event::ToolStatus) -> ToolStatus {
        Self::parse(status.as_str())
    }

    /// 照事件里的写法读。
    fn parse(text: &str) -> ToolStatus {
        match text {
            "ok" => ToolStatus::Ok,
            "error" => ToolStatus::Error,
            "denied" => ToolStatus::Denied,
            "cancelled" => ToolStatus::Cancelled,
            "skipped" => ToolStatus::Skipped,
            _ => ToolStatus::Other,
        }
    }
}

/// 一轮结束的原因（`turn.ended` 的 `reason`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EndReason {
    /// 照常结束。
    Completed,
    /// 打断了。
    Interrupted,
    /// 出错了。
    Error,
    /// 别的原因，照原样写出来。
    Other(String),
}

impl EndReason {
    /// 照内核的那一种读。
    pub fn of(reason: &lm_kernel::kernel::event::EndReason) -> EndReason {
        Self::parse(reason.as_str())
    }

    /// 照事件里的写法读。
    fn parse(text: &str) -> EndReason {
        match text {
            "completed" => EndReason::Completed,
            "interrupted" => EndReason::Interrupted,
            "error" => EndReason::Error,
            other => EndReason::Other(other.to_string()),
        }
    }
}
