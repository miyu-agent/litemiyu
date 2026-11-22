//! 撤销、恢复、重做（`/undo`、`/restore`，内核的 `Revert`、`Unrevert`；`/redo`、`/edit`，内核的 `Redo`）：接受了交 [`Update::Undone`]，被拒的照原因码报。撤掉、改回的
//! 样子照会话推来的事件画（`core/push.rs`），这里回一声成了，带着撤掉了几次压缩、清空、停了几个任务：界面照它放回、收回那一句，
//! 说明里多写那几行（Miyu 收 `session.revert` 回应那一段）。

use lm_kernel::kernel::block::{Block, Text};
use lm_kernel::kernel::session::{Command as Kernel, Outcome, Undid};

use super::Engine;
use crate::core::Update;

impl Engine {
    /// 撤回上一轮（`restore` 为假），或者恢复刚撤销的。
    pub(super) async fn undo(&mut self, restore: bool) {
        let command = if restore {
            Kernel::Unrevert
        } else {
            Kernel::Revert { turn: None }
        };
        match self.ask(command).await {
            Some(Outcome::Accepted { .. }) => self.report(Update::Undone {
                restore,
                undid: Undid::default(),
            }),
            // 撤掉了压缩、清空，停了任务：几个数照内核的回应交给界面（Miyu 核心撤销回应的 `compactions`、`clears`、`jobs`）。
            Some(Outcome::Reverted { undid, .. }) => {
                self.report(Update::Undone { restore, undid });
            }
            outcome => self.refused(outcome),
        }
    }

    /// 重做最后一轮：`text` 是改过的那句，`None` 原样重来；`images` 是改过的那句附的图片（LM-28），`None` 照原来的。同说
    /// 一句话：被拒的照「没发出去」报。
    pub(super) async fn redo(
        &mut self,
        text: Option<String>,
        images: Option<Vec<lm_engine::session::Attach>>,
    ) {
        let text = text.map(|text| vec![Block::Text(Text { text })]);
        self.send(images, |attachments| Kernel::Redo { text, attachments })
            .await;
    }
}
