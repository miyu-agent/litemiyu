//! 画一帧以前先走的一步（蓝图 `app.md` 第三节）：要改状态的都在这里做完，画面层只照结果画。输入框从有字变空时换一条
//! 提示，同步斜杠命令列表的选中。

use super::App;
use crate::commands::Spec;

/// 这一帧输入框上面的列表。
pub struct Lists {
    /// 斜杠命令列表对得上的几条；不开的是 `None`。
    pub matches: Option<Vec<Spec>>,
}

impl App {
    /// 画对话那一帧以前：换提示，算好斜杠命令列表（历史列表、面板、抽屉开着时不开：几个占同一个地方，
    /// `tui.md`「输入历史列表」第 1 条）。
    pub fn before_frame(&mut self) -> Lists {
        self.typing.tips.see(
            self.typing.input.editor.is_empty(),
            self.config.front.tips.len(),
        );
        let blocked =
            self.typing.history.open || self.boxes.panel.is_some() || self.talk.drawers.open();
        let matches = if blocked { None } else { self.menu_matches() };
        Lists { matches }
    }
}
