//! 补发来的事件怎么读（Miyu 终端 `core/replay.rs`，蓝图 `tui.md`「会话列表 `/sessions`」第 6 条）：打开旧会话时引擎从日志
//! 补发以前落了盘的事件，流式的增量不补。这里把它们读成和看着流出来时一样的推送：回答、思考、调工具照 `message.assistant`
//! 的块开一块、写字、收一块；人说的话照你说的画（看着发的那句先画上了，补发的没有）；每条前面垫一条 [`Push::Clock`]，正文
//! 照事件的时刻写时刻、算用时，补完了垫一条 `None`。
//!
//! 一块用了多久在紧跟着的那一条 `model.called` 里（`blocks` 的 `start_ms`、`end_ms`，从这次请求发出去算起），所以
//! `message.assistant` 先记着，等到它的 `model.called`（`seen` 一样）再一起读，每块照自己的起止垫钟：补发来的「已思考」、
//! 调工具的用时和看着流出来时一样（Miyu 终端同一条）。没等到的（日志里没有、或者中间隔了别的）照回答落盘的那一刻。读的是
//! 内核的事件本身，不是 JSON。

use lm_kernel::kernel::block::Block as Content;
use lm_kernel::kernel::event::{Body, Event, ModelCalled};
use lm_kernel::kernel::origin::By;

use super::{Block, Push};

/// 一次补发：记着还没等到 `model.called` 的那一条回答。
#[derive(Debug, Default)]
pub struct Replay {
    pending: Option<Event>,
    /// 上一条垫的钟（毫秒）：一样的不再垫，一万轮的会话打开时少十几万条推送。
    clock: Option<i64>,
}

impl Replay {
    /// 读一条补发来的事件。
    pub fn read(&mut self, event: Event) -> Vec<Push> {
        let mut out = Vec::new();
        if let Some(said) = self.pending.take() {
            let called = match (&said.body, &event.body) {
                (Body::MessageAssistant(reply), Body::ModelCalled(called))
                    if called.seen == reply.seen =>
                {
                    Some((&**called, event.at.unix_millis()))
                }
                _ => None,
            };
            self.answer(&said, called, &mut out);
            if called.is_some() {
                self.one(&event, &mut out);
                return out;
            }
        }
        if matches!(event.body, Body::MessageAssistant(_)) {
            self.pending = Some(event);
            return out;
        }
        self.one(&event, &mut out);
        out
    }

    /// 补完了：还记着的回答照它自己的时刻读，钟回到现在。
    pub fn finish(&mut self) -> Vec<Push> {
        let mut out = Vec::new();
        if let Some(said) = self.pending.take() {
            self.answer(&said, None, &mut out);
        }
        out.push(Push::Clock(None));
        out
    }

    /// 读一条不是回答的事件：垫上它的时刻，照平常读。
    fn one(&mut self, event: &Event, out: &mut Vec<Push>) {
        self.tick(out, event.at.unix_millis());
        one(event, out);
    }

    /// 一条回答读成一块块：开一块、写字、收一块，照 `called`（带着那一条 `model.called` 落盘的毫秒）里每块的起止垫钟；
    /// 没有的照回答落盘的时刻。
    fn answer(&mut self, said: &Event, called: Option<(&ModelCalled, i64)>, out: &mut Vec<Push>) {
        let end = said.at.unix_millis();
        // 这次请求发出去的那一刻：`model.called` 落盘的时刻往回倒它用的毫秒数。
        let sent = called.and_then(|(c, at)| {
            let took = i64::try_from(c.duration_ms?).ok()?;
            Some((c, at.checked_sub(took)?))
        });
        let span = |i: usize| -> (i64, i64) {
            let Some((c, sent)) = sent else {
                return (end, end);
            };
            let at = |ms: u64| i64::try_from(ms).ok().and_then(|ms| sent.checked_add(ms));
            let block = c.blocks.as_ref().and_then(|b| b.get(i));
            let from = block.and_then(|b| at(b.start_ms)).unwrap_or(end);
            let to = block.and_then(|b| at(b.end_ms)).unwrap_or(end);
            (from, to)
        };
        if let Body::MessageAssistant(message) = &said.body {
            for (i, block) in message.blocks.iter().enumerate() {
                let (kind, text) = match block {
                    Content::Text(text) => (Block::Text, text.text.clone()),
                    Content::Reasoning(reasoning) => (Block::Reasoning, reasoning.text.clone()),
                    Content::ToolCall(call) => {
                        (Block::ToolCall(call.name.clone()), call.args.clone())
                    }
                    _ => continue,
                };
                let index = i as u64;
                let (from, to) = span(i);
                self.tick(out, from);
                out.push(Push::BlockStart { index, block: kind });
                out.push(Push::Delta { index, text });
                self.tick(out, to);
                out.push(Push::BlockEnd(index));
            }
        }
        self.tick(out, end);
        out.extend(super::read(said));
    }

    /// 垫一条钟：和上一条一样的不垫，毫秒数读不成时刻的不垫（`Clock(None)` 是补完了）。
    fn tick(&mut self, out: &mut Vec<Push>, millis: i64) {
        if self.clock == Some(millis) {
            return;
        }
        if let Ok(at) = jiff::Timestamp::from_millisecond(millis) {
            self.clock = Some(millis);
            out.push(Push::Clock(Some(at)));
        }
    }
}

/// 读一条不是回答的事件（钟已经垫好了）：人说的话照你说的画，别的照平常读。
fn one(event: &Event, out: &mut Vec<Push>) {
    // 人说的：看着发的先画上了、落盘时只配序号；补发来的没有先画上的，照你说的画（Miyu 同一条）。
    if let Body::MessageUser(message) = &event.body
        && matches!(event.by, By::Person(_))
    {
        super::resent(event, out);
        let seq = event.seq.get();
        let text = message
            .typed()
            .map_or_else(|| message.text(), str::to_string);
        out.push(Push::Said { seq, text });
        if message.shell {
            out.push(Push::ShellAsked(seq));
        }
        return;
    }
    out.extend(super::read(event));
}

#[cfg(test)]
mod tests;
