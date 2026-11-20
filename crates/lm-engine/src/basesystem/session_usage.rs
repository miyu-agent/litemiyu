//! `session_usage`（`docs/blueprint/tools/session_usage.md`，施工 8-15，从 Miyu `miyu-basesystem` 搬来）：她自己查这个会话用了
//! 多少 token、上下文还剩多少。零参数，访问类别是读。经 [`Call::usage`] 交给执行器：用量照这个会话的日志累计（不带子会话），
//! 上下文是派出去那一刻内核照压缩线的算法估的。
//!
//! 输出一句一行：用量总有，后面接缓存命中了几成（有输入才写）；算得出上下文的写上下文，有窗口的写几成、有压缩线的另写一句
//! 压缩线，没窗口的写大约多少；最后写她干活用了多久（等人批准、等人回答的不算，时分秒）。Miyu 的金额、没价格的几次、读汇总
//! 失败那几句不搬（设计第二节「不要的」：harness 只记本会话的 token 和缓存命中）。

use crate::tool::{Call, ContextUse, Done, Progress, Running, SESSION_USAGE, Spec, Spent, Tool};
use lm_kernel::kernel::template::Template;
use lm_kernel::kernel::tool::Access;

use crate::basesystem::common::said;
use crate::basesystem::load::{self, LoadError, say};

/// `session_usage`。
pub(crate) struct SessionUsage {
    spec: Spec,
    texts: Texts,
}

/// 输出里给她看的几句：`software/basesystem/session_usage/*.txt`。
#[derive(Clone)]
struct Texts {
    usage: Template,
    context: Template,
    compaction: Template,
    no_window: Template,
    cache: Template,
    time: Template,
}

impl SessionUsage {
    /// 照编进程序的字造。访问类别是读：只读这个会话的累计，什么都不改。
    pub(crate) fn load() -> Result<SessionUsage, LoadError> {
        let text = |name: &str, fields: &[&str]| load::text(SESSION_USAGE, name, fields);
        Ok(SessionUsage {
            spec: load::spec(SESSION_USAGE, Access::Read)?,
            texts: Texts {
                usage: text("usage", &["requests", "input", "cached", "output"])?,
                context: text("context", &["used", "window"])?,
                compaction: text("compaction", &["line"])?,
                no_window: text("context-no-window", &["used"])?,
                cache: text("cache", &["percent"])?,
                time: text("time", &["time"])?,
            },
        })
    }
}

impl Tool for SessionUsage {
    fn spec(&self) -> &Spec {
        &self.spec
    }

    fn run(&self, call: Call, _progress: Progress) -> Running<'_> {
        let texts = self.texts.clone();
        Box::pin(async move {
            // 参数一个都不认：写了什么都不报错。没有端口的（测试里的假调用）照什么都没用答。
            let Some(port) = call.usage.clone() else {
                return Done::ok(texts.spent(&nothing(), None, None))
                    .said(said("session_usage/shown"));
            };
            if call.stop.stopped() {
                return Done::stopped();
            }
            Done::ok(texts.spent(&port.spent(), port.context(), port.busy_ms()))
                .said(said("session_usage/shown"))
        })
    }
}

impl Texts {
    /// 用量、缓存命中、上下文、干活用了多久，一句一行。
    fn spent(&self, spent: &Spent, context: Option<ContextUse>, busy_ms: Option<u64>) -> String {
        let number = |n: u64| n.to_string();
        let mut text = say(
            &self.usage,
            &[
                ("requests", &number(spent.requests)),
                ("input", &number(spent.input)),
                ("cached", &number(spent.cached)),
                ("output", &number(spent.output)),
            ],
        );
        if let Some(percent) = spent.cached.saturating_mul(100).checked_div(spent.input) {
            text.push_str(&say(&self.cache, &[("percent", &number(percent))]));
        }
        if let Some(context) = context {
            self.push_context(&mut text, context);
        }
        if let Some(ms) = busy_ms {
            text.push_str(&say(&self.time, &[("time", &hms(ms))]));
        }
        text
    }

    /// 上下文那一两句接在 `text` 后面。
    fn push_context(&self, text: &mut String, context: ContextUse) {
        let number = |n: u64| n.to_string();
        let used = number(context.used);
        match context.window {
            Some(window) => {
                text.push_str(&say(
                    &self.context,
                    &[("used", &used), ("window", &number(window))],
                ));
                if let Some(line) = context.line {
                    text.push_str(&say(&self.compaction, &[("line", &number(line))]));
                }
            }
            None => text.push_str(&say(&self.no_window, &[("used", &used)])),
        }
    }
}

/// 毫秒写成时分秒，零的那一截不写：`45s`、`2m 3s`、`1h 0m 5s`（统一时分秒，2026-09-23 项目主人定）。不到一秒的写 `0s`。
fn hms(ms: u64) -> String {
    let seconds = ms / 1000;
    let (h, m, s) = (seconds / 3600, seconds % 3600 / 60, seconds % 60);
    match (h, m) {
        (0, 0) => format!("{s}s"),
        (0, _) => format!("{m}m {s}s"),
        _ => format!("{h}h {m}m {s}s"),
    }
}

/// 什么都没用。
fn nothing() -> Spent {
    Spent {
        requests: 0,
        input: 0,
        cached: 0,
        output: 0,
    }
}

#[cfg(test)]
mod tests;
