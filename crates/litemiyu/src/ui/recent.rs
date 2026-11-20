//! 侧边栏「最近会话」（Miyu 终端 `ui/recent.rs`，蓝图 `tui.md`「后台命令、子代理和侧边栏」第 7 条）：照会话列表的先后列
//! （最近有动静的在前，连这个窗口开着的这一个），当前这个在原位（Miyu `848a8a1a`，2026-10-27 项目主人：切过去就顶到最上面，
//! 难找）。行首一列记号挂在缩进那两格里（Miyu `291aacda`）：有一轮在跑的转圈，当前这个强调色箭头；名字当前这个照常、别的暗。
//! 一共几个照 `sidebar_sessions`。
//!
//! 和 Miyu 不一样的：没有「等人确认」的黄色 `?`（Miyu `6725ac81`）、「做完了还没看」的圆点（Miyu `37a7e548`）——LiteMiyu
//! 切走就停下原来那个会话，没有在后台跑着的会话，别的窗口的会话等不等人列表里也看不到；没标题的写「未命名会话」，不照
//! 预览写（会话的小概要里没有预览）。

use lm_engine::session::Listed;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use crate::app::Seen;
use crate::theme;

/// 一行：会话编号、名字、记号的样子。
struct Row<'a> {
    id: &'a str,
    name: &'a str,
    status: Status,
}

/// 一个会话的几样状态，挑行首记号用（[`mark`]）。
#[derive(Debug, Clone, Copy, Default)]
struct Status {
    /// 当前这个。
    current: bool,
    /// 有一轮在跑。
    busy: bool,
}

/// 「最近会话」那一段：标题加一行行，一行是两格记号接名字，一并交回标题下面每一行是哪个会话。还没列过、一个都没有、
/// `sidebar_sessions` 是 0 的是空的。
pub fn lines(seen: &Seen, width: u16) -> (Vec<Line<'static>>, Vec<String>) {
    let count = seen.config.layout.sidebar_sessions;
    let Some(listed) = seen.talk.recent.listed.as_ref() else {
        return (Vec::new(), Vec::new());
    };
    if count == 0 {
        return (Vec::new(), Vec::new());
    }
    let texts = &seen.config.text().sessions;
    // 当前这个照主会话（切进了子会话的是停放着的那一份，子会话不列；Miyu `3ee2a978` 的 `listed_current`）。
    let transcript = seen.talk.main_transcript();
    let me = transcript.session.as_deref();
    // 当前这个忙不忙照正文（列表是列的那一刻的，一轮里不再列）。
    let running = transcript.running.is_some();
    let mut rows: Vec<Row> = listed
        .iter()
        .map(|s: &Listed| {
            let current = Some(s.id.as_str()) == me;
            Row {
                id: s.id.as_str(),
                name: super::session_list::title(s, texts),
                status: Status {
                    current,
                    busy: if current { running } else { s.busy },
                },
            }
        })
        .collect();
    if let Some(id) = me {
        let title = transcript.title.as_deref();
        match rows.iter().position(|r| r.status.current) {
            // 正文里有标题的照它（列表里的可能还是旧的）。
            Some(at) => rows[at].name = title.unwrap_or(rows[at].name),
            // 还没进列表（刚开的）：它最新，排第一行。
            None => rows.insert(
                0,
                Row {
                    id,
                    name: title.unwrap_or(&texts.untitled),
                    status: Status {
                        current: true,
                        busy: running,
                    },
                },
            ),
        }
    }
    // 当前这个排在名额以外的：前面的少列一个，它接在最后（先后不乱，自己也看得见）。
    if let Some(at) = rows.iter().position(|r| r.status.current)
        && at >= count
    {
        let mine = rows.remove(at);
        rows.truncate(count - 1);
        rows.push(mine);
    }
    rows.truncate(count);
    if rows.is_empty() {
        return (Vec::new(), Vec::new());
    }
    let mut lines = vec![Line::styled(texts.side_recent.clone(), theme::side_title())];
    let mut ids = Vec::new();
    // 记号两格（挂在缩进里）。
    let room = width.saturating_sub(2);
    let frame = super::spin_frame(seen);
    for row in rows {
        let (mark, mark_style) = mark(&row.status, frame, seen);
        let style = if row.status.current {
            Style::new()
        } else {
            theme::dim()
        };
        let pad = 2usize.saturating_sub(mark.width());
        lines.push(Line::from(vec![
            Span::styled(format!("{mark}{}", " ".repeat(pad)), mark_style),
            Span::styled(super::rows::clip(row.name, room), style),
        ]));
        ids.push(row.id.to_string());
    }
    (lines, ids)
}

/// 行首的记号（Miyu `mark`，裁掉等人的 `?`、做完了还没看的圆点）：在跑的转圈（当前这个强调色、别的暗），当前这个强调色
/// 箭头，别的空着。
fn mark(row: &Status, frame: usize, seen: &Seen) -> (String, Style) {
    let spinner = &seen.config.timeline.spinner;
    let lead = if row.current {
        theme::accent()
    } else {
        theme::dim()
    };
    if row.busy {
        (spinner[frame % spinner.len().max(1)].clone(), lead)
    } else if row.current {
        (seen.config.layout.side_here_mark.clone(), lead)
    } else {
        (String::new(), lead)
    }
}

#[cfg(test)]
mod tests;
