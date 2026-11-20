//! 本机时区（正文的钟、循环的时刻、会话的环境那一块）：做法在 [`lm_engine::zone`]（照 `/etc/localtime` 读一次，不走
//! zoneinfo 整个目录），引擎的运行日志、回收站也用它，挪过去两边共用。

pub use lm_engine::zone::{local, now};
