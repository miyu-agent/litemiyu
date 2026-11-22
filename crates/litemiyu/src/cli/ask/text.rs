//! `--format text`（照 Miyu `cli/ask.md`「样子：`--format text`」）：标准输出只有回答；思考、每一步、用量、最后那几句都在标准
//! 错误上（旁白，灰）。标准错误是终端、`NO_COLOR` 没设或者是空的才上色；标准输出从不上色。
//!
//! 接终端的那一头（标准输出、标准错误各看各的），回答、思考、每一步的标题和摘要里的终端控制序列洗掉再写（`wash`，审计 #064）；
//! 接管道、写进文件的照原样。

use std::io::Write;

use super::ending::Ending;
use super::follow::{Done, Follow, Out, Sink, Usage};
use super::out::Output;
use super::steps::Step;
use super::wash::Wash;
use crate::cli::words::Words;
use crate::core::ToolStatus;
use crate::human::FaceBlock;

/// 灰、红、回到原色。
const GRAY: &str = "\x1b[90m";
const RED: &str = "\x1b[31m";
const GREEN: &str = "\x1b[32m";
const RESET: &str = "\x1b[0m";

/// 标准错误上最后印的是什么：管空行。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Last {
    /// 还什么都没印，或者刚印了一个空行。
    Blank,
    /// 一行旁白（一步、等的那一行）。
    Line,
    /// 一段思考没收尾。
    Thinking,
    /// 一块（执行命令的输出、一段思考）收了尾：下一样来之前空一行。
    Block,
}

/// 纯文本的那一头。
pub struct Text<'w> {
    words: &'w Words,
    color: bool,
    last: Last,
    /// 回答那一行没完。
    open: bool,
    /// 等的那一行印过了（不是终端的只印一次）。
    waited: bool,
    /// 标准错误是终端：压缩的进度原地刷新。
    terminal: bool,
    /// 压缩进度那一行还在（没换行）：下一样印之前先擦掉。
    progress: std::cell::Cell<bool>,
    /// 标准输出是终端：回答一路洗着写。
    answer: Option<Wash>,
    /// 标准错误是终端：思考一路洗着写。
    thought: Option<Wash>,
}

impl<'w> Text<'w> {
    /// 照标准错误是不是终端、`NO_COLOR` 定上不上色。
    pub fn new(words: &'w Words) -> Text<'w> {
        use std::io::IsTerminal;
        let no_color = std::env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty());
        let terminal = std::io::stderr().is_terminal();
        Text {
            words,
            terminal,
            answer: std::io::stdout().is_terminal().then(Wash::default),
            thought: terminal.then(Wash::default),
            progress: std::cell::Cell::new(false),
            color: terminal && !no_color,
            last: Last::Blank,
            open: false,
            waited: false,
        }
    }

    /// 同 [`Text::new`]，但是不看标准输出、标准错误是不是终端，一律照接着终端办（shell 集成的后台进程：它的标准输出是一条
    /// 转交给终端的套接字，印的字最后到的是终端）。回答、思考里的终端控制序列照样洗掉（审计 2026-11-18 #064）。
    pub fn relayed(words: &'w Words) -> Text<'w> {
        let no_color = std::env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty());
        Text {
            terminal: true,
            answer: Some(Wash::default()),
            thought: Some(Wash::default()),
            color: !no_color,
            ..Text::new(words)
        }
    }

    /// 标准错误上写一段（写不进的没处可报）。
    fn err(&self, text: &str) {
        let mut err = std::io::stderr().lock();
        if self.progress.replace(false) {
            // 写不进的（那头关了）没处可报。
            drop(err.write_all(b"\r\x1b[2K"));
        }
        // 写不进的（那头关了）没处可报。
        drop(err.write_all(text.as_bytes()).and_then(|()| err.flush()));
    }

    /// 标准错误上要印的一整段（标题、摘要）：接终端的洗掉控制序列，别的原样。
    fn shown(&self, text: &str) -> String {
        match self.terminal {
            true => Wash::once(text),
            false => text.to_string(),
        }
    }

    /// 上了色的一段：不上色的原样。
    fn paint(&self, color: &str, text: &str) -> String {
        match self.color {
            true => format!("{color}{text}{RESET}"),
            false => text.to_string(),
        }
    }

    /// 要印旁白了：回答那一行没完的先换行，思考没收尾的收尾，一块后面空一行。
    fn before_aside(&mut self, block: bool) {
        if self.open {
            out("\n");
            self.open = false;
        }
        match self.last {
            Last::Thinking => {
                self.err("\n\n");
                self.last = Last::Blank;
            }
            Last::Block => {
                self.err("\n");
                self.last = Last::Blank;
            }
            Last::Line if block => {
                self.err("\n");
                self.last = Last::Blank;
            }
            _ => {}
        }
    }

    /// 印一行旁白（灰）。
    fn aside(&mut self, line: &str) {
        self.before_aside(false);
        let line = self.paint(GRAY, line);
        self.err(&format!("{line}\n"));
        self.last = Last::Line;
    }

    /// 一步结束：标题一行，执行命令的下面印她看到的输出。
    fn step(&mut self, step: &Step, done: &Done) {
        let failed = done.status == ToolStatus::Error;
        let word = match done.status {
            ToolStatus::Error => Some(self.words.get("step.failed")),
            ToolStatus::Denied => Some(self.words.get("step.denied")),
            ToolStatus::Cancelled if done.summary.is_none() => {
                Some(self.words.get("step.cancelled"))
            }
            ToolStatus::Skipped if done.summary.is_none() => Some(self.words.get("step.skipped")),
            _ => None,
        };
        let summary = done.summary.as_deref().map(|text| self.shown(text));
        let said = match (word, &summary) {
            (Some(word), Some(summary))
                if matches!(done.status, ToolStatus::Error | ToolStatus::Denied) =>
            {
                format!(
                    "{}{}{summary}",
                    self.paint(RED, word),
                    self.words.get("colon")
                )
            }
            (Some(word), _) => match done.status {
                ToolStatus::Error | ToolStatus::Denied => self.paint(RED, word),
                _ => word.to_string(),
            },
            (None, Some(summary)) => summary.clone(),
            (None, None) => String::new(),
        };
        let command = step.block == Some(FaceBlock::Command);
        let shown = command && done.status == ToolStatus::Ok && !done.text.trim().is_empty();
        let head = match (command, &step.command) {
            (true, Some(text)) => {
                let dollar = if failed {
                    self.paint(RED, "$")
                } else {
                    "$".to_string()
                };
                let mut lines = text.lines();
                let mut head =
                    format!("{dollar} {}", crate::cli::clean(lines.next().unwrap_or("")));
                for more in lines {
                    head.push_str(&format!("\n> {}", crate::cli::clean(more)));
                }
                head
            }
            _ => format!("{} {}", step.icon, self.shown(&step.title)),
        };
        let line = match (said.is_empty(), shown) {
            (false, false) => format!("{head} · {said}"),
            _ => head,
        };
        let edits = step.block == Some(FaceBlock::Edits)
            && done.status == ToolStatus::Ok
            && !step.edits.is_empty();
        self.before_aside(shown || edits);
        self.err(&format!("{line}\n"));
        self.last = Last::Line;
        if edits {
            self.err(&self.edits(&step.edits));
            self.last = Last::Block;
        }
        if shown {
            let body: Vec<&str> = done.text.trim_matches('\n').lines().collect();
            let body = body.join("\n");
            self.err(&format!("{}\n", crate::cli::clean(&body)));
            self.last = Last::Block;
        }
    }

    /// 改动那一块（照 Miyu `cli/ask.md`「编辑那一块」）：每一处改掉的一行一个 `-`（红）、改成的一行一个 `+`（绿），两处之间
    /// 一行 `…`（灰）；控制字符换成 `�`，制表符照留。
    fn edits(&self, edits: &[(String, String)]) -> String {
        let mut text = String::new();
        for (at, (old, new)) in edits.iter().enumerate() {
            if at > 0 {
                text.push_str(&format!("{}\n", self.paint(GRAY, "…")));
            }
            for line in old.lines() {
                let line = format!("-{}", crate::cli::clean(line));
                text.push_str(&format!("{}\n", self.paint(RED, &line)));
            }
            for line in new.lines() {
                let line = format!("+{}", crate::cli::clean(line));
                text.push_str(&format!("{}\n", self.paint(GREEN, &line)));
            }
        }
        text
    }

    /// 用量那一行：`· 输入 400 · 命中缓存 160（40%）· 输出 40`。
    fn usage(&self, usage: Usage) -> String {
        let percent = match usage.input {
            0 => String::new(),
            input => {
                let rate = (usage.cache_read * 100 + input / 2) / input;
                self.words.say("usage.rate", &[("rate", &rate.to_string())])
            }
        };
        self.words.say(
            "usage.line",
            &[
                ("input", &grouped(usage.input)),
                ("hit", &grouped(usage.cache_read)),
                ("rate", &percent),
                ("output", &grouped(usage.output)),
            ],
        )
    }
}

impl Sink for Text<'_> {
    fn put(&mut self, out: Out<'_>) {
        match out {
            Out::Text(text) => {
                if !self.open && self.last != Last::Blank {
                    self.before_aside(false);
                    self.err("\n");
                    self.last = Last::Blank;
                }
                let text = match &mut self.answer {
                    Some(wash) => wash.push(text),
                    None => text.to_string(),
                };
                self::out(&text);
                self.open = !text.ends_with('\n');
            }
            Out::Thinking(text) => {
                if self.open {
                    self::out("\n");
                    self.open = false;
                }
                if self.last != Last::Thinking {
                    self.before_aside(true);
                }
                let text = match &mut self.thought {
                    Some(wash) => wash.push(text),
                    None => text.to_string(),
                };
                let text = self.paint(GRAY, &text);
                self.err(&text);
                self.last = Last::Thinking;
            }
            Out::StepDone(step, done) => self.step(step, done),
            Out::AgentReported(job, title, _) | Out::JobReported(job, title, _, _) => {
                let (job, title) = (&self.shown(job), &self.shown(title));
                let line = match title.is_empty() {
                    true => self.words.say("reported_untitled", &[("job", job)]),
                    false => self
                        .words
                        .say("reported", &[("job", job), ("title", title)]),
                };
                self.aside(&line);
            }
            Out::Compaction(before, after) => {
                let (before, after) = (tokens(before), tokens(after));
                let line = self
                    .words
                    .say("compacted", &[("before", &before), ("after", &after)]);
                self.aside(&line);
            }
            Out::Compacting(written) => {
                // 不是终端的压缩中不印（同 Miyu）；终端里回到行首擦掉这一行重画。
                if !self.terminal {
                    return;
                }
                if !self.progress.get() {
                    self.before_aside(false);
                }
                let line = match written {
                    0 => self.words.get("compacting").to_string(),
                    n => self.words.say("compacting.n", &[("n", &grouped(n))]),
                };
                let line = format!("\r\x1b[2K{}", self.paint(GRAY, &line));
                let mut err = std::io::stderr().lock();
                // 写不进的（那头关了）没处可报。
                drop(err.write_all(line.as_bytes()).and_then(|()| err.flush()));
                self.progress.set(true);
            }
            Out::CompactionFailed(why) => {
                let why = self.shown(&self.words.why(why));
                let line = self.words.say("compact.failed", &[("reason", &why)]);
                self.before_aside(false);
                self.err(&format!("{}\n", self.paint(RED, &line)));
                self.last = Last::Line;
            }
            Out::TurnStarted(_) | Out::Step(_) | Out::TurnEnded(_) => {}
        }
    }
}

impl Output for Text<'_> {
    fn session(&mut self, _id: &str, _new: bool) {}

    fn waiting(&mut self, agents: usize, commands: usize) {
        if self.waited {
            return;
        }
        self.waited = true;
        let line = super::out::waiting_line(self.words, agents, commands);
        self.aside(&line);
    }

    fn finish(&mut self, ending: &Ending, follow: &Follow, _session: Option<&str>) {
        if self.open {
            out("\n");
            self.open = false;
        }
        if self.last == Last::Thinking {
            self.err("\n");
        }
        if let Some(total) = follow.total() {
            let line = self.paint(GRAY, &self.usage(total));
            self.err(&format!("{line}\n"));
        }
        if !follow.denied.is_empty() {
            let count = follow.denied.len().to_string();
            let key = if follow.denied.len() == 1 {
                "denied.one"
            } else {
                "denied.many"
            };
            self.words.warn(
                &self.words.say(key, &[("count", &count)]),
                Some(self.words.get("denied.hint")),
            );
        }
        if let Some(said) = &ending.said {
            self.words.error(said, None);
        }
    }
}

/// 标准输出上写一段（那头关了的不崩，`main` 退出时照退出码走）。
fn out(text: &str) {
    let mut out = std::io::stdout().lock();
    // 写不进的（那头关了）没处可报。
    drop(out.write_all(text.as_bytes()).and_then(|()| out.flush()));
}

/// 三位一撇：`1830` 写成 `1,830`。
fn grouped(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (k, c) in digits.chars().enumerate() {
        if k > 0 && (digits.len() - k).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// token 数：不到一千照写，一千以上写 `k`、一百万以上写 `M`，一位小数，整的不写小数（Miyu 压缩那一行）。
fn tokens(n: u64) -> String {
    let (unit, div) = match n {
        0..1_000 => return n.to_string(),
        1_000..1_000_000 => ("k", 1_000),
        _ => ("M", 1_000_000),
    };
    let tenths = (n * 10 + div / 2) / div;
    match tenths % 10 {
        0 => format!("{}{unit}", tenths / 10),
        rest => format!("{}.{rest}{unit}", tenths / 10),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn numbers_are_grouped_and_tokens_shortened() {
        assert_eq!(super::grouped(1830), "1,830");
        assert_eq!(super::grouped(999), "999");
        assert_eq!(super::grouped(1_000_000), "1,000,000");
        assert_eq!(super::tokens(812_300), "812.3k");
        assert_eq!(super::tokens(31_000), "31k");
        assert_eq!(super::tokens(999), "999");
        assert_eq!(super::tokens(2_500_000), "2.5M");
    }
}
