//! 替身的辅助请求（施工 3-8 五补的起标题，`docs/blueprint/kernel/session.md`「起标题」；四补的回顾 LM-8 删了）：起标题、提前
//! 压好的摘要的请求怎么回、停住的放行。辅助请求和主请求分开记、分开排剧本：它们不占主请求的剧本，也不算在
//! [`Stage::requests`] 里。
//!
//! 起标题没排剧本的只记下、不回（一直在路上）：不管标题的测试不用替它排。

use super::Stage;
use super::respond::{deltas, model};
use super::script::Line;
use crate::kernel::accumulate::{Delta, Kind};
use crate::kernel::event::{Purpose, Usage};
use crate::kernel::id::{CommandId, Seq};
use crate::kernel::request::Request;
use crate::kernel::session::{Command, Input};

impl Stage {
    /// alice 改标题、置顶，改哪样写哪样（施工 3-8 五补测起标题用）。返回这个命令的编号。
    pub fn set_meta(&mut self, title: Option<&str>, pinned: Option<bool>) -> CommandId {
        self.command(Command::SetMeta {
            title: title.map(str::to_string),
            pinned,
        })
    }

    /// alice 要求重起标题（施工 TT-1）。返回这个命令的编号。
    pub fn retitle(&mut self) -> CommandId {
        self.command(Command::Retitle)
    }

    /// 起标题的请求接下来几次，照先后这样回（施工 3-8 五补）。没排的只记下、不回。
    pub fn title_model(&mut self, lines: impl IntoIterator<Item = Line>) {
        self.title_lines.extend(lines);
    }

    /// 起标题的请求，照先后：照到第几条，和请求本身。
    pub fn titles(&self) -> &[(Seq, Request)] {
        &self.titles
    }

    /// 放行停住的那次起标题：送说完了。
    ///
    /// # Panics
    ///
    /// 没有停住的起标题。
    pub fn release_title(&mut self) {
        let (upto, line) = self
            .held_title
            .take()
            .unwrap_or_else(|| panic!("没有停住的起标题"));
        let ended = self.aside_ended(Purpose::Title, upto, &line);
        self.run(ended);
    }

    /// 辅助请求：记下，照各自的剧本回。
    pub(super) fn aside_call(
        &mut self,
        purpose: Purpose,
        upto: Seq,
        request: Request,
    ) -> Vec<Input> {
        let hash = request.hash();
        let line = match purpose {
            Purpose::Title => {
                self.titles.push((upto, request));
                self.title_lines.pop_front()
            }
            Purpose::Compaction => {
                self.prepare.asked.push((upto, request));
                self.prepare.lines.pop_front()
            }
            Purpose::Recap | Purpose::Other(_) => None,
        };
        let Some(line) = line else {
            return Vec::new();
        };
        let mut inputs = vec![Input::AsideSent {
            at: self.tick(),
            purpose: purpose.clone(),
            upto,
            model: model(),
            request: hash,
            fell_back: false,
        }];
        if let (true, Some(chars)) = (line.hold, line.partway) {
            for delta in partway(&line, chars, true) {
                inputs.push(Input::AsideDelta {
                    at: self.tick(),
                    purpose: purpose.clone(),
                    upto,
                    delta,
                });
            }
        } else if line.error.is_none() || line.says_something() {
            for delta in deltas(&line) {
                inputs.push(Input::AsideDelta {
                    at: self.tick(),
                    purpose: purpose.clone(),
                    upto,
                    delta,
                });
            }
        }
        if line.hold {
            match purpose {
                Purpose::Title => self.held_title = Some((upto, line)),
                _ => self.prepare.held = Some((upto, line)),
            }
        } else {
            inputs.push(self.aside_ended(purpose, upto, &line));
        }
        inputs
    }

    /// 辅助请求 `upto` 说完了：出错的带上分类和原话，说完了的带上用量。
    pub(super) fn aside_ended(&mut self, purpose: Purpose, upto: Seq, line: &Line) -> Input {
        Input::AsideEnded {
            sent: None,
            at: self.tick(),
            purpose,
            upto,
            cost: None,
            usage: line.error.is_none().then(|| {
                line.usage.unwrap_or(Usage {
                    uncached: 100,
                    cache_read: 0,
                    cache_write: 0,
                    output: 10,
                    reasoning: None,
                })
            }),
            error: line.error.clone(),
        }
    }
}

/// 只说话的一行正文分成两截（施工 6-11 下，[`Line::held_after`]）：`head` 是停住以前的那截（开始、头 `chars` 个字），不是的
/// 是放行时的那截（剩下的字、收全）。
pub(super) fn partway(line: &Line, chars: usize, head: bool) -> Vec<Delta> {
    let cut = line
        .text
        .char_indices()
        .nth(chars)
        .map_or(line.text.len(), |(at, _)| at);
    let (first, rest) = line.text.split_at(cut);
    match head {
        true => vec![
            Delta::Start {
                index: 0,
                kind: Kind::Text,
            },
            Delta::Text {
                index: 0,
                text: first.to_string(),
            },
        ],
        false => {
            let mut deltas = Vec::new();
            if !rest.is_empty() {
                deltas.push(Delta::Text {
                    index: 0,
                    text: rest.to_string(),
                });
            }
            deltas.push(Delta::End { index: 0 });
            deltas
        }
    }
}
