//! 三种输出：[`Output`] 是共用的那几样；`text` 在 `text.rs`，`json`、`jsonl` 在这里。JSON 的格
//! 照名字的字母先后排（`serde_json` 的对象就是按名字排的），可有可无的格没有时整格不写。

use std::io::Write;

use serde_json::{Map, Value, json};

use super::ending::Ending;
use super::follow::{Failure, Follow, Out, Sink, Turn, Usage};
use crate::cli::words::Words;
use crate::core::ToolStatus;

/// 输出的那一头多的几样。
pub trait Output: Sink {
    /// 开好了会话、发出了这一句：编号，是不是这一次新开的。
    fn session(&mut self, id: &str, new: bool);
    /// 一轮结束了还有没报的子代理、后台命令，在等。
    fn waiting(&mut self, agents: usize, commands: usize);
    /// 收尾：`session` 是开好了的会话（开好以前就出错的没有）。
    fn finish(&mut self, ending: &Ending, follow: &Follow, session: Option<&str>);
}

/// 等的那一行：`· 等待 1 个子代理、2 个后台命令回报…`。
pub fn waiting_line(words: &Words, agents: usize, commands: usize) -> String {
    let count = |n: usize, one: &str, many: &str| {
        let key = if n == 1 { one } else { many };
        words.say(key, &[("count", &n.to_string())])
    };
    let what = match (agents, commands) {
        (0, n) => count(n, "wait.command", "wait.commands"),
        (n, 0) => count(n, "wait.agent", "wait.agents"),
        (a, c) => words.say(
            "wait.both",
            &[
                ("agents", &count(a, "wait.agent", "wait.agents")),
                ("commands", &count(c, "wait.command", "wait.commands")),
            ],
        ),
    };
    words.say("wait", &[("what", &what)])
}

/// 一份用量的四格。
fn usage(usage: Usage) -> Value {
    json!({
        "cache_read": usage.cache_read,
        "cache_write": usage.cache_write,
        "input": usage.input,
        "output": usage.output,
    })
}

/// 出错的那一格。
fn error(failure: &Failure) -> Value {
    json!({"class": failure.class, "message": failure.message})
}

/// 一轮：`json` 的 `turns` 里一项，`jsonl` 的 `turn.ended` 也照它（另加 `turn`、`type`）。
fn turn(turn: &Turn) -> Map<String, Value> {
    let mut map = Map::new();
    map.insert("reason".into(), json!(turn.reason));
    map.insert("text".into(), json!(turn.text));
    map.insert("usage".into(), usage(turn.usage));
    if let Some(failure) = &turn.error {
        map.insert("error".into(), error(failure));
    }
    map
}

/// 写一行到标准输出、冲掉（那头关了的不崩）。
fn line(value: &Value) {
    let mut out = std::io::stdout().lock();
    drop(writeln!(out, "{value}").and_then(|()| out.flush()));
}

/// 收尾时 `done`、`json` 共有的几格。
fn ended(map: &mut Map<String, Value>, ending: &Ending, session: Option<&str>) {
    map.insert("exit".into(), json!(ending.exit));
    if let Some(reason) = &ending.reason {
        map.insert("reason".into(), json!(reason));
    }
    if let Some(session) = session {
        map.insert("session".into(), json!(session));
    }
}

/// 有几步要确认没做时说的那句告警：一句、建议。
fn denied_warning(words: &Words, follow: &Follow) -> Option<(String, String)> {
    let n = follow.denied.len();
    let key = match n {
        0 => return None,
        1 => "denied.one",
        _ => "denied.many",
    };
    Some((
        words.say(key, &[("count", &n.to_string())]),
        words.get("denied.hint").to_string(),
    ))
}

/// `--format json`：全部结束时印一行。
pub struct Json<'w> {
    words: &'w Words,
}

impl<'w> Json<'w> {
    /// 一个。
    pub fn new(words: &'w Words) -> Json<'w> {
        Json { words }
    }
}

impl Sink for Json<'_> {
    fn put(&mut self, _out: Out<'_>) {}
}

impl Output for Json<'_> {
    fn session(&mut self, _id: &str, _new: bool) {}

    fn waiting(&mut self, _agents: usize, _commands: usize) {}

    fn finish(&mut self, ending: &Ending, follow: &Follow, session: Option<&str>) {
        let mut map = Map::new();
        ended(&mut map, ending, session);
        let turns: Vec<Value> = follow
            .turns
            .iter()
            .map(|t| Value::Object(turn(t)))
            .collect();
        map.insert("turns".into(), Value::Array(turns));
        if let Some(failure) = &ending.error {
            map.insert("error".into(), error(failure));
        }
        if !follow.denied.is_empty() {
            let denied: Vec<Value> = follow
                .denied
                .iter()
                .map(|(tool, title)| json!({"title": title, "tool": tool}))
                .collect();
            map.insert("denied".into(), Value::Array(denied));
        }
        line(&Value::Object(map));
        if let Some(said) = &ending.said {
            self.words.error(said, None);
        }
    }
}

/// `--format jsonl`：一行一个事件，边跑边出。
pub struct Jsonl<'w> {
    words: &'w Words,
}

impl<'w> Jsonl<'w> {
    /// 一个。
    pub fn new(words: &'w Words) -> Jsonl<'w> {
        Jsonl { words }
    }
}

impl Sink for Jsonl<'_> {
    fn put(&mut self, out: Out<'_>) {
        let value = match out {
            Out::TurnStarted(turn) => json!({"turn": turn, "type": "turn.started"}),
            Out::Thinking(text) => json!({"text": text, "type": "thinking"}),
            Out::Text(text) => json!({"text": text, "type": "text"}),
            Out::Step(step) => {
                json!({"id": step.id, "title": step.title, "tool": step.tool, "type": "step"})
            }
            Out::StepDone(step, done) => {
                let mut map = Map::new();
                map.insert("id".into(), json!(step.id));
                map.insert("ok".into(), json!(done.status == ToolStatus::Ok));
                if let Some(summary) = &done.summary {
                    map.insert("summary".into(), json!(summary));
                }
                map.insert("type".into(), json!("step.done"));
                Value::Object(map)
            }
            Out::AgentReported(job, _, text) => {
                json!({"agent": job, "text": text, "type": "agent.reported"})
            }
            Out::JobReported(job, _, reason, exit) => {
                let mut map = Map::new();
                if let Some(exit) = exit {
                    map.insert("exit".into(), json!(exit));
                }
                map.insert("job".into(), json!(job));
                map.insert("reason".into(), json!(reason));
                map.insert("type".into(), json!("job.reported"));
                Value::Object(map)
            }
            Out::Compaction(before, after) => {
                json!({"after": after, "before": before, "type": "compaction"})
            }
            // 进度只在终端里原地刷新（`text`），这里不出事件（同 Miyu）。
            Out::Compacting(_) => return,
            Out::CompactionFailed(why) => {
                let message = self.words.say("compact.failed", &[("reason", why)]);
                json!({"message": message, "type": "warning"})
            }
            Out::TurnEnded(ended) => {
                let mut map = turn(ended);
                map.insert("turn".into(), json!(ended.turn));
                map.insert("type".into(), json!("turn.ended"));
                Value::Object(map)
            }
        };
        line(&value);
    }
}

impl Output for Jsonl<'_> {
    fn session(&mut self, id: &str, new: bool) {
        line(&json!({"new": new, "session": id, "type": "session"}));
    }

    fn waiting(&mut self, _agents: usize, _commands: usize) {}

    fn finish(&mut self, ending: &Ending, follow: &Follow, session: Option<&str>) {
        if let Some((message, hint)) = denied_warning(self.words, follow) {
            line(&json!({"hints": [hint], "message": message, "type": "warning"}));
        }
        let mut map = Map::new();
        ended(&mut map, ending, session);
        map.insert("type".into(), json!("done"));
        line(&Value::Object(map));
        if let Some(said) = &ending.said {
            self.words.error(said, None);
        }
    }
}
