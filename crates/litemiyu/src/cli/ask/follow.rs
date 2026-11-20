//! 跟着会话推来的（[`Push`]）记账：这一轮的回答、思考、每一步、用量、出错，派出去还没报的子代理和后台命令；记的同时交给
//! 输出的那一头（[`Sink`]，三种格式各一个，`out.rs`）。照 Miyu `ask/follow.rs` 的账，读的是全屏同一份推送。

use std::collections::BTreeMap;

use super::steps::{self, Step};
use crate::core::push::Compaction;
use crate::core::{Block, EndReason, Push, ToolStatus};
use crate::human::Human;

/// 一次请求的用量（Miyu `--format json` 的四格）。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Usage {
    /// 加起来的输入：没命中 + 命中 + 写进缓存。
    pub input: u64,
    /// 命中缓存的。
    pub cache_read: u64,
    /// 写进缓存的。
    pub cache_write: u64,
    /// 输出。
    pub output: u64,
}

impl Usage {
    /// 加上另一份。
    pub fn add(&mut self, other: Usage) {
        self.input += other.input;
        self.cache_read += other.cache_read;
        self.cache_write += other.cache_write;
        self.output += other.output;
    }
}

/// 一次请求出的错：分类、原话。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    /// 分类（`no_model`、`auth`、`retryable`……）。
    pub class: String,
    /// 原话。
    pub message: String,
}

/// 结束了的一轮。
#[derive(Debug, Clone)]
pub struct Turn {
    /// 回合编号：开轮那一条事件的序号。
    pub turn: u64,
    /// 结束的原因，照引擎的原样。
    pub reason: String,
    /// 这一轮的全部回答。
    pub text: String,
    /// 这一轮的用量。
    pub usage: Usage,
    /// 有没有一次供应商报过用量。
    pub reported: bool,
    /// 最后一次请求出的错。
    pub error: Option<Failure>,
    /// 开这一轮的那句话（看会话时从日志读到的；跟着说的那一轮没有）。
    pub user: Option<String>,
    /// 这一轮的每一步：步、怎么结束的。
    pub steps: Vec<(Step, Done)>,
}

/// 一步结束了。
#[derive(Debug, Clone)]
pub struct Done {
    /// 状态。
    pub status: ToolStatus,
    /// 结果那一句。
    pub summary: Option<String>,
    /// 结果里的字（她看到的）。
    pub text: String,
}

/// 交给输出那一头的一样。
#[derive(Debug)]
pub enum Out<'a> {
    /// 跟的一轮开始了。
    TurnStarted(u64),
    /// 思考的一段。
    Thinking(&'a str),
    /// 回答的一段。
    Text(&'a str),
    /// 一步开始。
    Step(&'a Step),
    /// 一步结束。
    StepDone(&'a Step, &'a Done),
    /// 等的子代理报回来了：任务编号、标题、它最后说的。
    AgentReported(&'a str, &'a str, &'a str),
    /// 等的后台命令报回来了：任务编号、标题、原因、退出码。
    JobReported(&'a str, &'a str, &'a str, Option<i32>),
    /// 压缩了：压之前、压完的 token 数。
    Compaction(u64, u64),
    /// 正在压缩：摘要收到了多少字。
    Compacting(u64),
    /// 压缩失败：原话。
    CompactionFailed(&'a str),
    /// 一轮结束了。
    TurnEnded(&'a Turn),
}

/// 输出的那一头。
pub trait Sink {
    /// 交一样。
    fn put(&mut self, out: Out<'_>);
}

/// 还没报的一个任务。
#[derive(Debug, Clone)]
pub struct Job {
    /// 子代理（不是的是后台命令）。
    pub agent: bool,
    /// 派它时的标题。
    pub title: String,
}

/// 正在跟的一轮的账。
#[derive(Debug, Default)]
struct Running {
    turn: u64,
    user: Option<String>,
    steps: Vec<(Step, Done)>,
    text: String,
    usage: Usage,
    reported: bool,
    error: Option<Failure>,
}

/// 记账的。
pub struct Follow {
    human: Human,
    /// 这一次请求里的几块：块的序号到它是什么、工具调用攒着的参数。
    blocks: BTreeMap<u64, (Block, String)>,
    /// 开了还没结果的步。
    steps: BTreeMap<String, Step>,
    running: Option<Running>,
    /// 人说的、还没有一轮开起来的那句话。
    said: Option<String>,
    /// 结束了的几轮，照先后。
    pub turns: Vec<Turn>,
    /// 派出去还没报的任务。
    pub jobs: BTreeMap<String, Job>,
    /// 开始回话的请求有几次（这一次请求头一段字到了算一次）：`--max-steps` 照它，第几次一开口就知道。
    pub opened: u64,
    /// 上一次请求结束以后有没有开口过。
    heard: bool,
    /// 因为没人确认没做的几步：工具名、标题。
    pub denied: Vec<(String, String)>,
    /// 命令行替人拒了的调用（`--yes` 没写、开关交到以前就问了的）：它们的结果也算没人确认。
    pub refused: Vec<String>,
}

impl Follow {
    /// 给人看的字照 `language`。
    pub fn new(language: &str) -> Follow {
        Follow {
            human: Human::load(language),
            blocks: BTreeMap::new(),
            steps: BTreeMap::new(),
            running: None,
            said: None,
            turns: Vec::new(),
            jobs: BTreeMap::new(),
            opened: 0,
            heard: false,
            denied: Vec::new(),
            refused: Vec::new(),
        }
    }

    /// 有一轮在进行。
    pub fn busy(&self) -> bool {
        self.running.is_some()
    }

    /// 记一份推送，交给 `sink`。
    pub fn push(&mut self, push: Push, sink: &mut dyn Sink) {
        match push {
            Push::TurnStarted(seq, _) => {
                self.running = Some(Running {
                    turn: seq,
                    user: self.said.take(),
                    ..Running::default()
                });
                sink.put(Out::TurnStarted(seq));
            }
            Push::Said { text, .. } if self.running.is_none() => self.said = Some(text),
            Push::BlockStart { index, block } => {
                self.blocks.insert(index, (block, String::new()));
            }
            Push::Delta { index, text } => self.delta(index, &text, sink),
            Push::Calls(ids) => self.calls(ids, sink),
            Push::ToolResult {
                call_id,
                status,
                text,
                said,
                display,
                ..
            } => {
                let step = self.steps.remove(&call_id).unwrap_or_else(|| {
                    let name = self.human.step_name("", &display, "en");
                    Step::new(&self.human, call_id.clone(), name, "")
                });
                let unattended = said
                    .as_ref()
                    .is_some_and(|said| said.key == "core/tool-results/unattended");
                if status == ToolStatus::Denied && (unattended || self.refused.contains(&call_id)) {
                    self.denied.push((step.tool.clone(), step.title.clone()));
                }
                let done = Done {
                    status,
                    summary: steps::summary(&self.human, said.as_ref()),
                    text,
                };
                sink.put(Out::StepDone(&step, &done));
                if let Some(running) = &mut self.running {
                    running.steps.push((step, done));
                }
            }
            Push::Usage(usage) => {
                if let Some(running) = &mut self.running {
                    running.reported = true;
                    running.usage.add(Usage {
                        input: usage.input(),
                        cache_read: usage.cache_read,
                        cache_write: usage.cache_write,
                        output: usage.output,
                    });
                }
            }
            Push::CallFailed(error) => {
                if let Some(running) = &mut self.running {
                    running.error = Some(Failure {
                        class: error.class,
                        message: error.message,
                    });
                }
            }
            Push::CallOk => {
                if let Some(running) = &mut self.running {
                    running.error = None;
                }
            }
            Push::Heard(_) if !self.heard => {
                self.heard = true;
                self.opened += 1;
            }
            Push::Sent { .. } => {
                self.heard = false;
                self.blocks.clear();
            }
            Push::JobStarted(started) => {
                let job = Job {
                    agent: started.agent,
                    title: started.title,
                };
                self.jobs.insert(started.job, job);
            }
            Push::JobEnded(ended) => {
                if let Some(job) = self.jobs.remove(&ended.job) {
                    sink.put(Out::JobReported(
                        &ended.job,
                        &job.title,
                        &ended.reason,
                        ended.exit_code,
                    ));
                }
            }
            Push::ChildEnded(ended) => {
                if let Some(job) = self.jobs.remove(&ended.job) {
                    sink.put(Out::AgentReported(&ended.job, &job.title, &ended.text));
                }
            }
            Push::Compaction(Compaction::Done { before, after, .. }) => {
                sink.put(Out::Compaction(before, after));
            }
            Push::Compaction(Compaction::Progress { written, .. }) => {
                sink.put(Out::Compacting(written));
            }
            Push::Compaction(Compaction::Failed(error)) => {
                sink.put(Out::CompactionFailed(&error.message));
            }
            Push::TurnEnded(reason) => self.ended(&reason, sink),
            _ => {}
        }
    }

    /// 一段字：回答的、思考的交出去，工具调用的攒参数。
    fn delta(&mut self, index: u64, text: &str, sink: &mut dyn Sink) {
        let Some((block, args)) = self.blocks.get_mut(&index) else {
            return;
        };
        match block {
            Block::Text => {
                if let Some(running) = &mut self.running {
                    running.text.push_str(text);
                }
                sink.put(Out::Text(text));
            }
            Block::Reasoning => sink.put(Out::Thinking(text)),
            Block::ToolCall(_) => args.push_str(text),
        }
    }

    /// 她的回复落了盘：工具调用的块照先后对上调用编号，一步步开始。
    fn calls(&mut self, ids: Vec<String>, sink: &mut dyn Sink) {
        let calls: Vec<(String, String)> = std::mem::take(&mut self.blocks)
            .into_values()
            .filter_map(|(block, args)| match block {
                Block::ToolCall(name) => Some((name, args)),
                _ => None,
            })
            .collect();
        // 前一段回答没换行的，接着的回答隔着步骤补一个换行（Miyu `--format json` 的 `text`）。
        if let Some(running) = &mut self.running
            && !ids.is_empty()
            && !running.text.is_empty()
            && !running.text.ends_with('\n')
        {
            running.text.push('\n');
        }
        for (k, id) in ids.into_iter().enumerate() {
            let (tool, args) = calls.get(k).cloned().unwrap_or_default();
            let step = Step::new(&self.human, id.clone(), tool, &args);
            sink.put(Out::Step(&step));
            self.steps.insert(id, step);
        }
    }

    /// 一轮结束了。
    fn ended(&mut self, reason: &EndReason, sink: &mut dyn Sink) {
        let Some(running) = self.running.take() else {
            return;
        };
        let reason = match reason {
            EndReason::Completed => "completed".to_string(),
            EndReason::Interrupted => "interrupted".to_string(),
            EndReason::Error => "error".to_string(),
            EndReason::Other(other) => other.clone(),
        };
        let turn = Turn {
            turn: running.turn,
            reason,
            text: running.text,
            usage: running.usage,
            reported: running.reported,
            error: running.error,
            user: running.user,
            steps: running.steps,
        };
        sink.put(Out::TurnEnded(&turn));
        self.turns.push(turn);
    }

    /// 跟过的几轮加起来的用量；一次都没报过的是 `None`。
    pub fn total(&self) -> Option<Usage> {
        let mut total = Usage::default();
        let mut any = false;
        for turn in &self.turns {
            any |= turn.reported;
            total.add(turn.usage);
        }
        any.then_some(total)
    }
}
