//! 开头里挪光标的键（照 Miyu 终端 `oobe/nav.rs` 搬）：不打字的地方认 vim 的 `h` `j` `k` `l`；打字的格子里字照打，另认
//! `Ctrl+N` `Ctrl+P`；搜模型的列表打字是筛，另认 `Ctrl+J` `Ctrl+K`（同 fzf）。方向键、`Tab`、`Shift+Tab` 哪里都认。

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// 往哪挪。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Nav {
    /// 上。
    Up,
    /// 下。
    Down,
    /// 左。
    Left,
    /// 右。
    Right,
}

/// 哪里都认的。
fn common(key: KeyEvent) -> Option<Nav> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    match key.code {
        KeyCode::Up | KeyCode::BackTab => Some(Nav::Up),
        KeyCode::Down | KeyCode::Tab => Some(Nav::Down),
        KeyCode::Left => Some(Nav::Left),
        KeyCode::Right => Some(Nav::Right),
        KeyCode::Char('p') if ctrl => Some(Nav::Up),
        KeyCode::Char('n') if ctrl => Some(Nav::Down),
        _ => None,
    }
}

/// 不打字的地方：另认 `h` `j` `k` `l`。
pub fn plain(key: KeyEvent) -> Option<Nav> {
    if key
        .modifiers
        .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
    {
        return common(key);
    }
    match key.code {
        KeyCode::Char('k') => Some(Nav::Up),
        KeyCode::Char('j') => Some(Nav::Down),
        KeyCode::Char('h') => Some(Nav::Left),
        KeyCode::Char('l') => Some(Nav::Right),
        _ => common(key),
    }
}

/// 打字的格子里：只认上下（左右挪光标）。
pub fn typing(key: KeyEvent) -> Option<Nav> {
    common(key).filter(|n| matches!(n, Nav::Up | Nav::Down))
}

/// 搜模型的时候：另认 `Ctrl+J` `Ctrl+K`。
pub fn search(key: KeyEvent) -> Option<Nav> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    match key.code {
        KeyCode::Char('k') if ctrl => Some(Nav::Up),
        KeyCode::Char('j') if ctrl => Some(Nav::Down),
        _ => typing(key),
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::{Nav, plain, search, typing};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    /// 同 Miyu `oobe/nav.rs` 的 `vim_keys_move_only_where_nothing_is_typed`。
    #[test]
    fn vim_keys_move_only_where_nothing_is_typed() {
        assert_eq!(plain(key(KeyCode::Char('j'))), Some(Nav::Down));
        assert_eq!(plain(key(KeyCode::Char('h'))), Some(Nav::Left));
        assert_eq!(typing(key(KeyCode::Char('j'))), None, "格子里 j 是字");
        assert_eq!(typing(ctrl('n')), Some(Nav::Down));
        assert_eq!(typing(key(KeyCode::Left)), None, "左右键在格子里挪光标");
        assert_eq!(search(ctrl('k')), Some(Nav::Up), "同 fzf");
        assert_eq!(search(key(KeyCode::Char('k'))), None, "搜索里 k 是字");
        assert_eq!(plain(ctrl('j')), None, "Ctrl+J 不是 j");
    }
}
