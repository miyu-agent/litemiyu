//! 开头里打字的一格（照 Miyu 终端 `oobe/field.rs` 搬，去掉几行字的那一种：LiteMiyu 的开头没有要写几行的格）：一行的、
//! 不显示的（密钥）。`Enter`、`Esc`、上下这些交回去由那一步管；粘贴照收，去掉换行。光标停上去不能直接打字：`Enter` 开始编辑，
//! 编辑时 `Enter` 写好、`Esc` 回到开始编辑以前的字（同 Miyu，2026-10-10 项目主人：「回车编辑回车退出编辑才对」）。

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::input::Editor;

/// 遮住的密钥一个字画成什么。
pub const DOT: &str = "•";

/// 一格。
#[derive(Debug, Default)]
pub struct Field {
    /// 字和光标。
    pub editor: Editor,
    /// 不显示（密钥）：一个字一个 [`DOT`]。
    pub secret: bool,
    /// 在编辑：开始编辑以前的字（`Esc` 回到它）。
    editing: Option<String>,
    /// 画的时候 `://` 中间插一个零宽的空格，终端不把它认成网址（地址那一格）。
    pub unlinked: bool,
    /// 折成几行画时上一帧露出的第一行（Miyu `Field::top`）：光标碰到上边、下边才挪。
    pub top: std::cell::Cell<usize>,
}

/// 地址里的 `://` 中间插一个零宽空格，终端认不成网址、不给点（Miyu `oobe/field.rs` 的 `unlink`）。
pub fn unlink(text: &str) -> String {
    text.replace("://", "\u{200B}://")
}

/// 编辑时按了一个键以后。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edit {
    /// 接着编辑。
    Stay,
    /// `Enter`：写好了。
    Done,
    /// `Esc`：字回到开始编辑以前。
    Cancelled,
}

impl Field {
    /// 一行字。
    pub fn line() -> Self {
        Self::default()
    }

    /// 密钥。
    pub fn secret() -> Self {
        Self {
            secret: true,
            ..Self::default()
        }
    }

    /// 地址。
    pub fn url() -> Self {
        Self {
            unlinked: true,
            ..Self::default()
        }
    }

    /// 一开始就在编辑的（搜模型那一格：`/` 以后直接打字）。
    #[must_use]
    pub fn live(mut self) -> Self {
        self.editing = Some(String::new());
        self
    }

    /// 在编辑。
    pub fn editing(&self) -> bool {
        self.editing.is_some()
    }

    /// 开始编辑：光标挪到最后。
    pub fn begin(&mut self) {
        self.editing = Some(self.text().to_string());
        let end = self.text().len();
        self.editor.move_to(end, false);
    }

    /// 编辑时按一个键。
    pub fn edit(&mut self, key: KeyEvent) -> Edit {
        match key.code {
            KeyCode::Enter => {
                self.editing = None;
                Edit::Done
            }
            KeyCode::Esc => {
                if let Some(before) = self.editing.take() {
                    self.editor.set(&before);
                }
                Edit::Cancelled
            }
            _ => {
                self.key(key);
                Edit::Stay
            }
        }
    }

    /// 字。
    pub fn text(&self) -> &str {
        self.editor.text()
    }

    /// 去掉前后空白的字。
    pub fn trimmed(&self) -> String {
        self.text().trim().to_string()
    }

    /// 打字、挪光标、删字；不认的交回假。
    pub fn key(&mut self, key: KeyEvent) -> bool {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let editor = &mut self.editor;
        match key.code {
            KeyCode::Backspace if ctrl || alt => editor.delete_word(),
            KeyCode::Char('w') if ctrl => editor.delete_word(),
            KeyCode::Backspace => editor.backspace(),
            KeyCode::Delete => editor.delete(),
            KeyCode::Left if ctrl || alt => editor.word_left(false),
            KeyCode::Right if ctrl || alt => editor.word_right(false),
            KeyCode::Left => editor.left(false),
            KeyCode::Right => editor.right(false),
            KeyCode::Home => editor.move_to(0, false),
            KeyCode::End => editor.move_to(editor.text().len(), false),
            KeyCode::Char('a') if ctrl => editor.move_to(0, false),
            KeyCode::Char('e') if ctrl => editor.move_to(editor.text().len(), false),
            KeyCode::Char('u') if ctrl => editor.set(""),
            KeyCode::Char(c) if !ctrl && !alt => editor.insert(&c.to_string()),
            _ => return false,
        }
        true
    }

    /// 粘贴：去掉换行。
    pub fn paste(&mut self, text: &str) {
        let flat = text
            .trim_end_matches(['\r', '\n'])
            .replace(['\r', '\n'], " ");
        self.editor.insert(&flat);
    }

    /// 画出来的样子：密钥一个字一个点。
    pub fn shown(&self) -> String {
        if self.secret {
            DOT.repeat(self.text().chars().count())
        } else {
            self.text().to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::{Edit, Field};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    /// 同 Miyu `oobe/field.rs` 的 `typing_editing_and_what_is_left_to_the_step`：`Enter` 才开始编辑，`Esc` 回到以前的字。
    #[test]
    fn enter_begins_esc_puts_the_text_back_and_secrets_are_dots() {
        let mut field = Field::secret();
        assert!(!field.editing());
        field.begin();
        for c in "sk-1".chars() {
            assert_eq!(field.edit(key(KeyCode::Char(c))), Edit::Stay);
        }
        assert_eq!(field.shown(), "••••");
        assert_eq!(field.edit(key(KeyCode::Enter)), Edit::Done);
        assert_eq!(field.text(), "sk-1");
        field.begin();
        field.edit(key(KeyCode::Backspace));
        assert_eq!(field.edit(key(KeyCode::Esc)), Edit::Cancelled);
        assert_eq!(field.text(), "sk-1", "Esc 回到开始编辑以前");
        field.paste("x\ny\n");
        assert_eq!(field.text(), "sk-1x y", "粘贴去掉换行");
    }
}
