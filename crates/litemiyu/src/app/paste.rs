//! 粘贴和复制（蓝图 `tui.md`「按键」`Ctrl+V`、「输入框」第 11 条、「斜杠命令」`/copy`）：粘进来的字去哪、发出去在
//! 正文里记成什么，复制以后提示一句。

use super::App;
use crate::clipboard;
use crate::input::Draft;
use crate::transcript::Chip;

/// 发出去的一句在正文里记成的块：块上的字、原文（「正文」第 2 条）。
pub(super) fn chips(draft: &Draft) -> Vec<Chip> {
    draft
        .blocks
        .iter()
        .map(|b| Chip {
            label: draft.text[b.start..b.end].to_string(),
            full: b.text.clone(),
        })
        .collect()
}

/// 被退回的一句拼回输入框用的粘贴块。
pub(super) fn pasted(chips: Vec<Chip>) -> Vec<(String, String)> {
    chips.into_iter().map(|c| (c.label, c.full)).collect()
}

impl App {
    /// 粘贴进来的字（终端送来的、`Ctrl+V` 读剪贴板的）：历史列表开着的进「历史：」，抽屉开着的进抽屉里写的字，
    /// 别的进输入框（大段的收成一块，`tui.md`「输入框」第 11 条）。
    pub(super) fn paste(&mut self, text: &str) {
        if self.typing.history.open {
            self.typing
                .history
                .type_text(&text.replace(['\r', '\n'], " "));
        } else if self.talk.drawers.open() {
            self.drawer_paste(text);
        } else {
            self.typing.input.paste(text);
        }
    }

    /// `Ctrl+V`（Miyu 终端 `app/paste.rs::paste_clipboard`）：输入框里（没开历史列表和抽屉）复制的是文件的收成文件块，
    /// 读来的字全是本机现成的文件的也收成文件块；别的照字粘贴（`tui.md`「输入框」第 12 条，2026-10-10 项目主人：
    /// 「ctrl+shift+V 粘贴文字，ctrl+V 粘贴占位符」）。读不到、是空的，提示一句（「按键」）。Miyu 剪贴板里的截图收成附件
    /// 那一步不搬（LiteMiyu 没有附件）。
    pub(super) fn paste_clipboard(&mut self) {
        let into_input = !self.typing.history.open && !self.talk.drawers.open();
        if into_input
            && let Some(files) = clipboard::read_files()
            && self.paste_files(&files)
        {
            return;
        }
        match clipboard::read() {
            Ok(text) if text.is_empty() => {
                self.hint(self.config.text().clipboard_empty.clone(), false);
            }
            Ok(text) if into_input && self.paste_files(&text) => {}
            Ok(text) => self.paste(&text),
            // 抽屉、历史搜索里读不成字的多半是剪贴板里放着图片：这里只收字。
            Err(_) if !into_input => {
                self.hint(self.config.text().clipboard_text_only.clone(), false);
            }
            Err(_) => self.hint(self.config.text().clipboard_unreadable.clone(), false),
        }
    }

    /// 一段字全是本机现成的文件、目录：收成文件块，交回 `true`（`InputBox::paste_files`）。块上的字照 `file_block` 那两句
    /// （Miyu 的 `attach.file`、`attach.folder`），是一批文件才读；读不懂的（安装坏了）当不是，照字粘。
    fn paste_files(&mut self, text: &str) -> bool {
        let config = &self.config;
        self.typing.input.paste_files(text, || {
            let words = config.file_block_texts().ok()?;
            Some((words.file, words.folder))
        })
    }

    /// 复制进剪贴板，提示一句。
    pub(super) fn copy(&mut self, text: &str) {
        let words = self.config.text();
        let (message, good) = match clipboard::copy(text) {
            Ok(()) => (
                words
                    .copied
                    .replace("{count}", &text.chars().count().to_string()),
                true,
            ),
            Err(e) => (words.copy_failed.replace("{reason}", &e.to_string()), false),
        };
        self.hint(message, good);
    }
}
