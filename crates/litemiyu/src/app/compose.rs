//! 让出终端去开编辑器（Miyu 终端 `app/compose.rs`）：`Ctrl+G` 把输入框的字写进一份临时文件，主循环让出终端给编辑器
//! （`main.rs`、`terminal::edit`），退出以后读回来放进输入框，粘贴块照样子接回去；配置界面里改提示词（`prompt.md`）也走这里。

use std::io;

use super::App;
use crate::editor::{Edit, Then};
use crate::input::Draft;

impl App {
    /// 按了 `Ctrl+G`：写临时文件，交给主循环开编辑器。编辑器照 `$VISUAL`、`$EDITOR`，都没有的用 `vi`（`editor.rs`）。
    pub(super) fn compose(&mut self) {
        let before = self.typing.input.draft();
        // 写在自己的缓存目录里、只有自己读得到（`scratch.rs`，同 Miyu）。
        let file = match crate::scratch::write("prompt", &before.text) {
            Ok(file) => file,
            Err(error) => {
                let note = self
                    .config
                    .text()
                    .open
                    .failed
                    .replace("{reason}", &error.to_string());
                self.hint(note, false);
                return;
            }
        };
        self.edit = Some(Edit {
            editor: crate::editor::command(|name| std::env::var(name).ok()),
            file,
            then: Then::Compose(before),
        });
    }

    /// 编辑器退出了（`ran` 是它跑没跑起来）：`Ctrl+G` 开的读回来放进输入框（末尾多出来的那个换行去掉），临时文件删掉；
    /// 起不来的弹一句原因。
    pub fn edited(&mut self, edit: Edit, ran: io::Result<io::Result<()>>) {
        if let Err(error) = ran.and_then(|ran| ran) {
            let note = self
                .config
                .text()
                .open
                .failed
                .replace("{reason}", &error.to_string());
            self.hint(note, false);
        }
        let Then::Compose(before) = edit.then else {
            if let Some(settings) = &mut self.settings {
                settings.prompt_edited();
            }
            return;
        };
        if let Ok(text) = std::fs::read_to_string(&edit.file) {
            let text = text.strip_suffix('\n').unwrap_or(&text);
            self.typing
                .input
                .editor
                .set_draft(Draft::rebind(text, &before));
        }
        drop(std::fs::remove_file(&edit.file));
    }
}
