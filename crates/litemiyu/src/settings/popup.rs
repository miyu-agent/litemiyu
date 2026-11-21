//! 盖在页上的几种窗（Miyu 终端 `settings/pages/choose.rs`、`settings/popup.rs`，蓝图「配置页」第 13、20、33、34 条）：下拉的
//! 选择窗、填一行字的编辑窗、能筛的长列表。
//! 窗只管自己的光标和字；选好、填好了交回去，写到哪由开窗的一方定。

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// 选择窗：一个选项一行，现在的值前面一个点。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choose {
    /// 标题：这一项的名字。
    pub title: String,
    /// 选项：给人看的名字。
    pub names: Vec<String>,
    /// 光标在第几个。
    pub at: usize,
    /// 现在的值是第几个；不在选项里的没有。
    pub current: Option<usize>,
}

/// 编辑窗：一行字，光标在第几个字后面。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    /// 标题：这一项的名字。
    pub title: String,
    /// 框里的字。
    pub text: String,
    /// 光标在第几个字（按字算，不按字节）。
    pub cursor: usize,
    /// 打的字遮住（密钥）。
    pub masked: bool,
    /// 填的不收：红字写在窗里（Miyu「请输入数字」），再打字就收起。
    pub error: Option<String>,
}

/// 问一句的窗（Miyu `settings/popup.rs` 的 `Confirm`）：标题、暗色的几行话、一排按钮，`←` `→` 挑按钮。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Confirm {
    /// 标题。
    pub title: String,
    /// 暗色的几行话。
    pub lines: Vec<String>,
    /// 按钮：名字、是不是删除那种（停着时红底）。
    pub buttons: Vec<(String, bool)>,
    /// 停在第几个按钮。
    pub at: usize,
}

impl Confirm {
    /// 按一个键：`←` `→`（`h` `l`、`↑` `↓`、`Tab`）挑按钮，`Enter` 选，`Esc` 收起。
    pub fn key(&mut self, key: KeyEvent) -> Done {
        let n = self.buttons.len().max(1);
        match key.code {
            KeyCode::Esc => return Done::Cancel,
            KeyCode::Enter => return Done::Accept,
            KeyCode::Char('h' | 'k') | KeyCode::Left | KeyCode::Up => {
                self.at = (self.at + n - 1) % n
            }
            KeyCode::Char('l' | 'j') | KeyCode::Right | KeyCode::Down | KeyCode::Tab => {
                self.at = (self.at + 1) % n;
            }
            _ => {}
        }
        Done::Open
    }
}

/// 窗里按了一个键以后。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Done {
    /// 接着开着。
    Open,
    /// 收起，什么都不改。
    Cancel,
    /// 选好、填好了。
    Accept,
}

impl Choose {
    /// 开窗：光标停在现在的值上，没有的停在第一个。
    pub fn new(title: String, names: Vec<String>, current: Option<usize>) -> Choose {
        Choose {
            title,
            names,
            at: current.unwrap_or(0),
            current,
        }
    }

    /// 按一个键：`j` `k` `↑` `↓` 挪，`Enter` 选，`Esc`、`q` 收起（同 Miyu `settings/pages/choose.rs` 的 `press`）。
    pub fn key(&mut self, key: KeyEvent) -> Done {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => Done::Cancel,
            KeyCode::Enter => Done::Accept,
            KeyCode::Down | KeyCode::Char('j') => {
                self.at = (self.at + 1).min(self.names.len().saturating_sub(1));
                Done::Open
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.at = self.at.saturating_sub(1);
                Done::Open
            }
            _ => Done::Open,
        }
    }
}

impl Edit {
    /// 开窗：框里是 `text`，光标在最后。
    pub fn new(title: String, text: String, masked: bool) -> Edit {
        let cursor = text.chars().count();
        Edit {
            title,
            text,
            cursor,
            masked,
            error: None,
        }
    }

    /// 按一个键：敲字、`Backspace` `Delete`、`←` `→` `Home` `End`、`Ctrl+U` 清空；`Enter` 交，`Esc` 收起。
    pub fn key(&mut self, key: KeyEvent) -> Done {
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        if key.code != KeyCode::Enter {
            self.error = None;
        }
        match key.code {
            KeyCode::Esc => return Done::Cancel,
            KeyCode::Enter => return Done::Accept,
            KeyCode::Char('u') if control => {
                self.text.clear();
                self.cursor = 0;
            }
            KeyCode::Char(c) if !control => self.insert(&c.to_string()),
            KeyCode::Backspace if self.cursor > 0 => {
                self.cursor -= 1;
                self.remove(self.cursor);
            }
            KeyCode::Delete if self.cursor < self.text.chars().count() => self.remove(self.cursor),
            KeyCode::Left => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Right => self.cursor = (self.cursor + 1).min(self.text.chars().count()),
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.text.chars().count(),
            _ => {}
        }
        Done::Open
    }

    /// 粘贴进来的字：换行去掉（一行字的框）。
    pub fn paste(&mut self, text: &str) {
        let flat: String = text.chars().filter(|c| !c.is_control()).collect();
        self.insert(&flat);
    }

    /// 给人看的那一行：遮住的每个字写成 `•`。
    pub fn shown(&self) -> String {
        if self.masked {
            "•".repeat(self.text.chars().count())
        } else {
            self.text.clone()
        }
    }

    /// 在光标处插进 `text`。
    fn insert(&mut self, text: &str) {
        let at = self.byte(self.cursor);
        self.text.insert_str(at, text);
        self.cursor += text.chars().count();
    }

    /// 删掉第 `index` 个字。
    fn remove(&mut self, index: usize) {
        let at = self.byte(index);
        self.text.remove(at);
    }

    /// 第 `index` 个字从第几个字节起。
    fn byte(&self, index: usize) -> usize {
        self.text
            .char_indices()
            .nth(index)
            .map_or(self.text.len(), |(at, _)| at)
    }
}

/// 勾选窗（Miyu 终端 `settings/pages/ticks.rs`）：一个选项一行，`[x]` 勾着；空格勾上、取消，`Enter` 交，`Esc` 收起。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ticks {
    /// 标题。
    pub title: String,
    /// 一行一个。
    pub names: Vec<String>,
    /// 每一行勾没勾。
    pub checked: Vec<bool>,
    /// 光标在第几行。
    pub at: usize,
}

impl Ticks {
    /// 开窗：光标在第一行。
    pub fn new(title: String, names: Vec<String>, checked: Vec<bool>) -> Ticks {
        Ticks {
            title,
            names,
            checked,
            at: 0,
        }
    }

    /// 按一个键。
    pub fn key(&mut self, key: KeyEvent) -> Done {
        match key.code {
            KeyCode::Esc => return Done::Cancel,
            KeyCode::Enter => return Done::Accept,
            KeyCode::Down | KeyCode::Char('j') => {
                self.at = (self.at + 1).min(self.names.len().saturating_sub(1));
            }
            KeyCode::Up | KeyCode::Char('k') => self.at = self.at.saturating_sub(1),
            KeyCode::Char(' ') => {
                if let Some(on) = self.checked.get_mut(self.at) {
                    *on = !*on;
                }
            }
            _ => {}
        }
        Done::Open
    }
}

/// 窗里的东西。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Window {
    /// 选择窗。
    Choose(Choose),
    /// 编辑窗。
    Edit(Edit),
    /// 勾选窗。
    Ticks(Ticks),
    /// 问一句的窗。
    Confirm(Confirm),
}

impl Window {
    /// 按一个键。
    pub fn key(&mut self, key: KeyEvent) -> Done {
        match self {
            Window::Choose(choose) => choose.key(key),
            Window::Edit(edit) => edit.key(key),
            Window::Ticks(ticks) => ticks.key(key),
            Window::Confirm(confirm) => confirm.key(key),
        }
    }
}

/// 选好、填好了写到哪。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// 照清单画的一页里的一项：键。
    Item(String),
    /// 从 Miyu 导入：勾要导的几家。
    Import,
    /// 从 Miyu 导入：问同名的覆盖还是跳过，带着勾了的几家（在读到的那一份里的下标）。
    ImportClash(Vec<usize>),
    /// 照清单画的一页里 `d` 恢复默认先问一句：清单里的第几项。
    Unset(usize),
    /// 「预设」页新建（`presets.rs`）：填名字；带着以自带的哪一个为底（没有的是全开的一份）。
    PresetNew(Option<String>),
    /// 「预设」页的表单里改名字：原来的名字。
    PresetRename(String),
    /// 「预设」页 `d` 删除先问一句：哪个预设。
    PresetDelete(String),
    /// 开头配色方案那一步的一项（键）：选好了先记下，到「下一步」一起存（Miyu `oobe/theme.rs`）。
    Pick(String),
}

/// 开着的窗：窗和写到哪。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Popup {
    /// 窗。
    pub window: Window,
    /// 写到哪。
    pub target: Target,
}

impl Popup {
    /// 开一个窗。
    pub fn new(window: Window, target: Target) -> Popup {
        Popup { window, target }
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::{Choose, Done, Edit};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn the_choice_starts_on_the_current_value_and_stops_at_the_ends() {
        let mut choose = Choose::new("t".into(), vec!["a".into(), "b".into()], Some(1));
        assert_eq!(choose.at, 1);
        assert_eq!(choose.key(key(KeyCode::Char('j'))), Done::Open);
        assert_eq!(choose.at, 1, "到底了不绕回去");
        choose.key(key(KeyCode::Up));
        choose.key(key(KeyCode::Up));
        assert_eq!(choose.at, 0);
        assert_eq!(choose.key(key(KeyCode::Enter)), Done::Accept);
        assert_eq!(choose.key(key(KeyCode::Esc)), Done::Cancel);
        assert_eq!(
            choose.key(key(KeyCode::Char('q'))),
            Done::Cancel,
            "同 Miyu：q 也收起"
        );
    }

    #[test]
    fn editing_counts_characters_not_bytes() {
        let mut edit = Edit::new("t".into(), "地址".into(), false);
        edit.key(key(KeyCode::Left));
        edit.key(key(KeyCode::Char('x')));
        assert_eq!(edit.text, "地x址");
        edit.key(key(KeyCode::Backspace));
        edit.key(key(KeyCode::Backspace));
        assert_eq!(edit.text, "址");
        edit.key(key(KeyCode::End));
        edit.paste("a\nb");
        assert_eq!(edit.text, "址ab");
        edit.key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        assert_eq!(edit.text, "");
    }

    #[test]
    fn a_masked_field_shows_dots() {
        let edit = Edit::new("key".into(), "sk-1".into(), true);
        assert_eq!(edit.shown(), "••••");
    }
}
