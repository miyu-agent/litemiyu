//! 日志里你说的话（`/jump` 的列表，红线 5、界面线「做法」第 7 条）：正文只留几屏以后，放掉的段里你说的话
//! 界面这头没有了；`/jump` 开的时候引擎在读完就退的线程里把日志补发一遍，交给这里办进一份草稿正文，和打开旧会话时办出来
//! 的一样（排着的几句拼成一段、退回的不算、跑命令的另记），一段办完（开了下一段）就收走那一段你说的话、放掉它的条目，
//! 草稿里只留在办的那一段。

use super::{Kind, Transcript};
use crate::config::Texts;
use crate::core::{Push, Update};

/// 你说的一句：落盘的序号（认它在哪一段）、`/jump` 列的样子、说的那一刻。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Said {
    /// 序号。
    pub seq: u64,
    /// 列的样子（[`super::Entry::listed`]）。
    pub text: String,
    /// 说的那一刻（[`super::Entry::at`]）。
    pub at: Option<jiff::Timestamp>,
}

/// 收补发来的推送、攒你说的话。
#[derive(Debug)]
pub struct Gather {
    draft: Transcript,
    texts: Box<Texts>,
    /// 第几段以前的都收过了。
    done: usize,
    said: Vec<Said>,
}

impl Gather {
    /// 一份空的，草稿里的旁白照 `texts` 写（同界面）。
    pub fn new(texts: Box<Texts>) -> Gather {
        Gather {
            draft: Transcript::untracked(),
            texts,
            done: 0,
            said: Vec::new(),
        }
    }

    /// 办一条推送：开了新的一段，前面办完的收走你说的话、放掉条目。
    pub fn push(&mut self, push: Push) {
        let before = self.draft.pieces().len();
        self.draft.update(Update::Push(push), &self.texts);
        let now = self.draft.pieces().len();
        if now > before {
            self.collect(now - 1);
        }
    }

    /// 办完了：交回全部你说的话，照先后。
    pub fn finish(mut self) -> Vec<Said> {
        let all = self.draft.pieces().len();
        self.collect(all);
        self.said
    }

    /// 收第 `done` 段到第 `upto` 段以前（不含）你说的话，放得掉的放掉。
    fn collect(&mut self, upto: usize) {
        for p in self.done..upto {
            let span = self.draft.span(p);
            self.said.extend(
                self.draft.entries[span]
                    .iter()
                    .filter(|e| e.kind == Kind::User && !e.hidden)
                    .filter_map(|e| {
                        Some(Said {
                            seq: e.seq?,
                            text: e.listed(),
                            at: e.at,
                        })
                    }),
            );
            self.draft.shed(p, 0, 0, 0);
        }
        self.done = self.done.max(upto);
    }
}

#[cfg(test)]
mod tests;
