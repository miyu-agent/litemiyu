//! 量时间用的一个小写法（原来是运行日志里的几个写法，`28-运行日志.md`；LM-1 #058 去掉了没人订阅的运行日志，只剩这个）。

use std::time::Duration;

/// 一段时间，毫秒。
pub(crate) fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}
