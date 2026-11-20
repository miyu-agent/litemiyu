//! 后台任务的几种事件（Miyu 终端 `core/push/jobs.rs`，蓝图 `tui.md`「后台命令、子代理和侧边栏」）：工具结果 `effects` 里的
//! `job.started`，`job.reported`、`child.reported`。Miyu 在这里读核心发来的 JSON；这里读内核的事件本身。

use lm_kernel::kernel::event::{ChildReported, Effect, JobKind, JobReported};

/// 派出去一个任务（`job.started`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobStart {
    /// 派它的那一步的调用编号：后台命令照那一步 `shell` 的参数写命令。
    pub call_id: String,
    /// 任务编号，`j2`、`j2.1` 这样的。
    pub job: String,
    /// 是子代理；不是的是后台命令。
    pub agent: bool,
    /// 调用时写的 `description`。
    pub title: String,
    /// 子代理的会话；后台命令没有。
    pub session: Option<String>,
    /// fork 出来的子代理（LM-18，照 Miyu 终端 `core/push/jobs.rs`）：子代理状态行名字前面画分叉记号。
    pub fork: bool,
}

/// 后台命令结束了（`job.reported`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobEnd {
    /// 任务编号。
    pub job: String,
    /// 为什么结束：`exited`、`stopped`、`undone`、`restarted`、`aborted`、`timed_out`、`too_large`、`too_chatty`。
    pub reason: String,
    /// 退出码；被信号杀掉的没有。
    pub exit_code: Option<i32>,
    /// 杀掉它的信号。
    pub signal: Option<u32>,
    /// 跑了多久，毫秒；没有的照收到的时刻算。
    pub duration_ms: Option<u64>,
}

/// 子代理报回来了（`child.reported`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChildEnd {
    /// 任务编号。
    pub job: String,
    /// 子会话。
    pub session: String,
    /// 为什么报：`done`、`stopped`、`undone`、`aborted`。
    pub reason: String,
    /// 它交回来的回答。
    pub text: String,
}

/// 一次工具结果的效果里派出去的任务。前台跑的子代理（`foreground`）不是后台任务：派它的那一步等它报回来，不进任务表
/// （Miyu 蓝图同一节「前台跑的子代理」）。
pub fn started<'a>(call_id: &'a str, effects: &'a [Effect]) -> impl Iterator<Item = JobStart> + 'a {
    effects.iter().filter_map(move |effect| match effect {
        Effect::JobStarted(started) if !started.foreground => Some(JobStart {
            call_id: call_id.to_string(),
            job: started.job.to_string(),
            agent: started.what == JobKind::Agent,
            title: started.title.clone(),
            session: started.session.as_ref().map(|s| s.as_str().to_string()),
            fork: started.fork,
        }),
        _ => None,
    })
}

impl JobEnd {
    /// 读一条 `job.reported`。
    pub fn of(reported: &JobReported) -> JobEnd {
        JobEnd {
            job: reported.job.to_string(),
            reason: reported.reason.as_str().to_string(),
            exit_code: reported.exit_code,
            signal: reported.signal,
            duration_ms: reported.duration_ms,
        }
    }
}

impl ChildEnd {
    /// 读一条 `child.reported`。
    pub fn of(reported: &ChildReported) -> ChildEnd {
        ChildEnd {
            job: reported.job.to_string(),
            session: reported.session.as_str().to_string(),
            reason: reported.reason.as_str().to_string(),
            text: reported.text.clone(),
        }
    }
}
