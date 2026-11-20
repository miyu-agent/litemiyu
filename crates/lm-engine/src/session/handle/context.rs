//! 会话的上下文上限（Miyu 施工 CTX-1、CTX-2，设计 68 第四节）：界面的 `/context <数值>`、`/context unset`，命令行照同一份。
//! Miyu 在协议那一层（`commands/context.rs` 认参数、`models/context.rs` 查太小的）；LiteMiyu 没有协议，搬成会话把手上的函数。
//!
//! 设得太小、照它算不出压缩线的拒（交回最少要多少）：最少要多少由内核照这时的策略和端口交的最大输出算
//! （`Session::least_context`，和压缩线同一个算法），什么都不记。策略里没有压缩的不主动压，设多少都行，只拒 0。去掉不查。
//! 设好以后下一次请求起生效，给头看的限额（[`Handle::limits`] 的 `window`、`compaction_line`、`context`）当场换。

use lm_kernel::kernel::id::CommandId;
use lm_kernel::kernel::origin::By;
use lm_kernel::kernel::session::{Command, Outcome, Reason};

use super::{Handle, Stopped};

/// 设上限没设成。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextRefused {
    /// 比最少要的还小（或者是 0）：最少要多少（Miyu 的 `bad_params`，`data.min`）。
    TooSmall {
        /// 最少要多少 token。
        min: u64,
    },
    /// 内核拒了（会话封存了这类）。
    Rejected(Reason),
}

impl Handle {
    /// 设这个会话的上下文上限：`tokens` 是上限（`None` 是去掉，照模型的窗口）。`id` 是命令编号
    /// （[`crate::session::new_command_id`]），`by` 是设的人。和现在一样的也接受、什么都不记。
    ///
    /// # Errors
    ///
    /// 会话停了。
    pub async fn set_context(
        &self,
        id: CommandId,
        by: By,
        tokens: Option<u64>,
    ) -> Result<Result<(), ContextRefused>, Stopped> {
        if let Some(tokens) = tokens {
            let least = self.current().await?.least_context.unwrap_or(1).max(1);
            if tokens < least {
                return Ok(Err(ContextRefused::TooSmall { min: least }));
            }
        }
        Ok(
            match self.command(id, by, Command::SetContext { tokens }).await? {
                Outcome::Rejected { reason, .. } => Err(ContextRefused::Rejected(reason)),
                _ => Ok(()),
            },
        )
    }
}

impl Handle {
    /// 钉这个会话的压缩触发线（Miyu 施工 CT-1，`session.configure` 的 `compaction_line`；界面的 `/compact-line`）：`percent` 是窗口的
    /// 百分之几，`None` 是去掉、回到跟着配置 `compaction.line`。下一个回合开始生效；和现在一样的也接受、什么都不记。范围照
    /// `compaction.line` 声明的（[`crate::session::settings::CompactionSettings::line_range`]，20 到 95），不在范围里的交回
    /// `Err(LineRefused::OutOfRange)`，什么都不记。
    ///
    /// # Errors
    ///
    /// 会话停了。
    pub async fn set_compaction_line(
        &self,
        id: CommandId,
        by: By,
        percent: Option<u64>,
    ) -> Result<Result<(), LineRefused>, Stopped> {
        let range = crate::session::settings::CompactionSettings::line_range();
        if let Some(percent) = percent
            && !range.contains(&percent)
        {
            return Ok(Err(LineRefused::OutOfRange {
                min: *range.start(),
                max: *range.end(),
            }));
        }
        let command = Command::SetCompactionLine { percent };
        Ok(match self.command(id, by, command).await? {
            Outcome::Rejected { reason, .. } => Err(LineRefused::Rejected(reason)),
            _ => Ok(()),
        })
    }
}

/// 钉压缩触发线没钉成。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LineRefused {
    /// 不在 `compaction.line` 的范围里（Miyu 的 `bad_params`，`data.field` 是 `compaction_line`）。
    OutOfRange {
        /// 最小。
        min: u64,
        /// 最大。
        max: u64,
    },
    /// 内核拒了（会话封存了这类）。
    Rejected(Reason),
}

/// `/context` 后面跟的字（Miyu `commands/context.rs` 的 `parsed`）：`unset`、`off` 是去掉（`Some(None)`），别的是数值：纯数字，
/// 也收 `128k`、`1m`（`k` 是千、`m` 是百万，大小写都行）。别的写法、大得装不下的没有。
pub fn parse_context(rest: &str) -> Option<Option<u64>> {
    let rest = rest.to_ascii_lowercase();
    if rest == "unset" || rest == "off" {
        return Some(None);
    }
    let (digits, unit) = match rest.strip_suffix('k') {
        Some(digits) => (digits, 1_000),
        None => match rest.strip_suffix('m') {
            Some(digits) => (digits, 1_000_000),
            None => (rest.as_str(), 1),
        },
    };
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    digits
        .parse::<u64>()
        .ok()
        .and_then(|number| number.checked_mul(unit))
        .map(Some)
}

/// 给人看的约数，往下取（Miyu `commands/context.rs` 的 `about`）：不到一千的照写，不到一百万的写成多少 `k`，再大的写成多少
/// `m`（留一位小数，`.0` 不写）。
pub fn about_tokens(tokens: u64) -> String {
    if tokens < 1_000 {
        return tokens.to_string();
    }
    if tokens < 1_000_000 {
        return format!("{}k", tokens / 1_000);
    }
    let tenths = tokens / 100_000;
    match tenths % 10 {
        0 => format!("{}m", tenths / 10),
        digit => format!("{}.{digit}m", tenths / 10),
    }
}

#[cfg(test)]
mod tests;
