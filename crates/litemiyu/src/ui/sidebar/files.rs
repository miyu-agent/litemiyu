//! 侧边栏「文件修改」（Miyu 终端 `ui/sidebar/files.rs`，蓝图 `tui.md`「文件修改」第 1 条；2026-11-20 项目主人）：这个会话改过
//! 的文件，一个一行（文件名、暗色的目录，后面暗色 `+3 -1`），先改的在前，最多列 `sidebar_files` 个，多的写「还有 N 个」；没改过
//! 文件的不出这一段。点一行开「文件修改」那一页、选上这个文件（`app/files_page.rs`）。
//!
//! 数照正文记的小账（`transcript/changed.rs`），不照常驻的步骤现数：正文只留几屏，放掉的段里的改动照样列着。

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use super::parts::Side;
use super::stack::Part;
use crate::theme;
use crate::transcript::Changed;

/// `files`：改过的文件，一个一行；点一行开那一页。
#[derive(Default)]
pub struct Files {
    /// 标题下面列了几个文件（不算「还有 N 个」那一行）。
    listed: usize,
    /// 标题下面一共几行（连「还有 N 个」那一行）。
    rows: usize,
}

impl Part<Side<'_, '_>> for Files {
    fn lay(&mut self, side: &Side, width: u16) -> Vec<Line<'static>> {
        let seen = side.seen;
        let count = seen.config.layout.sidebar_files;
        (self.listed, self.rows) = (0, 0);
        if count == 0 {
            return Vec::new();
        }
        let files = seen.talk.transcript.changed();
        if files.is_empty() {
            return Vec::new();
        }
        let words = &seen.config.text().files;
        let mut lines = vec![Line::styled(words.title.clone(), theme::side_title())];
        lines.extend(files.iter().take(count).map(|file| row(file, width)));
        self.listed = files.len().min(count);
        if files.len() > count {
            let more = words
                .more
                .replace("{n}", &(files.len() - count).to_string());
            lines.push(Line::styled(format!("  {more}"), theme::dim()));
        }
        self.rows = lines.len() - 1;
        lines
    }

    fn placed(&mut self, area: Rect, (first, shown): (usize, usize), side: &mut Side) {
        // 标题那一行（第 0 行）不算，同跳转消息；「还有 N 个」那一行点了开那一页、选上第一个没列出来的。
        let start = first.max(1);
        let rows = (first + shown).saturating_sub(start).min(self.rows);
        side.screen.areas.files = Rect {
            y: area.y + u16::try_from(start - first).unwrap_or(0),
            height: u16::try_from(rows).unwrap_or(0),
            ..area
        };
        side.screen.files = (start - 1..start - 1 + rows).collect();
    }
}

/// 一行：缩进两格、文件名（正文色），后面暗色的目录（家目录写 `~`，放不下先截目录），再靠右暗色 `+a -r`。
pub fn row(file: &Changed, width: u16) -> Line<'static> {
    let counts = format!("+{} -{}", file.added, file.removed);
    let room = usize::from(width).saturating_sub(2 + 1 + counts.width());
    let (dir, name) = split(&crate::local::home_short(file.path));
    let name = super::super::rows::clip(&name, u16::try_from(room).unwrap_or(u16::MAX));
    let left = room.saturating_sub(name.width() + 1);
    let mut spans = vec![Span::raw("  "), Span::raw(name)];
    if left > 1 && !dir.is_empty() {
        let dir = super::super::rows::clip(&dir, u16::try_from(left).unwrap_or(u16::MAX));
        spans.push(Span::styled(format!(" {dir}"), theme::faint()));
    }
    let used: usize = spans.iter().map(Span::width).sum();
    let pad = usize::from(width)
        .saturating_sub(used + counts.width())
        .max(1);
    spans.push(Span::raw(" ".repeat(pad)));
    spans.push(Span::styled(counts, theme::dim()));
    Line::from(spans)
}

/// 路径拆成目录（带结尾的 `/`，没有的是空的）和文件名。
fn split(path: &str) -> (String, String) {
    match path.rfind('/') {
        Some(at) if at + 1 < path.len() => (path[..=at].to_string(), path[at + 1..].to_string()),
        _ => (String::new(), path.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::{row, split};
    use crate::transcript::Changed;

    #[test]
    fn a_row_is_the_name_the_dim_directory_and_the_counts_at_the_right() {
        let file = Changed {
            path: "/w/src/ui/sidebar.rs",
            added: 12,
            removed: 3,
        };
        let line = row(&file, 36);
        let text = line.to_string();
        assert_eq!(unicode_width::UnicodeWidthStr::width(text.as_str()), 36);
        assert!(text.starts_with("  sidebar.rs /w/src/ui/"), "{text:?}");
        assert!(text.ends_with(" +12 -3"), "{text:?}");
        // 放不下：先截目录，文件名留着。
        let narrow = row(&file, 24).to_string();
        assert!(narrow.starts_with("  sidebar.rs "), "{narrow:?}");
        assert!(narrow.ends_with("+12 -3"), "{narrow:?}");
        assert!(unicode_width::UnicodeWidthStr::width(narrow.as_str()) <= 24);
    }

    #[test]
    fn a_path_splits_into_its_directory_and_name() {
        assert_eq!(split("a/b/c.rs"), ("a/b/".into(), "c.rs".into()));
        assert_eq!(split("c.rs"), (String::new(), "c.rs".into()));
        assert_eq!(split("a/b/"), (String::new(), "a/b/".into()));
    }
}
