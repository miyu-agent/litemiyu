//! 找回来（蓝图 `tui.md`「输入框」第 7、8、13 条、「按键」`Ctrl+C`）：撤销放回的那句、恢复时收回去；照正文里的字
//! 在输入历史里找发出去时的样子（长文、附件还是块）；`Ctrl+C` 清掉的那一份什么时候不要了。

use super::{Draft, InputBox, Piece};

impl InputBox {
    /// 撤销成了：撤掉的那句放回输入框（框里空着才放），全选着，回车就重发；照输入历史里发出去时的样子放（「正文」第 6 条，
    /// Miyu 终端 `input/recall.rs`）。
    /// `images` 是那句话带的图片块（正文里记着的）：输入历史里找不到、或者找到的那一份图片文件已经不在了的（暂存的截图
    /// 上一次退出时删了），照它们把图片块接回来，指着会话里留着的那一份。
    pub fn put_back(&mut self, said: &str, images: &[Piece]) {
        if !self.editor.is_empty() {
            return;
        }
        let sent = self.history.iter().rev().find(|s| s.draft.expand() == said);
        match sent.map(|s| s.draft.clone()).filter(intact) {
            Some(draft) => self.editor.set_draft(draft),
            None if images.is_empty() => self.editor.set(said),
            None => self.editor.set_draft(Draft::from_pasted(said, images)),
        }
        self.editor.select_all();
        self.put_back = Some(said.to_string());
    }

    /// 恢复了：撤销时放回来的那句还没动过的，收回去，免得回车再发一遍（`tui.md`「正文」第 6 条）。
    pub fn take_back(&mut self) {
        if self
            .put_back
            .take()
            .is_some_and(|said| self.editor.draft().expand() == said)
        {
            self.editor.take();
        }
    }

    /// 框里是翻输入历史翻出来的那一句、还没改过：这时不弹命令列表、`@` 文件列表，`↑` `↓` 接着翻（蓝图「按键」`↑`、`↓`）。
    pub fn recalled(&self) -> bool {
        self.browsing
            .and_then(|at| self.history.get(at))
            .is_some_and(|sent| sent.draft == self.as_typed(&self.editor.draft()))
    }

    /// 发出去一句话了：`Ctrl+C` 清掉的那一份不要了，`↑` 照旧先翻到刚发的（「按键」`Ctrl+C`）。
    pub fn forget_cleared(&mut self) {
        self.cleared = None;
    }

    /// 编辑上一句：放进输入框，光标在最后，框上写「编辑上一条 · Esc 取消」（第 13 条，Miyu 终端 `input/recall.rs`）。
    pub fn start_edit(&mut self, draft: Draft) {
        self.goal_col = None;
        self.follow = true;
        self.editor.set_draft(draft.clone());
        self.editing = Some(draft);
    }

    /// 接着编辑：框里的字照旧，改之前的那句是 `original`（打错了命令、会话拒了，第 13 条）。
    pub fn resume_edit(&mut self, original: Draft) {
        self.editing = Some(original);
    }

    /// 不编辑了（`Esc`）：框里清空。
    pub fn cancel_edit(&mut self) {
        self.editing = None;
        self.editor.take();
    }

    /// 在编辑上一句。
    pub fn editing(&self) -> bool {
        self.editing.is_some()
    }

    /// 回车了：不再编辑，交回改之前的那句；没在编辑是 `None`。
    pub fn take_edit(&mut self) -> Option<Draft> {
        self.editing.take()
    }

    /// 输入历史里最近一句写出来是 `text` 的，发出去时的样子（被退回的排队消息，第 8 条）。
    pub fn sent_by_text(&self, text: &str) -> Option<Draft> {
        let sent = self.history.iter().rev().find(|s| s.draft.text == text);
        sent.map(|s| s.draft.clone()).filter(intact)
    }
}

/// 这一段里图片块的文件都还在：不在了的（暂存的截图删了、应急模式里只在内存里的）不照输入历史放，照正文里记着的拼。
fn intact(draft: &Draft) -> bool {
    draft.images().iter().all(|path| path.exists())
}
