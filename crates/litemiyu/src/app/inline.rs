//! shell 集成进这份状态的几个口子（`cli/shellhook/inline.rs`，施工线 shellhook）：在 shell 里说话时，后台的进程里跑的是
//! 这同一个 [`App`]，只是不画整屏：引擎的消息照旧交给 [`App::core`]，这里另开「说一句」「开好了没有」「了结了没有」
//! 「这一句以后的正文排成行」几样。全屏界面不走这里。

use std::cell::Cell;

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Block;
use unicode_width::UnicodeWidthStr;

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

/// 在 shell 里要另外办的斜杠命令。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slash {
    /// 只在全屏界面里有意义（`/exit`、`/quit`、`/icons`）：说明一句。
    FullscreenOnly,
    /// 要进全屏界面办（`/config`、`/tui`、`/files`、`/jump`）。
    Fullscreen,
    /// `/loop`：拿着会话的进程要留下来才到得了点。
    Loop,
    /// `/new`：照全屏办，另留一行结果。
    New,
}

/// 斜杠命令开的面板在 shell 里怎么办。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shown {
    /// 整个印出来就完（`/help`、`/status`）：没有要选的。
    Printed,
    /// 画在提示符下面等人选（`/model`、`/effort`、`/sessions`、`/preset`）：选了、关了才完。
    Picked,
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

    /// 这一句是 `/edit` 放回命令行、改过的上一句（脚本带 `--redo` 交来的，坑 C4）：替换上一轮重来，照全屏「编辑上一句」
    /// 回车的那一套办。上一轮重不了的（没有会话、最后一轮不是你说的话开的）照普通的一句发。
    pub fn redo_inline(&mut self, text: &str) {
        self.edit_last();
        if !self.typing.input.editing() {
            return self.say_inline(text);
        }
        self.typing.input.editor.set_draft(Draft::plain(text));
        let draft = self.typing.input.editor.take_draft();
        self.submit(draft);
    }

    /// 输入框正在「编辑上一句」（`/edit` 刚把上一句放进去）：交还给 shell 的命令行时要告诉脚本，下一次回车是替换上一轮
    /// 重来。
    pub fn editing_inline(&self) -> bool {
        self.typing.input.editing()
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

    /// 了结了：没有一轮在跑、没有在等开轮的、没有还在转的步，派出去的后台命令、子代理都报了；没有开着的抽屉、没有排着
    /// 还没被听到的话、没有在撤掉「还没开口就打断」的那一轮。
    ///
    /// `jobs` 是等不等后台任务：拿着会话的进程能留下来的不等（它们报的时候另外通知），留不下来的等它们都报了。
    pub fn settled_inline(&self, jobs: bool) -> bool {
        let transcript = &self.talk.transcript;
        !self.answering_inline()
            && !transcript.busy()
            && (!jobs || self.talk.jobs.pending() == 0)
            && !self.talk.drawers.open()
            && self.talk.takeback.is_none()
            && !transcript.entries.iter().any(|entry| entry.queued)
    }

    /// 还有要等的：没结束的后台命令、子代理，开着的循环、挂着的到点继续。有的话拿着会话的进程留下来。
    pub fn lingering_inline(&self) -> bool {
        self.talk.jobs.pending() > 0 || self.talk.looping.is_some()
    }

    /// 在回答：一轮在跑，或者话发出去了还在等开轮。
    pub fn answering_inline(&self) -> bool {
        let transcript = &self.talk.transcript;
        transcript.running.is_some() || transcript.starting != Starting::No
    }

    /// `Ctrl+C`：照全屏打断这一轮（排着的话退回输入框；AI 还没开口的，这一轮撤掉、那一句放回输入框）。
    pub fn interrupt_inline(&mut self) {
        self.interrupt();
    }

    /// 回车插话：输入框里的话照全屏回车发出去的那一套办（正在回答，这一句先排着，AI 下一次请求时听到）。交回发了没有。
    /// 不发的：空的；斜杠、`!` 打头的（命令答完回到 shell 的命令行再办）；没在回答的（字留着，答完回到命令行）。
    pub fn interject_inline(&mut self) -> bool {
        let text = self.typing.input.editor.text().trim_start();
        let command = text.starts_with('/') || self.typing.input.shell();
        if text.is_empty() || command || !self.answering_inline() {
            return false;
        }
        let draft = self.typing.input.editor.take_draft();
        self.submit(draft);
        true
    }

    /// 没有抽屉的时候按的一个键：照输入框自己的办（挪光标、删字、`Alt+回车` 换行）。它要整个界面办的那几样（退出、挂起、
    /// 开编辑器）在 shell 里不办。
    pub fn type_inline(&mut self, key: crossterm::event::KeyEvent) {
        let _ignored = self.typing.input.key(key);
    }

    /// 抽屉开着的时候按的一个键：归抽屉（同全屏）。
    pub fn drawer_key_inline(&mut self, key: crossterm::event::KeyEvent) {
        self.drawer_key(key);
    }

    /// 粘贴来的一段字：照全屏的粘贴办（长的收成块；抽屉在写字的归它）。
    pub fn paste_inline(&mut self, text: &str) {
        self.paste(text);
    }

    /// 输入框里没发出去的字（粘贴块换回原文）：答完交还给 shell 的命令行。
    pub fn typed_inline(&self) -> String {
        self.typing.input.draft().expand()
    }

    /// 这个会话现在用的模型（引用的写法）；还没有会话、没说过话的是 `None`。
    pub fn model_inline(&self) -> Option<String> {
        self.talk.transcript.current_ref()
    }

    /// 这个会话的标题；没起名的是 `None`。
    pub fn title_inline(&self) -> Option<String> {
        self.talk.transcript.title.clone()
    }

    /// 开着的面板在 shell 里怎么办：没有面板的是 `None`。
    pub fn panel_inline(&self) -> Option<Shown> {
        match self.boxes.panel.as_ref()? {
            super::Panel::Help { .. } | super::Panel::Status { .. } => Some(Shown::Printed),
            _ => Some(Shown::Picked),
        }
    }

    /// 面板开着的时候按的一个键：归面板（同全屏）。
    pub fn panel_key_inline(&mut self, key: crossterm::event::KeyEvent) {
        let _ignored = self.key(key);
    }

    /// 关上面板。
    pub fn close_panel_inline(&mut self) {
        self.boxes.panel = None;
    }

    /// 开着的面板画成一块（`ui::panel_block`）；等人选的下边框写按键提示，整个印出来就完的不写。
    pub fn panel_block_inline(&self, width: u16, most: u16) -> Option<Buffer> {
        let keys = self.panel_inline() == Some(Shown::Picked);
        crate::ui::panel_block(&self.seen(), (width, most), keys)
    }

    /// 这一行在 shell 里要另外办的斜杠命令是哪一种（[`Slash`]）；不是命令的、照全屏办就行的命令是 `None`。
    pub fn slash_inline(&self, text: &str) -> Option<Slash> {
        use crate::commands::{Line as Typed, Run};
        // `/tui` 只有 shell 里有：用全屏界面打开这个终端的会话。
        if text.trim() == "/tui" {
            return Some(Slash::Fullscreen);
        }
        let Typed::Command(spec, _) = self.config.commands.read(text) else {
            return None;
        };
        match spec.run {
            Run::Quit | Run::Icons => Some(Slash::FullscreenOnly),
            Run::Config | Run::Files | Run::Jump => Some(Slash::Fullscreen),
            Run::Loop => Some(Slash::Loop),
            Run::New => Some(Slash::New),
            _ => None,
        }
    }

    /// 输入框清空（没发出去的字已经交还给 shell 的命令行；这个进程留下来的话，下一句来的时候输入框是空的）。
    pub fn clear_typed_inline(&mut self) {
        drop(self.typing.input.editor.take_draft());
        drop(self.typing.input.take_edit());
    }

    /// 有抽屉开着。
    pub fn asking_inline(&self) -> bool {
        self.talk.drawers.open()
    }

    /// 活动区最下面的输入那几行，`width` 是终端的宽：排着还没被听到的话各一行暗色的，下面是输入框里的字（照全屏输入框
    /// 排，前面是提示符）。另交回光标在第几行、第几列（坑 P9：没打字的时候也有这一行、光标停在这里，输入法的候选框一开始
    /// 就在对的地方）。`after` 同 [`App::lay_inline`]。
    pub fn input_inline(&self, after: u64, width: u16) -> (Vec<Line<'static>>, (usize, u16)) {
        let config = &self.config;
        let room = usize::from(width).saturating_sub(2);
        let mut lines: Vec<Line<'static>> = self
            .talk
            .transcript
            .entries
            .iter()
            .filter(|entry| entry.queued && entry.id > after)
            .map(|entry| {
                let said = entry.text.replace(['\r', '\n'], " ");
                let text = format!("{}{said}", config.layout.queued_mark);
                Line::styled(
                    format!("  {}", crate::cells::clip(&text, room)),
                    crate::theme::dim(),
                )
            })
            .collect();
        let prompt = config.layout.prompt.as_str();
        // 两格槽、提示符、一个空格。
        let lead = 2 + prompt.width() + 1;
        let input = &self.typing.input;
        let editor = &input.editor;
        if editor.is_empty() {
            lines.push(Line::default());
            let at = (lines.len() - 1, u16::try_from(lead).unwrap_or(0));
            return (lines, at);
        }
        let columns = u16::try_from(room.saturating_sub(lead))
            .unwrap_or(u16::MAX)
            .max(1);
        let wrapped = input.lines(columns);
        let (row, column) = crate::input::locate(editor.text(), &wrapped, editor.cursor());
        // 最多露这么多行，露着光标那一行。
        let most = usize::from(config.layout.max_rows).max(1);
        let top = (row + 1).saturating_sub(most);
        let blocks = editor.blocks();
        let first = lines.len();
        for (index, visual) in wrapped.iter().enumerate().skip(top).take(most) {
            let styled =
                crate::ui::styled_line(editor.text(), *visual, editor.selection(), &blocks, None);
            let mut spans = vec![Span::raw("  ")];
            spans.push(match index {
                0 => Span::styled(format!("{prompt} "), crate::theme::level()),
                _ => Span::raw(" ".repeat(prompt.width() + 1)),
            });
            spans.extend(
                styled
                    .spans
                    .into_iter()
                    .map(|span| Span::styled(span.content.into_owned(), span.style)),
            );
            lines.push(Line::from(spans));
        }
        let at = (
            first + row - top,
            u16::try_from(lead + column).unwrap_or(u16::MAX),
        );
        (lines, at)
    }

    /// 开着的抽屉画成一块（`width` 宽，最高 `most` 行，连边框）：一个暗色的圆角框，里面照全屏的画法画（`ui/drawer`）。
    /// 另交回里面写字的那一格在这一块的第几行、第几列（坑 P10：输入法的候选框跟着它）；没在写字的没有。没有抽屉的是
    /// `None`。
    pub fn drawer_inline(
        &mut self,
        width: u16,
        most: u16,
    ) -> Option<(Buffer, Option<(usize, u16)>)> {
        let texts = &self.config.text().drawer;
        let look = &self.config.layout.drawer_preview;
        let vim = self.config.layout.vim;
        let drawer = self.talk.drawers.current.as_mut()?;
        // 左右各一条边线、一格空。
        let inner = width.saturating_sub(4).max(1);
        let rows =
            crate::ui::drawer_rows(drawer, texts, look, inner, most.saturating_sub(2).max(1))
                .max(1);
        let frame = Rect::new(0, 0, width, rows + 2);
        let mut buffer = Buffer::empty(frame);
        let mut caret = crate::caret::Caret::default();
        {
            let mut canvas = crate::ui::Frame::new(&mut buffer);
            canvas.render_widget(
                Block::bordered()
                    .border_set(crate::sheet::read(|sheet| sheet.frame.input.set()))
                    .border_style(crate::theme::dim()),
                frame,
            );
            crate::ui::draw_drawer(
                &mut canvas,
                (Rect::new(2, 1, inner, rows), 0),
                drawer,
                texts,
                look,
                vim,
                &mut caret,
            );
        }
        let at = caret.shown.then(|| (usize::from(caret.at.y), caret.at.x));
        Some((buffer, at))
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
