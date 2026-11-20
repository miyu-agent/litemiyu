//! 终端光标（蓝图 `tui.md`「每一帧」）：界面自己管，不交给 ratatui。显示着的这一帧由 `frame.set_cursor_position`
//! 交给 ratatui，画完它挪过去、显示；藏着的光标由 `park` 挪到插入点。
//!
//! 不每帧先藏再显：输入法的预编辑挂在光标上，跟着藏/显会和输入框里的提示来回闪（2026-10-02 项目主人报的 fcitx5：
//! 打字时提示和候选来回切；原来 ratatui 每帧 `Hide`、画完再 `Show`）。
//!
//! 藏着时也挪到同一个地方（输入框的插入点）：不挪的话光标停在这一帧最后写的格子，转轮、token 数、吉祥物轮流变，
//! 藏着的光标跟着跳，开了拖尾的终端（kitty 的 `cursor_trail`）照样给它画拖尾。

use ratatui::layout::Position;

/// 这一帧的光标。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Caret {
    /// 停在哪一格：输入框的插入点，抽屉里有搜索框时在搜索框里。输入框没画出来就留着上一帧的。
    pub at: Position,
    /// 显不显示：焦点在输入框、或者抽屉的搜索框在等字。
    pub shown: bool,
}

impl Caret {
    /// 新的一帧：先当不显示，停的地方留着上一帧的，画到输入框、抽屉时再改。
    pub fn begin(&mut self) {
        self.shown = false;
    }

    /// 停在 `at`，`shown` 说显不显示。
    pub fn put(&mut self, at: Position, shown: bool) {
        self.at = at;
        self.shown = shown;
    }
}

#[cfg(test)]
mod tests {
    use ratatui::layout::Position;

    use super::Caret;

    #[test]
    fn a_new_frame_keeps_the_place_but_hides() {
        let mut caret = Caret::default();
        caret.put(Position::new(7, 9), true);
        caret.begin();
        assert_eq!(
            caret,
            Caret {
                at: Position::new(7, 9),
                shown: false
            }
        );
    }
}
