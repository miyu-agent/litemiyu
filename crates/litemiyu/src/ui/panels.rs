//! 输入框上面那个框里画什么（`/help`）：照开着的是哪一个排成行，交回框（标题、提示）、行、每行是第几项（点中用）。
//! Miyu 这里还有后台面板、`/model`、`/sessions`、`/effort`、`/usage` 这些框（L5、L7、L8 跟着功能搬回）。

use ratatui::text::Line;

use super::help;
use super::panel::Chrome;
use crate::app::{Panel, Seen};

/// 开着的框排成的行，最多 `max` 行（框里上下的空行由 `panel.rs` 留）。这些框都认 `j`、`k`：按键模式是 Vim 时按键提示
/// 写「j/k」（蓝图「按键」表「按键模式」那一行）。
pub(super) fn content(
    seen: &Seen,
    width: u16,
    max: usize,
) -> (Chrome, Vec<Line<'static>>, Vec<Option<usize>>) {
    let (chrome, lines, map) = rows(seen, width, max);
    (chrome.vim(seen.config.layout.vim), lines, map)
}

/// 照开着的是哪一个排。
fn rows(seen: &Seen, width: u16, max: usize) -> (Chrome, Vec<Line<'static>>, Vec<Option<usize>>) {
    match seen.boxes.panel {
        Some(Panel::Help { scroll }) => {
            let (chrome, lines) = help::lines(seen.config, width, scroll, max);
            let map = vec![None; lines.len()];
            (chrome, lines, map)
        }
        Some(Panel::Jobs) => super::jobs_panel::lines(seen, width, max),
        Some(Panel::Models) => match &seen.boxes.model_list {
            Some(list) => super::model_list::lines(
                list,
                seen.talk.transcript.current_ref().as_deref(),
                seen.config,
                width,
                max,
            ),
            None => Default::default(),
        },
        Some(Panel::Effort { selected }) => super::effort_list::lines(
            seen.boxes.efforts.as_ref(),
            selected,
            seen.config,
            width,
            max,
        ),
        Some(Panel::Sessions) => match &seen.boxes.sessions {
            Some(list) => super::session_list::lines(
                list,
                seen.cwd,
                jiff::Timestamp::now(),
                seen.config,
                (width, max),
            ),
            None => Default::default(),
        },
        Some(Panel::Status { scroll }) => super::status_box::lines(seen, scroll, width, max),
        None => Default::default(),
    }
}
