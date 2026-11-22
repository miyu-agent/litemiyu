//! shell 集成进这份状态的几个口子（`cli/shellhook/inline.rs`，施工线 shellhook）：在 shell 里说话时，后台的进程里跑的是
//! 这同一个 [`App`]，只是不画整屏：引擎的消息照旧交给 [`App::core`]，这里另开「说一句」「开好了没有」「了结了没有」
//! 「这一句以后的正文排成行」几样。全屏界面不走这里。

use std::cell::Cell;

use ratatui::text::Line;

use super::App;
use super::sessions::Opening;
use crate::input::Draft;
use crate::transcript::{Entry, Kind, Starting};
use crate::ui::rows::{Ctx, entry_rows, shown};

/// 排好的几行，和前几行已经定下来。
pub struct Laid {
    /// 行，从上到下。
    pub lines: Vec<Line<'static>>,
    /// 前几行不会再变：可以印进终端的滚动历史。后面的还会变（进行中的时间线、还在写的那一段的末尾、末尾转着的），
    /// 留在活动区里（坑 P3、P17）。
    pub settled: usize,
}

/// 排的时候记着的：还在写的那一段回答，前几行已经定下来（[`settled_rows`]）。只在「写完了的那几块到哪」变了的时候重算。
#[derive(Default)]
pub struct Laying {
    /// 哪一条、原文里写完了的那几块到第几个字节、照多宽排的，前几行定下来了。
    cut: Cell<Option<(u64, usize, u16, usize)>>,
}

impl App {
    /// 说一句：照输入框里回车发出去的那一套办（斜杠打头的照命令办）。
    pub fn say_inline(&mut self, text: &str) {
        self.submit(Draft::plain(text));
    }

    /// 要开的旧会话还没开好、还在收补发。
    pub fn opening_inline(&self) -> bool {
        self.talk.opening != Opening::None
    }

    /// 正文里最新的一条的编号（没有的是 0）：说话以前记下它，之后编号比它大的就是这一句以后的。
    pub fn last_entry_inline(&self) -> u64 {
        self.talk
            .transcript
            .entries
            .last()
            .map_or(0, |entry| entry.id)
    }

    /// 了结了：没有一轮在跑、没有在等开轮的、没有还在转的步，派出去的后台命令、子代理都报了。
    pub fn settled_inline(&self) -> bool {
        let transcript = &self.talk.transcript;
        transcript.running.is_none()
            && transcript.starting == Starting::No
            && !transcript.busy()
            && self.talk.jobs.pending() == 0
    }

    /// 编号在 `after` 以后的正文排成行，照全屏界面的排法（`ui/rows.rs` 的 `entry_rows`），内容宽 `width`（调的一方照终端
    /// 宽减两格槽给，坑 P5）。`said` 是开这一轮的那句话的编号：shell 已经在提示符那一行留着它了，不再排。`frame` 是转圈转到
    /// 第几帧。最前面空一行，和提示符那一行隔开（坑 P6）。
    pub fn lay_inline(
        &self,
        laying: &Laying,
        (after, said): (u64, Option<u64>),
        width: u16,
        frame: usize,
    ) -> Laid {
        let transcript = &self.talk.transcript;
        let ctx = Ctx {
            config: &self.config,
            human: &self.link.human,
            indent: String::new(),
            width,
            hover: None,
            frame,
            md: &self.media.md_cache,
            sessions: self.talk.recent.listed.as_deref().unwrap_or_default(),
            jobs: Some(&self.talk.jobs),
        };
        // 还在写的那一段回答（没有的是 `None`）。
        let writing = transcript
            .reading()
            .filter(|(_, writing)| *writing)
            .map(|(at, _)| at);
        let mut lines: Vec<Line<'static>> = Vec::new();
        let mut settled = None;
        let ours = transcript
            .entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.id > after && Some(entry.id) != said)
            // 排着队、还没被听到的话画在活动区的最下面（做了答话时的输入行以后），这里不排。
            .filter(|(_, entry)| shown(entry) && !(entry.kind == Kind::User && entry.queued));
        for (at, entry) in ours {
            if !lines.is_empty() {
                lines.push(Line::default());
            }
            let rows = entry_rows(at, entry, &ctx);
            let unsettled = entry
                .segment
                .as_ref()
                .is_some_and(|segment| !segment.settled());
            if settled.is_none() && unsettled {
                settled = Some(lines.len());
            }
            let first = lines.len();
            lines.extend(rows.into_iter().map(|row| row.line));
            if settled.is_none() && writing == Some(at) {
                let done = settled_rows(laying, (at, entry), &ctx, &lines[first..]);
                settled = Some(first + done);
            }
        }
        let mut settled = settled.unwrap_or(lines.len());
        // 等第一个字：末尾先转着（同全屏）。
        if transcript.waiting() {
            lines.extend(crate::ui::tail_rows(&ctx).into_iter().map(|row| row.line));
        }
        // 开头的空行（只有两格槽的也算）去掉，再空一行。
        while lines.first().is_some_and(blank) {
            lines.remove(0);
            settled = settled.saturating_sub(1);
        }
        if !lines.is_empty() {
            lines.insert(0, Line::default());
            settled += 1;
        }
        Laid { lines, settled }
    }
}

/// 这一行只有空白。
fn blank(line: &Line<'_>) -> bool {
    line.spans.iter().all(|span| span.content.trim().is_empty())
}

/// 还在写的那一段回答（`entry`，排成了 `rows`），前几行算定下来了：原文里已经写完的那几块（[`written`]）单独排出来有几行，
/// 就是前几行。正在写的那一块前面几行也会跟着后来的字变——表头那一行要等下一行到了才认得出是表格，表格的列宽跟着后面的
/// 行变，代码块没写完时最后一行下面已经画着下边框——印进滚动历史就改不了了（坑 P17；2026-11-23 在真终端里看到表头多印了
/// 一行、代码行被截断）。
///
/// 照原文认「写完了」，不照排好的行里有没有空行认：代码写到一半时只到了缩进的那一行，排出来也是一个空行。单独排出来的
/// 和整段排出来的前几行对不上的（前面的块也跟着后面的字变了），一行都不算。
fn settled_rows(
    laying: &Laying,
    (at, entry): (usize, &Entry),
    ctx: &Ctx,
    rows: &[Line<'static>],
) -> usize {
    let upto = written(&entry.text);
    if upto == 0 {
        return 0;
    }
    if let Some((id, cut, width, count)) = laying.cut.get()
        && (id, cut, width) == (entry.id, upto, ctx.width)
    {
        return count.min(rows.len());
    }
    let mut head = entry.clone();
    head.text.truncate(upto);
    // 换一个编号：照编号记着的排好的行不和整段的那一份混。
    head.id = u64::MAX;
    let alone: Vec<Line<'static>> = entry_rows(at, &head, ctx)
        .into_iter()
        .map(|row| row.line)
        .collect();
    let count = match rows.get(..alone.len()) {
        Some(same) if same == alone.as_slice() => alone.len(),
        _ => 0,
    };
    laying.cut.set(Some((entry.id, upto, ctx.width, count)));
    count
}

/// Markdown 原文里已经写完的那几块到第几个字节：最后一个完整的空行为止。围栏代码（三个反引号、三个波浪号起头的）里面的
/// 空行不算：代码还没写完，它的下边框要等围栏关上才定。一块都没写完的是 0。
fn written(text: &str) -> usize {
    let mut done = 0;
    // 开着的围栏用的什么记号：围栏里面的空行不算。
    let mut fence: Option<char> = None;
    let mut at = 0;
    for line in text.split_inclusive('\n') {
        at += line.len();
        // 最后一行还没写完（没有换行）：不看。
        if !line.ends_with('\n') {
            break;
        }
        let trimmed = line.trim_start();
        let marker = ['`', '~']
            .into_iter()
            .find(|mark| trimmed.starts_with(&mark.to_string().repeat(3)));
        match (fence, marker) {
            (None, Some(mark)) => fence = Some(mark),
            (Some(open), Some(mark)) if open == mark => fence = None,
            (None, None) if trimmed.trim().is_empty() => done = at,
            _ => {}
        }
    }
    done
}

#[cfg(test)]
mod tests {
    use super::written;

    #[test]
    fn only_finished_blocks_of_a_reply_being_written_count_as_written() {
        // 一个空行都没有：一块都没写完。
        assert_eq!(written("第一段的头一行\n第二行"), 0);
        // 表头那一行要等下一行到了才认得出是表格：空行以后的都不算。
        let text = "一段话。\n\n| 名称 | 用途 |\n|-";
        assert_eq!(&text[..written(text)], "一段话。\n\n");
        // 最后的空行还没换行（只到了几个空格）：不算空行。
        assert_eq!(written("一段话。\n   "), 0);
        // 代码写到一半、只到了缩进的那一行：整段代码都不算，到它的开头为止（2026-11-23 真终端里那一行印成了空行）。
        let text = "说明。\n\n```rust\nfn main() {\n\n    let a = 1;\n        ";
        assert_eq!(&text[..written(text)], "说明。\n\n");
        // 代码写完了、后面空了一行：连代码一起算。
        let text = "说明。\n\n```rust\nfn main() {}\n```\n\n接着";
        assert_eq!(
            &text[..written(text)],
            "说明。\n\n```rust\nfn main() {}\n```\n\n"
        );
        // 围栏紧跟在一段话后面、中间没有空行：那段话还不算写完（没有空行隔开）。
        assert_eq!(written("说明。\n~~~\ncode\n"), 0);
    }
}
