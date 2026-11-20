//! 基础工具的集成测试并成一个程序（照 Miyu `miyu-basesystem/tests/all.rs`）：一个文件一个模块，`support` 只编一次。Miyu 的
//! 这一组照 L3 四搬到哪件工具就搬哪几份（执行计划第六节 L3 四）。

mod support;

mod ask_user;
mod background;
mod checked;
mod edit;
mod jobs;
mod read;
mod read_image;
mod session_usage;
mod shell;
mod shell_seen;
mod wakeup;
mod watch;
