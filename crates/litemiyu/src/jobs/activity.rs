//! 子代理正在做什么、用了多少（Miyu 终端 `app/sessions/notes.rs` 的 `doing`、`tokens`，蓝图 `tui.md`「后台命令、子代理和
//! 侧边栏」第 6 条）：子代理状态行照它写。
//!
//! 和 Miyu 不一样的：Miyu 每个子会话停放着一份正文，照正文最后那一步算；这里不留子会话的正文，照它推来的几样记最后在做的
//! （一轮开没开、最后一块是想、是说还是调工具，工具调用的参数收齐了取它的对象），推完一条就只剩这几格。

use crate::core::{Block, Push};
use crate::human::Human;

/// 一个子代理这一刻在做什么、一共用了多少。后台命令一直是空的。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Activity {
    /// 一轮开了、还没完。
    pub busy: bool,
    /// 最后在做的。
    pub doing: Doing,
    /// 一共用了多少 token：每次请求的输入加输出（同 Miyu `tokens`，辅助请求不算）。
    pub tokens: u64,
    /// 正在收的工具调用：哪一块、参数收到哪儿了；收齐了就放掉。
    call: Option<(u64, String)>,
    /// 它自己又派出去、还在跑的子代理（任务编号）：状态行名字后面写「（+N）」（Miyu `Talk::nested_agents`）。
    nested: Vec<String>,
}

/// 最后在做的。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum Doing {
    /// 还没做什么。
    #[default]
    Nothing,
    /// 在想。
    Thinking,
    /// 在说。
    Replying,
    /// 调一件工具：工具名，参数里它的对象（Miyu `Face.subject` 指的那一项；`shell` 没写的照 `description`）。
    Tool {
        /// 工具名。
        name: String,
        /// 对象；参数还没收齐、没有这一项的是 `None`。
        subject: Option<String>,
    },
}

impl Activity {
    /// 子会话推来一条：记下它在做什么、用了多少。
    pub fn push(&mut self, push: &Push, human: &Human) {
        match push {
            Push::TurnStarted(..) => self.busy = true,
            Push::TurnEnded(_) => {
                self.busy = false;
                self.call = None;
            }
            Push::BlockStart { index, block } => {
                self.doing = match block {
                    Block::Reasoning => Doing::Thinking,
                    Block::Text => Doing::Replying,
                    Block::ToolCall(name) => {
                        self.call = Some((*index, String::new()));
                        Doing::Tool {
                            name: name.clone(),
                            subject: None,
                        }
                    }
                };
            }
            Push::Delta { index, text } => {
                if let Some((at, args)) = &mut self.call
                    && at == index
                {
                    args.push_str(text);
                }
            }
            Push::BlockEnd(index) => {
                if self.call.as_ref().is_some_and(|(at, _)| at == index)
                    && let Some((_, args)) = self.call.take()
                    && let Doing::Tool { name, subject } = &mut self.doing
                {
                    *subject = subject_of(name, &args, human);
                }
            }
            Push::Usage(usage) => self.tokens += usage.input() + usage.output,
            // 它又派的子代理：派出去记上，报回来了去掉（报完了被留言叫醒的，同一个编号又来一次）。
            Push::JobStarted(started) if started.agent && !self.nested.contains(&started.job) => {
                self.nested.push(started.job.clone());
            }
            Push::ChildEnded(end) => self.nested.retain(|job| *job != end.job),
            _ => {}
        }
    }

    /// 它自己又派出去、还在跑的子代理有几个。
    pub fn nested(&self) -> usize {
        self.nested.len()
    }
}

/// 参数里给人看的那一项：表里写了是哪一项的照表，`shell` 没写的照 `description`；不是字的、没有的是 `None`。
fn subject_of(name: &str, args: &str, human: &Human) -> Option<String> {
    let key = human
        .tool(name)
        .and_then(|face| face.subject.as_deref())
        .or((name == "shell").then_some("description"))?;
    let args: serde_json::Value = lm_kernel::kernel::json::from_str(args).ok()?;
    args.get(key)?.as_str().map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::{Activity, Doing};
    use crate::core::{Block, ChildEnd, EndReason, JobStart, Push, Usage};
    use crate::human::Human;

    #[test]
    fn the_last_block_says_what_it_is_doing_and_usage_adds_up() {
        let human = Human::default();
        let mut a = Activity::default();
        a.push(&Push::TurnStarted(1, None), &human);
        assert!(a.busy);
        a.push(
            &Push::BlockStart {
                index: 0,
                block: Block::Reasoning,
            },
            &human,
        );
        assert_eq!(a.doing, Doing::Thinking);
        let call = Push::BlockStart {
            index: 1,
            block: Block::ToolCall("shell".into()),
        };
        a.push(&call, &human);
        for text in [r#"{"command":"sleep 9","#, r#""description":"Wait a bit"}"#] {
            let delta = Push::Delta {
                index: 1,
                text: text.into(),
            };
            a.push(&delta, &human);
        }
        let named = |subject: Option<&str>| Doing::Tool {
            name: "shell".into(),
            subject: subject.map(str::to_string),
        };
        assert_eq!(a.doing, named(None), "参数没收齐");
        a.push(&Push::BlockEnd(1), &human);
        assert_eq!(a.doing, named(Some("Wait a bit")), "shell 照 description");
        let usage = Usage {
            uncached: 10,
            cache_read: 5,
            cache_write: 0,
            output: 7,
            aux: 0,
        };
        a.push(&Push::Usage(usage), &human);
        a.push(&Push::AuxUsage(usage), &human);
        assert_eq!(a.tokens, 22, "辅助请求不算");
        a.push(&Push::TurnEnded(EndReason::Completed), &human);
        assert!(!a.busy);
    }

    /// Miyu `Talk::nested_agents`：子代理自己又派了的，照它在跑的数（状态行写「（+N）」）；后台命令不算，报回来了的去掉，
    /// 同一个编号又来一次不重数。
    #[test]
    fn nested_agents_are_counted_until_they_report() {
        let human = Human::default();
        let mut a = Activity::default();
        let started = |job: &str, agent: bool| {
            Push::JobStarted(JobStart {
                call_id: format!("call_{job}"),
                job: job.into(),
                agent,
                title: String::new(),
                session: agent.then(|| format!("s-{job}")),
                fork: false,
            })
        };
        a.push(&started("j1.1", true), &human);
        a.push(&started("j1.2", true), &human);
        a.push(&started("j1.3", false), &human);
        a.push(&started("j1.1", true), &human);
        assert_eq!(a.nested(), 2);
        let ended = ChildEnd {
            job: "j1.1".into(),
            session: "s-j1.1".into(),
            reason: "done".into(),
            text: String::new(),
        };
        a.push(&Push::ChildEnded(ended), &human);
        assert_eq!(a.nested(), 1);
    }
}
