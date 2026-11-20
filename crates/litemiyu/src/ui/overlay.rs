//! 输入框上面那一块的覆盖层（蓝图「斜杠命令列表」第 1、3 条）：命令列表、输入历史列表、面板（`/help`）占同一个地方，
//! 开哪个照 `app/frame.rs`；盖在上面那一块（`above.rs`）、运行状态行上面（后画）。

use crate::ui::Frame;
use ratatui::text::Line;

use super::panel::Chrome;
use super::{Areas, history, menu, panel};
use crate::app::{App, Lists};

/// 画开着的那一个：`history`、`panel` 是先排好的行（占几行由它们定，`ui::draw`）。
pub(super) fn draw(
    frame: &mut Frame,
    app: &App,
    areas: Areas,
    lists: &Lists,
    history_laid: history::Laid,
    (panel_chrome, panel_lines): (Chrome, Vec<Line<'static>>),
) {
    if let Some(matches) = &lists.matches {
        let rows = menu::rows(app.config.layout.menu_rows, areas.menu_text.height);
        let (chrome, lines) = menu::lines(
            matches,
            app.typing.menu.selected,
            app.typing.menu.pinned,
            rows,
            areas.menu_text.width,
            &app.config.text().menu,
        );
        menu::draw(frame, areas.menu, areas.menu_text, chrome, lines);
    }
    if app.typing.history.open {
        history::draw(frame, areas.menu, areas.menu_text, history_laid);
    }
    if app.boxes.panel.is_some() {
        panel::draw(
            frame,
            areas.menu,
            areas.menu_text,
            panel_chrome,
            panel_lines,
        );
    }
}
