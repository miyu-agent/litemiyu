//! 查用量的端口（施工 8-15，`docs/blueprint/tools/session_usage.md`、`tools/interface.md`）：`session_usage` 这件工具只拿它，
//! 不认识用量汇总和内核。执行器照这一次调用造一个，交给 [`crate::tool::Call::usage`]：
//!
//! - 上下文（[`UsagePort::context`]）：派出去那一刻内核照压缩线的算法估的用量、窗口、压缩线（`Session::context_used()`）。
//! - 干活的时间（[`UsagePort::busy_ms`]，施工 E-M 下）：内核照日志算的，等人不算。
//! - 用量（[`UsagePort::spent`]）：照这个会话的日志累计的（`session::current::Tally`，和界面底栏读的是同一份），派出去那一刻
//!   抄下。金额不算（设计第二节「不要的」）。
//!
//! 端口由下层定义、上层装（`00-设计理念.md` 第四节「依赖与接口的规矩」）。测试里的假调用没有，`session_usage` 照什么都没花答。

use std::fmt;

/// 查用量的那件工具的名字：本机的会话造会话时工具面上有它（「施工时定的」8-15）。
pub const SESSION_USAGE: &str = "session_usage";

/// 这个会话的用量、上下文。
pub trait UsagePort: Send + Sync {
    /// 上下文：派出去那一刻内核算的。没交过限额、策略里没有压缩的会话算不了，没有。
    fn context(&self) -> Option<ContextUse>;

    /// 派出去那一刻她在这个会话里干活的毫秒数（施工 E-M 下）：每一轮从开始到结束，等人批准、等人回答的那几段不算。拿不到的
    /// （测试里的假端口）没有。
    fn busy_ms(&self) -> Option<u64> {
        None
    }

    /// 这个会话到派出去那一刻为止的用量。
    fn spent(&self) -> Spent;
}

/// 上下文：用了多少、窗口、压缩线，都是 token 数。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContextUse {
    /// 用了多少：和压缩线同一个算法（锚加上锚以后的本地估算）。
    pub used: u64,
    /// 窗口；模型的资料没报的没有。
    pub window: Option<u64>,
    /// 压缩线：用量过了它就压；没有窗口的、窗口太小的没有。
    pub line: Option<u64>,
}

/// 用量：都是 token 数，请求是次数。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spent {
    /// 发出去的请求数。
    pub requests: u64,
    /// 输入：没命中、命中、写进缓存三项加起来。
    pub input: u64,
    /// 其中命中缓存的。
    pub cached: u64,
    /// 输出。
    pub output: u64,
}

/// 端口不打出里面的东西。
impl fmt::Debug for dyn UsagePort {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("UsagePort")
    }
}

/// 两个端口比的是不是同一个：[`crate::tool::Call`] 照格子比较时用。
impl PartialEq for dyn UsagePort {
    fn eq(&self, other: &dyn UsagePort) -> bool {
        std::ptr::addr_eq(self, other)
    }
}

impl Eq for dyn UsagePort {}
