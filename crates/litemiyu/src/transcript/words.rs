//! 旁白里的几句话怎么拼：收尾行后面的本轮用量，撤销点开以后停了几个任务。字在配置里，这里只管拼。

use lm_kernel::kernel::session::Undid;

use crate::config::{Texts, UndoTexts};
use crate::core::Usage;

/// 撤销点开以后最后那一行：`已停止 1 个任务`。没有的是 `None`（Miyu 终端 `transcript/words.rs::undo_counts`）。
pub fn undo_counts(undid: &Undid, texts: &UndoTexts) -> Option<String> {
    (undid.jobs > 0).then(|| {
        texts
            .stopped_jobs
            .replace("{count}", &undid.jobs.to_string())
    })
}

/// 收尾行后面那一截：本轮用量（输入加输出）和命中率（命中 ÷ 输入）；这一轮一次模型都没调成的是空的
/// （`tui.md`「正文」第 4 条）。
pub(super) fn turn_usage(usage: &Usage, texts: &Texts) -> String {
    let input = usage.input();
    if input == 0 {
        return String::new();
    }
    texts
        .done_usage
        .replace("{tokens}", &crate::meter::short(input + usage.output))
        .replace(
            "{percent}",
            &crate::meter::hit_rate(usage.cache_read, input),
        )
}
