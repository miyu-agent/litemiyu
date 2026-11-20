//! 一条后台命令的上限（施工 7-12 下，照 Claude Code）：最多跑多久（`jobs.max_running`，默认 30 分钟、最多 2 小时），输出
//! 最多多少字节（5 GB，不给改）。到了整组停掉，报 `timed_out`、`too_large`（`run.rs`）。

use std::time::Duration;

use crate::session::config::TurnConfig;
use crate::session::settings::JobSettings;

/// 输出最多多少字节（照 Claude Code）。
pub(crate) const OUTPUT_LIMIT: u64 = 5 * 1024 * 1024 * 1024;

/// 应急模式（设计第五节第 16 条）输出留在内存里，最多多少字节：8 MB。过了同 [`OUTPUT_LIMIT`] 照 `too_large` 停掉。给她看的照旧
/// 是读输出时一页页截的（`jobs` 一页最多 3 万个字，同前台命令的 `LIMIT`），8 MB 够两百多页；再大的日志该让命令自己写进文件。
pub(crate) const MEMORY_OUTPUT_LIMIT: u64 = 8 * 1024 * 1024;

/// 一条后台命令的上限。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct JobLimits {
    /// 最多跑多久。
    pub(crate) running: Duration,
    /// 输出最多多少字节。
    pub(crate) output: u64,
}

impl JobLimits {
    /// 照这一轮的配置 `config`。
    pub(crate) fn of(config: &TurnConfig) -> JobLimits {
        JobLimits {
            running: JobSettings::from(&config.resolved.values()).max_running,
            output: OUTPUT_LIMIT,
        }
    }
}
