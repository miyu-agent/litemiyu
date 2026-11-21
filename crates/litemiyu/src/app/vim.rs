//! 列表开着时 `Ctrl+J`、`Ctrl+K` 当 `↓`、`↑`（蓝图 `tui.md`「按键」，2026-10-01 项目主人要 vim 的上下）；开着时不能打字的框
//! 直接 `j`、`k` 也行（2026-10-11 项目主人）；按键模式是 Vim 时会话列表、`/model` 没在打搜索的字也算（2026-10-21）。
//! 只换按键本身，后面照常分给开着的那个列表；抽屉里写字时 `Ctrl+J` 照旧换行，不换。

use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};

use super::{App, Panel};

impl App {
    /// 有列表、面板开着，或者抽屉开着、没在写字时，换掉这个按键；`menu_open`：命令列表、`@` 文件列表开着。
    pub(super) fn vim_keys(&self, event: Event, menu_open: bool) -> Event {
        let listing = menu_open
            || self.typing.history.open
            || self.boxes.panel.is_some()
            || (self.talk.drawers.open()
                && !self.talk.drawers.editing()
                && !self.talk.drawers.command_scrolls());
        let no_typing = self.no_typing();
        match event {
            Event::Key(key) => Event::Key(plain(step(key, listing), no_typing)),
            other => other,
        }
    }

    /// 开着时不能打字的框；抽屉开着时按键先归抽屉（它自己认 vim 键）。
    fn no_typing(&self) -> bool {
        !self.talk.drawers.open()
            && (self.vim_list_idle()
                || matches!(
                    self.boxes.panel,
                    Some(
                        Panel::Effort { .. }
                            | Panel::Presets
                            | Panel::Jobs
                            | Panel::Help { .. }
                            | Panel::Status { .. }
                    )
                ))
    }

    /// 按键模式是 Vim，会话列表、`/model`、输入历史列表（`/jump` 同一个；Miyu 发版前 check 第 31 条）开着、没在打搜索的字
    /// （`list_search.rs`）。
    fn vim_list_idle(&self) -> bool {
        if !self.config.layout.vim {
            return false;
        }
        if self.typing.history.open {
            return !self.typing.history.typing;
        }
        match self.boxes.panel {
            Some(Panel::Sessions) => self.boxes.sessions.as_ref().is_some_and(|l| !l.typing),
            Some(Panel::Models) => self.boxes.model_list.as_ref().is_some_and(|l| !l.typing),
            _ => false,
        }
    }
}

/// 开着时不能打字的框（`no_typing`：`/effort`、后台面板、`/help`、`/status`，按键模式是 Vim 时没在打搜索的字的列表）：直接按
/// `j`、`k` 也换成 `↓`、`↑`（2026-10-11 项目主人）。会话列表、换模型、命令列表、输入历史打字是搜，不换。
pub fn plain(key: KeyEvent, no_typing: bool) -> KeyEvent {
    if !no_typing || key.modifiers != KeyModifiers::NONE {
        return key;
    }
    let code = match key.code {
        KeyCode::Char('j') => KeyCode::Down,
        KeyCode::Char('k') => KeyCode::Up,
        _ => return key,
    };
    KeyEvent { code, ..key }
}

/// `list_open`：有列表、面板开着，或者抽屉开着、没在写字。是的话把 `Ctrl+J`、`Ctrl+K` 换成 `↓`、`↑`。
pub fn step(key: KeyEvent, list_open: bool) -> KeyEvent {
    if !list_open || key.modifiers != KeyModifiers::CONTROL {
        return key;
    }
    let code = match key.code {
        KeyCode::Char('j' | 'J') => KeyCode::Down,
        KeyCode::Char('k' | 'K') => KeyCode::Up,
        _ => return key,
    };
    KeyEvent {
        code,
        modifiers: KeyModifiers::NONE,
        ..key
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::step;

    #[test]
    fn ctrl_j_and_k_move_only_while_a_list_is_open() {
        let ctrl = |c| KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL);
        assert_eq!(step(ctrl('j'), true).code, KeyCode::Down);
        assert_eq!(step(ctrl('k'), true).code, KeyCode::Up);
        assert_eq!(step(ctrl('k'), true).modifiers, KeyModifiers::NONE);
        assert_eq!(step(ctrl('j'), false), ctrl('j'), "没开列表：照旧换行");
        let plain = KeyEvent::new(KeyCode::Char('j'), KeyModifiers::NONE);
        assert_eq!(step(plain, true), plain, "不按 Ctrl 的照常打字");
        let shifted = KeyEvent::new(
            KeyCode::Char('J'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        );
        assert_eq!(step(shifted, true), shifted);
    }

    #[test]
    fn plain_j_and_k_move_in_boxes_that_take_no_typing() {
        // 2026-10-11 项目主人：人格框这类开着不能打字的框，直接 j、k 上下。
        let plain = |c| KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE);
        assert_eq!(super::plain(plain('j'), true).code, KeyCode::Down);
        assert_eq!(super::plain(plain('k'), true).code, KeyCode::Up);
        assert_eq!(
            super::plain(plain('j'), false),
            plain('j'),
            "能打字的列表照常打字"
        );
        assert_eq!(super::plain(plain('x'), true), plain('x'));
    }

    /// 2026-11-13 项目主人：「我选择了 vimkey，但是 /model 之类的列表还是 ctrl+JK 上下」。按键模式是 Vim 时会话列表、`/model`、
    /// 输入历史（`/jump` 同一个）没在打搜索的字就是不能打字的框，`j`、`k` 换成上下；按了 `/` 在搜的、方向键模式的照旧是字。
    #[test]
    fn with_vim_keys_idle_lists_take_j_and_k_as_moves() {
        use crate::app::{App, Panel};
        use crate::config::Config;
        use crate::core::Core;
        let mut app = App::new(
            Config::builtin().expect("出厂的配置读得懂"),
            Core::new(|_| true),
        );
        app.config.layout.vim = true;
        app.boxes.model_list = Some(Default::default());
        app.boxes.panel = Some(Panel::Models);
        assert!(app.no_typing(), "/model 没在搜");
        app.boxes.model_list.as_mut().expect("开着").typing = true;
        assert!(!app.no_typing(), "按了 / 在搜：j 是字");
        app.boxes.sessions = Some(Default::default());
        app.boxes.panel = Some(Panel::Sessions);
        assert!(app.no_typing(), "会话列表没在搜");
        app.boxes.panel = None;
        app.typing.history.open_jump();
        assert!(app.no_typing(), "/jump 没在搜");
        app.typing.history.typing = true;
        assert!(!app.no_typing(), "/jump 在搜");
        app.config.layout.vim = false;
        app.typing.history.typing = false;
        assert!(!app.no_typing(), "方向键模式：打字就是搜");
        app.typing.history.close();
        app.boxes.panel = Some(Panel::Effort { selected: 0 });
        assert!(
            app.no_typing(),
            "/effort 不能打字，两种模式都认 j、k（同 Miyu）"
        );
    }
}
