//! 「文件修改」那一页（Miyu 终端 `app/files_page.rs`，蓝图 `tui.md`「文件修改」第 2 条；2026-11-20 项目主人）：点侧边栏那一段
//! 的一行、`/files` 开；整屏归它：左边看选中那个文件的差异（这个会话里改它的每一步照先后接起来，中间一道细线），右边完整的
//! 文件列表。`↑` `↓`（`j` `k`）换文件，`PgUp` `PgDn`、滚轮滚差异，点右边一行换过去，`Esc` 回会话（正文、输入框都照原样）。
//!
//! 和 Miyu 不一样的：
//! - Miyu 先照参数比的画、同时问核心完整的差异（`view.detail`，带行号）换上。LiteMiyu 的内核不存改以前的文件（撤销也不改回
//!   文件），只照工具调用的参数比：写入的整份算加上的，编辑的片段不带行号（同正文里点开的差异，`diff.rs`）。
//! - 正文只留几屏（`window.rs`）：常驻的段里改这个文件的几步当场照参数比；放掉的段里的，开这一页、换文件时让引擎在读完就退
//!   的线程里把那一截日志读一遍、只交回这个文件的差异（[`Command::Changes`]），到了换上；换了文件、关了页就扔。差异不常驻。

use std::rc::Rc;

use crossterm::event::{Event, KeyCode, KeyEventKind, MouseEventKind};

use super::App;
use crate::core::{Command, Update};
use crate::diff::Diff;
use crate::transcript::Transcript;

/// 滚轮一格滚几行。
const WHEEL: usize = 3;

/// 这一页的状态。
#[derive(Debug, Default)]
pub struct FilesPage {
    /// 选中第几个文件（[`Transcript::changed`] 的下标）。
    pub selected: usize,
    /// 差异滚到第几行。
    pub top: usize,
    /// 选中的那个文件放掉的段里的改动：向引擎要的、读来的。没有放掉的段的是 `None`。
    read: Option<Read>,
}

/// 从日志读的一个文件的改动。
#[derive(Debug)]
struct Read {
    /// 哪个文件。
    path: String,
    /// 要的是日志里哪一截：从哪一句话的序号起、到哪一句话以前。
    range: (u64, u64),
    /// 读到哪了。
    found: Fetched,
}

/// 向引擎要的那一截读到哪了。
#[derive(Debug)]
enum Fetched {
    /// 要了，还没到。
    Waiting,
    /// 读来的：（哪一轮，差异），照先后。
    Ready(Vec<(u64, Rc<Diff>)>),
    /// 读不了。
    Failed,
}

/// 那一页里一处改动画什么。
#[derive(Debug, Clone)]
pub enum Shown {
    /// 差异。
    Diff(Rc<Diff>),
    /// 还在从日志读。
    Loading,
    /// 读不出来。
    Unreadable,
}

impl FilesPage {
    /// 文件 `path` 的每一处改动，照先后：常驻的段里的照步骤的参数比，放掉的段里的照日志读来的。
    pub fn changes(&self, t: &Transcript, path: &str) -> Vec<Shown> {
        let read = self.read.as_ref().filter(|r| r.path == path);
        let mut out = Vec::new();
        for (turn, piece) in t.changed_in(path) {
            if t.pieces().resident(piece) {
                let here = crate::transcript::steps_of(&t.entries[t.span(piece)], path)
                    .filter(|(at, _)| *at == turn)
                    .filter_map(|(_, args)| crate::diff::from_args(args))
                    .map(Shown::Diff);
                out.extend(here);
                continue;
            }
            match read.map(|r| &r.found) {
                Some(Fetched::Ready(found)) => out.extend(
                    found
                        .iter()
                        .filter(|(at, _)| *at == turn)
                        .map(|(_, diff)| Shown::Diff(diff.clone())),
                ),
                Some(Fetched::Failed) => out.push(Shown::Unreadable),
                Some(Fetched::Waiting) | None => out.push(Shown::Loading),
            }
        }
        out
    }
}

impl App {
    /// 开这一页，选上第 `index` 个文件；这个会话还没改过文件的说一句。
    pub(super) fn open_files(&mut self, index: usize) {
        let count = self.talk.transcript.changed().len();
        if count == 0 {
            let text = self.config.text().files.none.clone();
            return self.hint(text, false);
        }
        let page = self.boxes.files_page.get_or_insert_with(FilesPage::default);
        page.selected = index.min(count - 1);
        page.top = 0;
        self.screen.draw_now = true;
        self.ask_changes();
    }

    /// 点了侧边栏「文件修改」的第 `row` 行（标题下面起）：开那一页选上这个文件；「还有 N 个」那一行选上第一个没列出来的。
    pub(super) fn files_clicked(&mut self, row: u16) {
        if let Some(index) = self.screen.files.get(usize::from(row)).copied() {
            self.open_files(index);
        }
    }

    /// 这一页开着时的一个事件。
    pub(super) fn files_event(&mut self, event: &Event) {
        match event {
            Event::Key(key) if key.kind == KeyEventKind::Release => {}
            Event::Key(key) => self.files_key(key.code),
            Event::Mouse(mouse) => {
                let at = ratatui::layout::Position::new(mouse.column, mouse.row);
                self.screen.pointer = Some(at);
                match mouse.kind {
                    MouseEventKind::ScrollDown => self.files_scroll(WHEEL, true),
                    MouseEventKind::ScrollUp => self.files_scroll(WHEEL, false),
                    MouseEventKind::Down(_) if self.screen.files_page.list.contains(at) => {
                        let row = usize::from(at.y - self.screen.files_page.list.y);
                        if let Some(index) = self.screen.files_page.rows.get(row).copied() {
                            self.files_select(index);
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }

    fn files_key(&mut self, code: KeyCode) {
        let Some(page) = self.boxes.files_page.as_ref() else {
            return;
        };
        let selected = page.selected;
        let view = usize::from(self.screen.files_page.diff.height).max(1);
        match code {
            KeyCode::Esc | KeyCode::Char('q') => {
                self.boxes.files_page = None;
                self.screen.draw_now = true;
            }
            KeyCode::Down | KeyCode::Char('j') => self.files_select(selected + 1),
            KeyCode::Up | KeyCode::Char('k') => self.files_select(selected.saturating_sub(1)),
            KeyCode::PageDown | KeyCode::Char(' ') => self.files_scroll(view, true),
            KeyCode::PageUp => self.files_scroll(view, false),
            KeyCode::Home => self.files_scroll(usize::MAX, false),
            KeyCode::End => self.files_scroll(usize::MAX, true),
            _ => {}
        }
    }

    /// 换到第 `index` 个文件（到头停）：差异从头看，放掉的段里的改动向引擎要。
    fn files_select(&mut self, index: usize) {
        let count = self.talk.transcript.changed().len();
        if let Some(page) = self.boxes.files_page.as_mut() {
            let index = index.min(count.saturating_sub(1));
            if index != page.selected {
                page.selected = index;
                page.top = 0;
            }
        }
        self.ask_changes();
    }

    /// 差异往下（`down`）或往上滚几行，到头停（最多滚到哪照上一帧画的，`ui/files_page.rs`）。
    fn files_scroll(&mut self, by: usize, down: bool) {
        let most = self.screen.files_page.most;
        if let Some(page) = self.boxes.files_page.as_mut() {
            let now = page.top.min(most);
            page.top = if down {
                now.saturating_add(by).min(most)
            } else {
                now.saturating_sub(by)
            };
        }
    }

    /// 这一页开着、这个会话还有改过的文件；开着但没有了的（换了会话这类）关掉。
    pub(crate) fn files_open(&mut self) -> bool {
        if self.boxes.files_page.is_some() && self.talk.transcript.changed().is_empty() {
            self.boxes.files_page = None;
        }
        self.boxes.files_page.is_some()
    }

    /// 选中那个文件有改动在放掉的段里、还没要过的：向引擎要那一截日志里它的差异（同读回一样排进 `wanted`，画完这一帧
    /// 交给引擎）。读来的只留选中这一个文件的。这一页开着时又放掉了段（在跑的那一轮开了新的一段，`updates.rs`）也照这里
    /// 重要一遍：要的那一截变了才要。
    pub(super) fn ask_changes(&mut self) {
        let t = &self.talk.transcript;
        let Some(page) = self.boxes.files_page.as_mut() else {
            return;
        };
        let files = t.changed();
        let Some(path) = files.get(page.selected).map(|f| f.path) else {
            return;
        };
        // 放掉的段里的那几处：从最早那一段开头读到最晚那一段末尾。
        let shed: Vec<usize> = t
            .changed_in(path)
            .into_iter()
            .map(|(_, piece)| piece)
            .filter(|&p| !t.pieces().resident(p))
            .collect();
        let range = shed
            .first()
            .zip(shed.last())
            .and_then(|(&first, &last)| Some((t.first_seq(first)?, t.first_seq(last + 1)?)));
        let (Some(range), Some(session)) = (range, t.session.clone()) else {
            page.read = None;
            return;
        };
        if page
            .read
            .as_ref()
            .is_some_and(|r| r.path == path && r.range == range)
        {
            return;
        }
        let (from, next) = range;
        page.read = Some(Read {
            path: path.to_string(),
            range,
            found: Fetched::Waiting,
        });
        let command = Command::Changes {
            session,
            path: path.to_string(),
            after: from.saturating_sub(1),
            upto: next.saturating_sub(1),
            texts: self.config.text_shared(),
        };
        self.talk.wanted.push(command);
    }

    /// 日志里读来的改动到了（`Update::Changes`）：这一页开着、还在等这个会话这个文件的，换上；别的（关了页、换了文件、
    /// 换了会话以后才到的）不要。
    pub(super) fn changes_arrived(&mut self, update: Update) {
        let Update::Changes {
            session,
            path,
            found,
        } = update
        else {
            return;
        };
        if self.talk.transcript.session.as_deref() != Some(session.as_str()) {
            return;
        }
        let read = self.boxes.files_page.as_mut().and_then(|p| p.read.as_mut());
        let waiting = |r: &&mut Read| r.path == path && matches!(r.found, Fetched::Waiting);
        let Some(read) = read.filter(waiting) else {
            return;
        };
        read.found = match found {
            Ok(found) => Fetched::Ready(
                found
                    .into_iter()
                    .map(|f| (f.turn, Rc::new(f.diff)))
                    .collect(),
            ),
            Err(_) => Fetched::Failed,
        };
        self.screen.draw_now = true;
    }
}
