//! 输入框：文字编辑、折行、滚动、键盘和鼠标。
//!
//! 画在哪、多大由 `ui` 定；这里只记着上一次画的位置，好把鼠标坐标换回文字下标。

mod dropped;
mod editor;
mod label;
mod mouse;
mod pasted;
mod place;
mod recall;
mod saved;
mod shell;
mod wrap;

#[cfg(test)]
mod tests;

use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Position, Rect};

pub use editor::Editor;
pub use label::quoted;
pub use pasted::{Draft, PasteRule, Sent};
pub use saved::Saved;
pub use wrap::{VisualLine, locate, offset_at, pieces, tail_pieces, wrap, wrap_words};

/// 输入框处理完一个事件后，要外面做的事。
#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    /// 什么都不用做。
    None,
    /// 发出去这段话（连同里面的粘贴块，`Draft::expand` 是全文）。
    Submit(Draft),
    /// 把这段字放进剪贴板。
    Copy(String),
    /// 退出程序。
    Quit,
    /// 空着按了 `Ctrl+C`：不退出，提示用 `Ctrl+D` 退出。
    ExitHint,
    /// `Ctrl+C` 清空了输入框：清掉的留着一份，提示按 `↑` 找回。
    Cleared,
    /// `Ctrl+Z`：挂起到后台，`fg` 回来接着用（Miyu 蓝图「按键」）。
    Suspend,
    /// `Ctrl+G`：用编辑器写输入框里的话（Miyu 蓝图「按键」）。
    Compose,
    /// 点了输入框里的文件块：用系统的程序打开这个文件（Miyu 终端 `input/mouse.rs`）。
    Open(String),
}

/// 输入框的全部状态。
#[derive(Debug)]
pub struct InputBox {
    /// 最多长到几行，再多就在框里滚。
    max_rows: u16,
    /// 两次点击隔多久以内算双击。
    double_click: Duration,
    /// 文字和光标。
    pub editor: Editor,
    /// 框里第一行显示的是第几行。
    scroll: usize,
    /// 上下移动时想回到的列：经过短行时列会变小，再到长行要回到原来的列。
    goal_col: Option<usize>,
    /// 按着左键在拖。
    dragging: bool,
    /// 上一次按下左键的时间和位置，判双击。
    last_click: Option<(Instant, usize)>,
    /// 按在文件块上还没松开：松开时打开这个文件；拖起来就从这个位置拖选（Miyu 终端 `input/mod.rs` 同名的那一项）。
    pressed: Option<(std::path::PathBuf, usize)>,
    /// 鼠标悬停在哪个文件块上（字节范围）：画的时候加下划线，指针变手。
    hover: Option<(usize, usize)>,
    /// 上一次画的时候，文字区在屏幕上的位置。
    area: Rect,
    /// `Ctrl+S` 暂存的字，连同粘贴块。
    stash: Option<Draft>,
    /// 发过的话和命令，从旧到新，连同粘贴块和发出去的时刻：翻历史、历史列表用。
    history: Vec<Sent>,
    /// 正在翻历史，翻到第几条；没在翻是 `None`。
    browsing: Option<usize>,
    /// 开始翻历史之前没发的那句：翻过最新一条回到它。
    draft: Draft,
    /// 开始翻历史时框里的字：不空的只翻以它开头的（像 fish，2026-10-17 项目主人定：打「/loop」按 `↑` 调出上一句
    /// 带参数的 `/loop`；原话拿已经砍掉的 `/goal` 举例）。
    prefix: String,
    /// 画的时候把光标滚进可见范围。滚轮滚过以后关掉，不然滚一下就被拽回光标那里。
    follow: bool,
    /// 大段粘贴什么时候收成一块、块上写什么（`pasted.rs`）。
    paste_rule: PasteRule,
    /// `Ctrl+C` 刚清掉的那一句，只留最近一份：空着按 `↑` 先拿回它；发出去一句话、再清一次就换掉。
    cleared: Option<Draft>,
    /// 在跑命令的模式里（`shell.rs`，「人自己跑命令 `!`」第 1 条）。
    shell: bool,
    /// 输入历史记在哪个文件（`saved.rs`），和读回来过没有：第一次翻历史时才读。
    saved: Saved,
    saved_read: bool,
    /// 撤销时放回来的那句（`recall.rs` 的 `put_back`）：恢复时还没动过的收回去。
    put_back: Option<String>,
    /// 在编辑上一句（`/edit`，`recall.rs` 的 `start_edit`）：改之前的那句。
    editing: Option<Draft>,
}

impl InputBox {
    /// 空的输入框。
    pub fn new(max_rows: u16, double_click: Duration) -> Self {
        Self {
            max_rows: max_rows.max(1),
            double_click,
            editor: Editor::default(),
            scroll: 0,
            goal_col: None,
            dragging: false,
            last_click: None,
            pressed: None,
            hover: None,
            area: Rect::default(),
            stash: None,
            history: Vec::new(),
            browsing: None,
            prefix: String::new(),
            draft: Draft::default(),
            follow: false,
            paste_rule: PasteRule::never(),
            cleared: None,
            put_back: None,
            editing: None,
            saved: Saved::default(),
            saved_read: false,
            shell: false,
        }
    }

    /// 照配置设大段粘贴收成一块的门槛和写法（蓝图「输入框」第 11 条）。
    pub fn set_paste_rule(&mut self, rule: PasteRule) {
        self.paste_rule = rule;
    }

    /// 现在输入框里的字，连同粘贴块。
    pub fn draft(&self) -> Draft {
        self.editor.draft()
    }

    /// 按当前宽度折好的行。
    pub fn lines(&self, width: u16) -> Vec<VisualLine> {
        wrap(self.editor.text(), width)
    }

    /// 框里要露出几行：跟着文字长，最少一行，最多到配置的上限。
    pub fn rows(&self, width: u16) -> u16 {
        let n = self.lines(width).len();
        u16::try_from(n)
            .unwrap_or(self.max_rows)
            .clamp(1, self.max_rows)
    }

    /// 画之前调用：记下文字区的位置，把光标滚进可见范围。返回第一行显示第几行。
    pub fn place(&mut self, area: Rect) -> usize {
        self.area = area;
        let lines = self.lines(area.width);
        let (row, _) = locate(self.editor.text(), &lines, self.editor.cursor());
        let rows = usize::from(area.height.max(1));
        if self.follow {
            if row < self.scroll {
                self.scroll = row;
            } else if row >= self.scroll + rows {
                self.scroll = row + 1 - rows;
            }
        }
        self.scroll = self.scroll.min(lines.len().saturating_sub(rows));
        self.scroll
    }

    /// 光标在屏幕上的位置；滚出框外时是 `None`。
    pub fn cursor_position(&self) -> Option<Position> {
        let lines = self.lines(self.area.width);
        let (row, col) = locate(self.editor.text(), &lines, self.editor.cursor());
        let row = row.checked_sub(self.scroll)?;
        if row >= usize::from(self.area.height) {
            return None;
        }
        Some(Position {
            x: self.area.x + u16::try_from(col).ok()?,
            y: self.area.y + u16::try_from(row).ok()?,
        })
    }

    /// 处理一个按键。
    pub fn key(&mut self, key: KeyEvent) -> Action {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        self.follow = true;
        // 字变了，悬停着的块的位置可能不对了：等鼠标再动一下重新认。
        self.hover = None;
        if !matches!(key.code, KeyCode::Up | KeyCode::Down) {
            self.goal_col = None;
        }
        if self.shell_key(key) {
            return Action::None;
        }
        let action = self.edit_key(key, ctrl, shift, alt);
        // 头一个字打成了 `!`：进跑命令的模式（`shell.rs`）。
        self.absorb_bang();
        action
    }

    /// 照按键改框里的字（[`InputBox::key`] 分出来的）。
    fn edit_key(&mut self, key: KeyEvent, ctrl: bool, shift: bool, alt: bool) -> Action {
        match key.code {
            // Shift+Enter 要终端报得出来（kitty 键盘协议）；报不出来的终端用 Alt+Enter 或 Ctrl+J。
            KeyCode::Enter if shift || alt => self.editor.insert("\n"),
            KeyCode::Char('j') if ctrl => self.editor.insert("\n"),
            KeyCode::Enter => {
                if self.editor.text().trim().is_empty() {
                    return Action::None;
                }
                self.scroll = 0;
                self.browsing = None;
                return Action::Submit(self.editor.take_draft());
            }
            // Ctrl+Shift+C 在认得 kitty 键盘协议的终端里报成大写的 C。
            KeyCode::Char('c' | 'C') if ctrl => return self.ctrl_c(),
            KeyCode::Char('d') if ctrl && self.editor.is_empty() => return Action::Quit,
            KeyCode::Char('z') if ctrl => return Action::Suspend,
            KeyCode::Char('g') if ctrl => return Action::Compose,
            // 回到输入框开头（2026-09-30 项目主人改：原来是全选）。
            KeyCode::Char('a') if ctrl => self.editor.move_to(0, false),
            KeyCode::Char('s') if ctrl => self.swap_stash(),
            KeyCode::Char('w') if ctrl => self.editor.delete_word(),
            KeyCode::Backspace if ctrl || alt => self.editor.delete_word(),
            KeyCode::Left if ctrl => self.editor.word_left(shift),
            KeyCode::Right if ctrl => self.editor.word_right(shift),
            KeyCode::Char(c) if !ctrl => self.editor.insert(c.encode_utf8(&mut [0; 4])),
            KeyCode::Backspace => self.editor.backspace(),
            KeyCode::Delete => self.editor.delete(),
            KeyCode::Left => self.editor.left(shift),
            KeyCode::Right => self.editor.right(shift),
            KeyCode::Up => self.vertical(-1, shift),
            KeyCode::Down => self.vertical(1, shift),
            KeyCode::Home => self.line_edge(false, shift),
            KeyCode::End => self.line_edge(true, shift),
            // 编辑上一句时 `Esc` 是取消：不编辑了，框里清空（「输入框」第 13 条）。
            KeyCode::Esc if self.editing() => self.cancel_edit(),
            KeyCode::Esc => self.editor.clear_selection(),
            _ => {}
        }
        Action::None
    }

    /// `Ctrl+V` 读来的是一批本机现成的文件、目录（文件管理器里复制的、复制的路径）：收成文件块，交回 `true`；不是的什么都
    /// 不做，交回 `false`（Miyu 终端 `input/mod.rs` 的 `paste_files`，蓝图「输入框」第 12 条）。`words` 交回块上文件、目录的
    /// 写法，是一批文件才问；交回 `None`（字读不懂）的也当不是。
    pub fn paste_files(
        &mut self,
        text: &str,
        words: impl FnOnce() -> Option<(String, String)>,
    ) -> bool {
        let home = std::env::var("HOME").ok();
        let Some(items) = dropped::dropped(text, home.as_deref()) else {
            return false;
        };
        let Some((file, folder)) = words() else {
            return false;
        };
        self.goal_col = None;
        self.follow = true;
        for (i, path) in items.into_iter().enumerate() {
            if i > 0 {
                self.editor.insert(" ");
            }
            self.put_path(path, (&file, &folder));
        }
        true
    }

    /// 处理一次粘贴（终端送来的：`Ctrl+Shift+V`、拖文件进来）：照原样是字，路径也是字（2026-10-10 项目主人：
    /// 「ctrl+shift+V 粘贴文字，ctrl+V 粘贴占位符」）；大段的收成一块（蓝图「输入框」第 11 条）。
    pub fn paste(&mut self, text: &str) {
        self.goal_col = None;
        self.follow = true;
        let clean = editor::clean(text);
        if self.paste_rule.folds(&clean) {
            let label = self.paste_rule.label(&clean);
            self.editor.insert_block(label, clean);
        } else {
            self.editor.insert(&clean);
        }
        self.absorb_bang();
    }

    /// Ctrl+C：有选区复制；没有选区但有字，清空；空着不退出，要外面提示用 Ctrl+D 退出。
    fn ctrl_c(&mut self) -> Action {
        // 复制的是原文：选中的粘贴块换回来（`tui.md`「输入框」第 11 条）。
        if let Some(text) = self.editor.selected_full() {
            return Action::Copy(text);
        }
        if self.editor.is_empty() {
            return Action::ExitHint;
        }
        // 清掉的连同块、附件留最近一份，不进输入历史；空着按 `↑` 先拿回它（「按键」`Ctrl+C`）。正在翻历史的照旧，
        // 翻之前没发的那句还在，翻过最新一条回到它。
        self.cleared = Some(self.editor.take_draft());
        self.editing = None;
        Action::Cleared
    }

    /// 有没有暂存着的字：有就在输入框第一行最右边写一个标记。
    pub fn stashed(&self) -> bool {
        self.stash.is_some()
    }

    /// Ctrl+S 暂存（照旧版）：有字存起来、清空；空着取回来；两边都有互换。
    fn swap_stash(&mut self) {
        let current = (!self.editor.is_empty()).then(|| self.editor.take_draft());
        if let Some(back) = std::mem::replace(&mut self.stash, current) {
            self.editor.set_draft(back);
        }
    }

    fn vertical(&mut self, delta: isize, extend: bool) {
        let lines = self.lines(self.area.width);
        let text = self.editor.text();
        let (row, col) = locate(text, &lines, self.editor.cursor());
        let goal = *self.goal_col.get_or_insert(col);
        let target = row.checked_add_signed(delta).filter(|&r| r < lines.len());
        let pos = match target {
            Some(r) => offset_at(text, &lines, r, goal),
            // 第一行再往上、最后一行再往下：翻输入历史；按着 Shift 的照旧到开头、结尾。
            None if !extend => return self.browse(delta),
            None if delta < 0 => 0,
            None => text.len(),
        };
        self.editor.move_to(pos, extend);
    }

    /// 翻输入历史：`delta` 小于 0 往旧的翻。翻过最新一条，回到开始翻之前没发的那句。
    fn browse(&mut self, delta: isize) {
        if delta < 0 {
            self.recall_saved();
        }
        // 空着、没在翻：`Ctrl+C` 刚清掉的那一句排在输入历史前面，先拿回它（「按键」`Ctrl+C`）。
        if delta < 0
            && self.browsing.is_none()
            && self.editor.is_empty()
            && let Some(draft) = self.cleared.take()
        {
            self.goal_col = None;
            self.editor.set_draft(draft);
            // 它排在输入历史最前面：再按 `↑` 接着往旧的翻，不照它开头筛；`↓` 回到空的框。
            self.browsing = Some(self.history.len());
            self.prefix.clear();
            self.draft = Draft::default();
            return;
        }
        if delta > 0 && self.browsing.is_none() {
            return;
        }
        if self.browsing.is_none() {
            self.prefix = self.editor.text().to_string();
        }
        let newest = self.history.len();
        let at = self.browsing.unwrap_or(newest);
        // 框里原来有字的，只翻以它开头的那几条（和它一模一样的跳过）；翻过最新一条回到原来的字。
        let prefix = &self.prefix;
        let fits = |i: &usize| {
            let text = &self.history[*i].draft.text;
            prefix.is_empty() || (text.starts_with(prefix.as_str()) && text != prefix)
        };
        let next = if delta < 0 {
            (0..at).rev().find(fits)
        } else {
            Some((at + 1..newest).find(fits).unwrap_or(newest))
        };
        let Some(next) = next else {
            return;
        };
        if self.browsing.is_none() {
            self.draft = self.as_typed(&self.editor.draft());
        }
        self.goal_col = None;
        if next == newest {
            self.browsing = None;
            let draft = std::mem::take(&mut self.draft);
            self.load(draft);
        } else {
            self.browsing = Some(next);
            self.load(self.history[next].draft.clone());
        }
    }

    /// 发过的话和命令，从旧到新（输入历史列表用）。
    pub fn sent(&self) -> &[Sent] {
        &self.history
    }

    /// 从输入历史列表里挑了一条：放进输入框，光标在末尾。框里原来有字的先存进暂存；暂存里已经有字的，
    /// 原来的字记进输入历史（`↑` 翻得到），不丢（`tui.md`「输入历史列表」第 4 条）。
    pub fn pick(&mut self, text: &str) {
        let current = self.editor.draft();
        if !current.text.is_empty() && current.text != text {
            if self.stash.is_none() {
                self.stash = Some(current);
            } else {
                self.remember(current);
            }
        }
        self.browsing = None;
        // 挑中的是记着的那一条：连同粘贴块放回来（同样的字记过几次的，取最近那次）。
        let picked = self
            .history
            .iter()
            .rev()
            .find(|s| s.draft.text == text)
            .map(|s| s.draft.clone());
        self.load(picked.unwrap_or_else(|| Draft::plain(text)));
    }

    /// 记下一句发出去的话或命令，连同粘贴块，翻历史用。和上一条一样的不重复记。
    pub fn remember(&mut self, sent: Draft) {
        if self.history.last().is_none_or(|last| last.draft != sent) {
            let sent = Sent {
                draft: sent,
                at: saved::now(),
            };
            // 记进文件：重启以后、别的会话里都翻得到（Miyu 蓝图「输入历史列表」第 8 条）。
            self.saved.append(&sent);
            self.history.push(sent);
        }
    }

    /// 输入历史记在这个文件里：以后发的接着记，翻历史时读回最近的（[`InputBox::recall_saved`]）。起来时不读。
    pub fn keep_history(&mut self, saved: Saved) {
        self.saved = saved;
        self.saved_read = false;
    }

    /// 第一次翻历史（`↑`、`Ctrl+R`）：把文件里记着的接在这一次发过的前面；读过的不再读。
    pub fn recall_saved(&mut self) {
        if std::mem::replace(&mut self.saved_read, true) {
            return;
        }
        // 这一次发的已经追加进文件了：读回来的里面有它们，照文件的为准。
        let mut earlier = self.saved.load(saved::KEEP);
        if earlier.is_empty() {
            return;
        }
        if let Some(i) = self.history.first().and_then(|first| {
            earlier
                .iter()
                .rposition(|s| s.draft == first.draft && s.at == first.at)
        }) {
            earlier.truncate(i);
        }
        earlier.append(&mut self.history);
        self.history = earlier;
    }

    fn line_edge(&mut self, end: bool, extend: bool) {
        let lines = self.lines(self.area.width);
        let (row, _) = locate(self.editor.text(), &lines, self.editor.cursor());
        let line = lines[row];
        self.editor
            .move_to(if end { line.end } else { line.start }, extend);
    }
}
