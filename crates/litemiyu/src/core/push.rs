//! 把会话推来的一条事件读成界面关心的几样。纯函数，好测。
//!
//! Miyu 的终端在这里读核心发来的 JSON（`docs/blueprint/kernel/events.md` 的写法）；LiteMiyu 的引擎和界面在一个进程里，
//! 读的就是内核的事件本身（`lm_kernel::kernel::event`），一格一格照原来读 JSON 的那一套换成读字段。

mod compaction;
mod jobs;
pub mod replay;
mod shell;
mod transient;
mod usage;

use lm_kernel::kernel::block::Block as Content;
use lm_kernel::kernel::event::{
    ApprovalDecided, ApprovalRequested, Body, Event, QuestionAnswered, QuestionAsked, Said,
};
use lm_kernel::kernel::origin::By;

pub use compaction::Compaction;
pub use jobs::{ChildEnd, JobEnd, JobStart};
pub use shell::ShellRan;
pub use transient::read_transient;
pub use usage::Usage;

use super::Limits;
use super::kinds::{EndReason, ToolStatus};

/// 一次请求出的错（`model.called` 的 `error`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallError {
    /// 分类，例如 `auth`。
    pub class: String,
    /// 原话。
    pub message: String,
    /// HTTP 状态码；没有状态的（连不上、流里报的、内核自己造的）没有。
    pub status: Option<u16>,
    /// 还要等多久才恢复，毫秒（供应商的 retry-after、原话里的 try again in，池全在冷却的是最早恢复的那个）。
    pub retry_after_ms: Option<u64>,
}

impl CallError {
    /// 照内核记的读。
    fn of(error: &lm_kernel::kernel::event::CallError) -> Self {
        Self {
            class: error.class.as_str().to_string(),
            message: error.message.clone(),
            status: error.status,
            retry_after_ms: error.retry_after_ms,
        }
    }
}

/// 一块的种类：增量开头那一条的 `start`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Block {
    /// 回答。
    Text,
    /// 思考。
    Reasoning,
    /// 调一件工具，带着工具名。
    ToolCall(String),
}

/// 确认、提问的一条事件：界面照它开、收抽屉，正文里写结果。读成抽屉的形状在 `drawer/event.rs`。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Asking {
    /// `question.asked`：她问的几道题。
    Asked(QuestionAsked),
    /// `tool.approval_requested`：要确认的一次调用。
    Approval(ApprovalRequested),
    /// `question.answered`：答了。
    Answered(QuestionAnswered),
    /// `tool.approval_decided`：确认定了。
    Decided(ApprovalDecided),
}

/// 界面关心的一件事。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Push {
    /// 会话起了名字（`session.meta_changed` 的 `title`）；`named`：是人起的（`by` 是人），不是内核自动起的。
    Title {
        /// 名字。
        title: String,
        /// 是人起的。
        named: bool,
    },
    /// 一轮开始了，带着回合编号（那一条 `turn.started` 的序号）和开这一轮的那条消息的序号（`trigger`）。
    TurnStarted(u64, Option<u64>),
    /// 你说的一句落了盘（`message.user`），带着它的序号。
    UserMessage(u64),
    /// 这几句排着队的话退回来了（`message.withdrawn`）：序号。
    Withdrawn(Vec<u64>),
    /// 撤掉了这几轮（`turn.reverted`）：回合编号、这一条的序号。改过的文件内核不改回（Miyu 核心 U-1），日志里没有
    /// `files.restored`，不读。
    Reverted {
        /// 回合编号。
        turns: Vec<u64>,
        /// 这一条的序号。
        seq: u64,
        /// 撤它的那个命令（`cause`）：重做的撤销后面跟着同一个命令重发的话（[`Push::Resent`]）。
        cause: Option<String>,
    },
    /// 一句你说的话是某个命令记的（`message.user` 带 `cause`）：和最近一次撤销是同一个命令的，是重做（`/redo`、`/edit`）重发
    /// 的那句，重做不留撤销说明（Miyu 终端 `core/replay.rs` 的 `undo_events`）。
    Resent(String),
    /// 这几轮恢复了（`turn.unreverted`）：回合编号。
    Unreverted(Vec<u64>),
    /// 用了一条斜杠命令（`command.ran`）：不画；`/stop` 留着的排队话照它放进正文。
    CommandRan,
    /// 上次关窗口时循环还开着、这次重开停了（引擎记的 `command.ran` 的 `loop`，`by` 是内核）：正文里写一行，要再 `/loop`。
    LoopStopped,
    /// 序号是这个的那句话是人用 `!` 打的命令（`message.user` 的 `shell`）：跟在那一句的推送后面。
    ShellAsked(u64),
    /// 人用 `!` 打的命令跑完了（`shell.ran`）。
    ShellRan(ShellRan),
    /// 这一条是模型说的：哪个端点、哪个模型（事件的 `by`）。
    Model {
        /// 端点，例如 `deepseek`。
        endpoint: String,
        /// 模型名，例如 `deepseek-flash`。
        model: String,
    },
    /// 这一次请求带上了第几条为止（一块开头那一条增量的 `seen`）：排着队的话照它挪进正文。
    Heard(u64),
    /// 一句带斜杠命令的话落了盘（`message.user` 的 `command`）：序号、人打的那一行，照你说的画。
    Said {
        /// 落盘的序号。
        seq: u64,
        /// 说的字。
        text: String,
    },
    /// 一块开始了。
    BlockStart {
        /// 这一块在这一次请求里的序号。
        index: u64,
        /// 什么块。
        block: Block,
    },
    /// 一块里的一段字。
    Delta {
        /// 哪一块。
        index: u64,
        /// 这一段字。
        text: String,
    },
    /// 一块收全了。
    BlockEnd(u64),
    /// 这一次请求里她调的工具，照先后的调用编号（`message.assistant` 里的 `tool_call` 块）。
    Calls(Vec<String>),
    /// 一件工具的结果。
    ToolResult {
        /// 调用编号。
        call_id: String,
        /// 状态。
        status: ToolStatus,
        /// 结果里的字。
        text: String,
        /// 给人看的那一句。
        said: Option<Said>,
        /// 调用那一刻的显示名：语言到名字（MCP 这类别处来的工具写的；自带的工具没有）。
        display: std::collections::BTreeMap<String, String>,
        /// 给人看的结构化结果（工具交的一个对象，形状由那件工具定，例如 `web_search` 的链接）：点开照它画。
        detail: Option<serde_json::Value>,
    },
    /// 在跑的那一步的一段输出（瞬时的 `tool.progress`）。
    ToolProgress {
        /// 调用编号。
        call_id: String,
        /// 一段输出。
        text: String,
    },
    /// 确认、提问。
    Asking(Asking),
    /// 派出去一个后台任务（工具结果效果里的 `job.started`）：跟在那一步的结果后面。
    JobStarted(JobStart),
    /// 后台命令结束了（`job.reported`）。
    JobEnded(JobEnd),
    /// 监视的后台命令（`shell` 写了 `watch`）出了一批行（`job.lines`）：哪一个、这一批的字、原来几行。
    JobLines {
        /// 任务编号。
        job: String,
        /// 这一批的字。
        text: String,
        /// 原来几行。
        lines: u64,
    },
    /// 子代理报回来了（`child.reported`）。
    ChildEnded(ChildEnd),
    /// 现在能不能压缩（瞬时的 `compactable.changed`）。
    Compactable(bool),
    /// 一次请求的用量（`model.called` 的 `usage`，供应商没报的没有这一条）。
    Usage(Usage),
    /// 辅助请求（起标题这类，`model.called` 带 `purpose`）的用量：算进累计，不算上下文、缓存、速度。
    AuxUsage(Usage),
    /// 一次请求花了多少（`model.called` 的 `cost`）：币种、金额；报了用量、模型没价格的是 `None`。紧跟在那一次的用量后面。
    /// 主会话的账照引擎 `Handle::meter` 拿（`engine/forward.rs`），这一条只给子代理的账一笔笔加（`app/meter.rs`）。
    Billed(Option<(String, lm_kernel::kernel::event::Real)>),
    /// 会话换了模型（`session.policy_changed` 的 `model`，人换的，或者钉着的没了内核退回默认，`replaced` 是原来的）。
    ModelSet {
        /// 现在的引用。
        reference: String,
        /// 内核退回默认时原来的引用。
        replaced: Option<String>,
    },
    /// 会话自己定的思考程度换了（`session.policy_changed` 的 `effort`）：`None` 是回到跟着配置。
    EffortSet(Option<String>),
    /// 会话换了模型（瞬时的 `model.changed`）：一轮开始时照配置换的，或者出错换到别的端点。
    ModelChanged {
        /// 端点。
        endpoint: Option<String>,
        /// 模型。
        model: Option<String>,
        /// 换了以后的限额。
        limits: Option<Limits>,
        /// 现在的引用。
        reference: Option<String>,
        /// 用的思考强度。
        effort: Option<String>,
    },
    /// 会话这时的限额（`Handle::limits`，`engine/forward.rs`）：订阅上时交一次（同 Miyu 订阅回应里的 `limits`），以后变了再交。
    /// 引擎只在回合开始头看得到的变了时推 `model.changed`，开会话时就定好的限额不会另推。
    Limits(Limits),
    /// 这一次请求发出去了（`model.called` 带 `request`）。
    Sent {
        /// 看到第几条为止（`seen`）。
        seen: u64,
        /// 前缀和上一次请求比变了（带 `first_difference`），不是只往后接着加。
        changed: bool,
        /// 是压缩的摘要请求。
        summary: bool,
    },
    /// 压缩的几样。
    Compaction(Compaction),
    /// 压完了（`context.compacted`）。
    Compacted {
        /// 是清空。
        clear: bool,
    },
    /// 这一次回答落定时内核截掉了的（`model.called` 的 `cut`，内核 FT-1：模型在回答后面编的「下一轮」）：第几块（同增量的
    /// `index`）、从那一块原文的第几个字节起不要。界面把流式攒的那一块截到这里。
    Cut(Vec<(u64, usize)>),
    /// 出字的速度。
    Speed {
        /// 输出的 token 数。
        output: u64,
        /// 出字花的毫秒数：总用时减去等第一个字的时间。
        ms: u64,
    },
    /// 一次请求出错了。
    CallFailed(CallError),
    /// 一次请求成了。
    CallOk,
    /// 出错了等一会再来（瞬时的 `status` 带 `retry`）。
    Retry {
        /// 第几次。
        attempt: u64,
        /// 最多几次。
        limit: u64,
        /// 出错的分类（`stalled` 的原话前面接人话，Miyu `transcript/failure.rs` 的 `retry_line`）。
        class: String,
        /// 原话。
        message: String,
    },
    /// 一轮结束了。
    TurnEnded(EndReason),
    /// 别的会话发来的话（`message.user`，`by` 是会话，设计第五节第 8 条）：序号、哪个会话（整个编号）、说的字。
    Foreign {
        /// 落盘的序号。
        seq: u64,
        /// 发话的会话。
        from: String,
        /// 说的字。
        text: String,
    },
    /// 订了「空了告诉我」的会话空下来了、等作废了、没了、没开着（`peer.idle`）。
    PeerIdle {
        /// 等的是哪个会话（整个编号）。
        session: String,
        /// 为什么来：`idle`、`expired`、`gone`、`closed`，新版本才有的原样。
        reason: String,
        /// `idle` 的才有：它最近那一轮最后一句回复的第一行。
        status: Option<String>,
    },
    /// 补发来的事件的时刻（`replay.rs`）：正文照它算用时、写时刻；`None` 是补完了，回到照现在。
    Clock(Option<jiff::Timestamp>),
}

/// 读一条落了盘的事件，照先后交回界面关心的几样；界面不关心的是空的。
/// 人说的一句带着记它的命令（`cause`）：先交一条 [`Push::Resent`]，正文照它认重做重发的那句（补发也走这里，`replay.rs`）。
pub(super) fn resent(event: &Event, out: &mut Vec<Push>) {
    if let (Body::MessageUser(_), By::Person(_), Some(cause)) =
        (&event.body, &event.by, &event.cause)
    {
        out.push(Push::Resent(cause.to_string()));
    }
}

pub fn read(event: &Event) -> Vec<Push> {
    let mut out = Vec::new();
    if let By::Model(by) = &event.by {
        out.push(Push::Model {
            endpoint: by.endpoint.as_str().to_string(),
            model: by.model.as_str().to_string(),
        });
    }
    let seq = event.seq.get();
    resent(event, &mut out);
    match &event.body {
        Body::TurnStarted(started) => out.push(Push::TurnStarted(
            seq,
            started.trigger.map(lm_kernel::kernel::id::Seq::get),
        )),
        Body::MessageUser(message) if message.shell => {
            out.push(Push::UserMessage(seq));
            out.push(Push::ShellAsked(seq));
        }
        // 循环到点交进来的（`by: schedule`）：照你说的话画（同 Miyu 终端 `push/jobs.rs`：「正常和用户消息类似」）。
        Body::MessageUser(message) if matches!(event.by, By::Schedule(_)) => out.push(Push::Said {
            seq,
            text: message
                .blocks
                .iter()
                .filter_map(|block| match block {
                    Content::Text(text) => Some(text.text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n"),
        }),
        // 别的会话发来的（收件目录里读到的、派它的父会话留的言）：照别处来的话画。
        Body::MessageUser(message) if let By::Session(from) = &event.by => {
            out.push(Push::Foreign {
                seq,
                from: from.id.as_str().to_string(),
                text: message.text(),
            });
        }
        Body::PeerIdle(idle) => out.push(Push::PeerIdle {
            session: idle.session.as_str().to_string(),
            reason: idle.reason.as_str().to_string(),
            status: idle.status.clone(),
        }),
        Body::MessageUser(message) => out.push(match message.typed() {
            Some(text) if !text.is_empty() => Push::Said {
                seq,
                text: text.to_string(),
            },
            _ => Push::UserMessage(seq),
        }),
        Body::ShellRan(ran) => out.push(Push::ShellRan(ShellRan::of(ran))),
        // 重开会话时引擎记的「上次关窗口时循环还开着」（`by` 是内核，`lm_engine::session::looping::Trail`）：正文里写一行。
        Body::CommandRan(ran) if ran.command == "loop" && matches!(event.by, By::Kernel) => {
            out.push(Push::LoopStopped);
        }
        Body::CommandRan(_) => out.push(Push::CommandRan),
        Body::TurnReverted(reverted) => out.push(Push::Reverted {
            turns: reverted.turns.iter().map(|t| t.started().get()).collect(),
            seq,
            cause: event.cause.as_ref().map(ToString::to_string),
        }),
        Body::TurnUnreverted(unreverted) => out.push(Push::Unreverted(
            unreverted.turns.iter().map(|t| t.started().get()).collect(),
        )),
        Body::MessageWithdrawn(withdrawn) => out.push(Push::Withdrawn(
            withdrawn.messages.iter().map(|seq| seq.get()).collect(),
        )),
        Body::PolicyChanged(changed) => {
            if let Some(level) = changed.effort.as_ref().and_then(|e| e.session()) {
                out.push(Push::EffortSet(level.map(str::to_string)));
            }
            if let Some(reference) = &changed.model {
                out.push(Push::ModelSet {
                    reference: reference.clone(),
                    replaced: changed.replaced.clone(),
                });
            }
        }
        Body::MetaChanged(meta) => {
            if let Some(title) = &meta.title {
                // 人起的：`by` 是人，不是内核自动起的。
                let named = matches!(event.by, By::Person(_));
                out.push(Push::Title {
                    title: title.clone(),
                    named,
                });
            }
        }
        Body::ContextCompacted(compacted) => out.push(Push::Compacted {
            clear: compacted
                .trigger
                .as_ref()
                .is_some_and(|trigger| trigger.as_str() == "clear"),
        }),
        Body::CompactionPaused(paused) => out.push(Push::Compaction(Compaction::Paused {
            reason: paused.reason.as_str().to_string(),
            failures: paused.failures.map(u64::from),
            entry: paused.entry.map(lm_kernel::kernel::id::Seq::get),
        })),
        Body::TurnEnded(ended) => out.push(Push::TurnEnded(EndReason::of(&ended.reason))),
        Body::MessageAssistant(message) => {
            let calls = message
                .blocks
                .iter()
                .filter_map(|block| match block {
                    Content::ToolCall(call) => Some(call.call_id.to_string()),
                    _ => None,
                })
                .collect();
            out.push(Push::Calls(calls));
        }
        Body::QuestionAsked(asked) => out.push(Push::Asking(Asking::Asked(asked.clone()))),
        Body::ApprovalRequested(asked) => {
            out.push(Push::Asking(Asking::Approval(asked.clone())));
        }
        Body::QuestionAnswered(answered) => {
            out.push(Push::Asking(Asking::Answered(answered.clone())));
        }
        Body::ApprovalDecided(decided) => {
            out.push(Push::Asking(Asking::Decided(decided.clone())));
        }
        Body::ToolResult(result) => out.push(Push::ToolResult {
            call_id: result.call_id.to_string(),
            status: ToolStatus::of(&result.status),
            text: result
                .blocks
                .iter()
                .filter_map(|block| match block {
                    Content::Text(text) => Some(text.text.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("\n"),
            said: result.human.clone(),
            display: display_names(&result.display_name),
            detail: result
                .detail
                .as_ref()
                .and_then(|raw| lm_kernel::kernel::json::from_str(raw.get()).ok())
                .filter(serde_json::Value::is_object),
        }),
        Body::ModelCalled(called) => usage::called(called, &mut out),
        Body::JobReported(reported) => out.push(Push::JobEnded(JobEnd::of(reported))),
        Body::JobLines(lines) => out.push(Push::JobLines {
            job: lines.job.to_string(),
            text: lines.text.clone(),
            lines: lines.lines,
        }),
        Body::ChildReported(reported) => out.push(Push::ChildEnded(ChildEnd::of(reported))),
        _ => {}
    }
    if let Body::ToolResult(result) = &event.body {
        let call_id = result.call_id.to_string();
        out.extend(jobs::started(&call_id, &result.effects).map(Push::JobStarted));
    }
    out
}

/// 调用那一刻记下的显示名，去掉空的、两头的空白。一格格放进去，不 `collect`：`collect` 成 `BTreeMap` 要另展开一份稳定
/// 排序（瘦身，界面线第七趟），原来的表本来就照语言排好、不重名。
fn display_names(
    names: &std::collections::BTreeMap<String, String>,
) -> std::collections::BTreeMap<String, String> {
    let mut out = std::collections::BTreeMap::new();
    for (lang, name) in names {
        if !name.trim().is_empty() {
            out.insert(lang.clone(), name.trim().to_string());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use lm_kernel::kernel::event::Event;

    use super::{Push, read};

    fn event(json: &str) -> Event {
        lm_kernel::kernel::json::from_str(json).expect("事件的写法")
    }

    #[test]
    fn a_loop_message_reads_as_said_and_a_kernel_loop_off_as_stopped() {
        let woke = event(
            r#"{"seq":7,"at":"2026-11-12T08:00:00.000Z","kind":"message.user","by":{"kind":"schedule","schedule":"t1","loop":"fixed"},"cause":"schedule/t1/1/say","body":{"blocks":[{"type":"text","text":"看看 CI"}]}}"#,
        );
        assert_eq!(
            read(&woke),
            [Push::Said {
                seq: 7,
                text: "看看 CI".into()
            }]
        );
        let stopped = event(
            r#"{"seq":9,"at":"2026-11-12T08:00:00.000Z","kind":"command.ran","by":{"kind":"kernel"},"body":{"text":"/loop off","command":"loop"}}"#,
        );
        assert_eq!(read(&stopped), [Push::LoopStopped]);
        let typed = event(
            r#"{"seq":10,"at":"2026-11-12T08:00:00.000Z","kind":"command.ran","by":{"kind":"person"},"cause":"c3","body":{"text":"/loop off","command":"loop"}}"#,
        );
        assert_eq!(read(&typed), [Push::CommandRan], "人敲的照旧不画");
    }

    /// 旧日志里的长期目标（2026-11-14 砍了 `goal`）：`goal.*` 读成不认识的种类，什么都不推；续轮那句话照人说的画，
    /// 人敲的 `/goal` 照别的斜杠命令。
    #[test]
    fn an_old_goal_in_the_log_reads_without_breaking() {
        for kind in [
            "goal.set",
            "goal.changed",
            "goal.paused",
            "goal.resumed",
            "goal.ended",
        ] {
            let old = event(&format!(
                r#"{{"seq":3,"at":"2026-09-25T22:05:00.000Z","kind":"{kind}","by":{{"kind":"person"}},"cause":"cmd-1","body":{{"text":"修绿","limit":30,"outcome":"blocked"}}}}"#
            ));
            assert!(read(&old).is_empty(), "{kind}");
        }
        let round = event(
            r#"{"seq":4,"at":"2026-09-25T22:05:01.000Z","kind":"message.user","by":{"kind":"module","id":"goal"},"cause":"goal-1-0199a2b3-0000-7000-8000-000000000000","body":{"blocks":[{"type":"text","text":"Round 1"}]}}"#,
        );
        assert_eq!(read(&round), [Push::UserMessage(4)]);
        let typed = event(
            r#"{"seq":5,"at":"2026-09-25T22:05:02.000Z","kind":"command.ran","by":{"kind":"person"},"cause":"c4","body":{"text":"/goal 修绿","command":"goal"}}"#,
        );
        assert_eq!(read(&typed), [Push::CommandRan]);
    }

    /// 旧日志里的待办（2026-11-14 砍了 `todowrite`）：人清掉待办的 `todos.cleared` 读成不认识的种类，什么都不推；`todowrite`
    /// 的结果照别的工具结果推，带着的 `todo.written` 不另推什么。
    #[test]
    fn an_old_todo_list_in_the_log_reads_without_breaking() {
        let cleared = event(
            r#"{"seq":167,"at":"2026-09-25T22:08:00.000Z","kind":"todos.cleared","by":{"kind":"person"},"cause":"cmd-8a21","body":{}}"#,
        );
        assert!(read(&cleared).is_empty());
        let result = event(
            r#"{"seq":168,"at":"2026-09-25T22:08:01.000Z","kind":"tool.result","turn":160,"by":{"kind":"tool","call_id":"call_167_1"},"body":{"call_id":"call_167_1","status":"ok","blocks":[{"type":"text","text":"Todo list updated: 1 of 2 done."}],"human":{"key":"software/basesystem/todowrite/updated","fields":{"done":"1","total":"2"}},"effects":[{"kind":"todo.written","todos":[{"content":"读代码","status":"completed"},{"content":"写测试","status":"in_progress"}]}]}}"#,
        );
        let pushed = read(&result);
        assert!(
            matches!(
                pushed.as_slice(),
                [Push::ToolResult { call_id, text, said: Some(_), .. }]
                    if call_id == "call_167_1" && text == "Todo list updated: 1 of 2 done."
            ),
            "{pushed:?}"
        );
    }
}
