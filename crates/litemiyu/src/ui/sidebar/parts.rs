//! 侧边栏的几段（Miyu 终端 `ui/sidebar/parts.rs`，蓝图 `tui.md`「挂载位」第 4 条）：会话、工作目录、上下文、用量、循环、跳转消息、
//! 最近会话，照这个先后（Miyu `config/slots.rs` 的 `SIDE_PARTS`，去掉头像）。每段照宽度排出自己那几行；要点的（短编号、跳转
//! 消息、最近会话）画好以后照给它的那一块记点击区。

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};

use super::stack::Part;
use crate::app::{Screen, Seen};
use crate::theme;
use crate::transcript::Kind;

/// 侧边栏画的时候要用的。
pub struct Side<'s, 'a> {
    /// 这一帧看到的。
    pub seen: &'s Seen<'a>,
    /// 点击区、跳转消息、最近会话那几行记在这里。
    pub screen: &'s mut Screen,
}

/// `session`：会话名、短编号。点短编号复制完整编号。
#[derive(Default)]
pub struct Session {
    id_row: Option<usize>,
}

impl Part<Side<'_, '_>> for Session {
    fn lay(&mut self, side: &Side, width: u16) -> Vec<Line<'static>> {
        let t = &side.seen.talk.transcript;
        let (lines, id_row) = super::session_lines(t, side.seen.config, width);
        self.id_row = id_row;
        lines
    }

    fn placed(&mut self, area: Rect, (first, shown): (usize, usize), side: &mut Side) {
        // 短编号那一行露出来了才能点（往下滚过的照露出来的那一截算）。
        if let Some(row) = self
            .id_row
            .filter(|row| (first..first + shown).contains(row))
        {
            let y = area.y + u16::try_from(row - first).unwrap_or(0);
            side.screen.areas.session_id = Rect::new(area.x, y, area.width, 1);
        }
    }
}

/// `cwd`：工作目录照会话的：开的旧会话照它记下的，别的照终端所在的（同 `/status`、`@` 列表）。
pub struct Cwd;

impl Part<Side<'_, '_>> for Cwd {
    fn lay(&mut self, side: &Side, width: u16) -> Vec<Line<'static>> {
        let seen = side.seen;
        let opened = seen
            .talk
            .workdir
            .as_ref()
            .map(|dir| crate::local::home_short(&dir.to_string_lossy()));
        let cwd = opened.as_deref().unwrap_or(seen.cwd);
        super::cwd_lines(seen.config, width, cwd)
    }

    /// 第 0 行是标题，第 1 行起是路径：第 2 行起都是路径放不下折下来的（Miyu 同名的这一段）。
    fn continues(&self, row: usize) -> bool {
        row >= 2
    }
}

/// `context`：上下文。
pub struct Context;

impl Part<Side<'_, '_>> for Context {
    fn lay(&mut self, side: &Side, width: u16) -> Vec<Line<'static>> {
        super::context_lines(&side.seen.talk.transcript, side.seen.config, width)
    }
}

/// `usage`：用量，最后接花了多少。
pub struct Usage;

impl Part<Side<'_, '_>> for Usage {
    fn lay(&mut self, side: &Side, _: u16) -> Vec<Line<'static>> {
        let seen = side.seen;
        let total = seen.talk.usage_total();
        let mut lines = super::usage_lines(&seen.talk.transcript, &total, seen.config);
        lines.extend(super::spent_lines(seen.spent(), seen.config));
        lines
    }
}

/// `loops`：这个会话挂着的循环（「循环」第 3 条）。
pub struct Loops;

impl Part<Side<'_, '_>> for Loops {
    fn lay(&mut self, side: &Side, width: u16) -> Vec<Line<'static>> {
        let seen = side.seen;
        super::super::loop_tag::sidebar(
            seen.talk.looping.as_ref(),
            seen.config,
            (jiff::Timestamp::now(), crate::zone::local()),
            width,
        )
    }
}

/// `recent`：最近会话，一行一个；点一行切过去。
#[derive(Default)]
pub struct Recent {
    /// 标题下面每一行是哪个会话。
    ids: Vec<String>,
}

impl Part<Side<'_, '_>> for Recent {
    fn lay(&mut self, side: &Side, width: u16) -> Vec<Line<'static>> {
        let (lines, ids) = super::super::recent::lines(side.seen, width);
        self.ids = ids;
        lines
    }

    fn placed(&mut self, area: Rect, (first, shown): (usize, usize), side: &mut Side) {
        // 标题那一行（第 0 行）不算：从第一个露出来的会话起一行一个（往下滚过、标题滚出去的从第 `first` 行起）。
        let start = first.max(1);
        let rows = (first + shown).saturating_sub(start);
        side.screen.areas.recent = Rect {
            y: area.y + u16::try_from(start - first).unwrap_or(0),
            height: u16::try_from(rows).unwrap_or(0),
            ..area
        };
        side.screen.recent = self
            .ids
            .iter()
            .skip(start - 1)
            .take(rows)
            .cloned()
            .collect();
    }
}

/// `jump`：跳转消息（Miyu 排期第 15 条）：这个会话里你说的话（排着队的也算，同 Miyu），列最近的 `sidebar_jumps` 条，旧的
/// 在上、新的在下（同正文），一条一行、只写第一行，截掉放不下的；当前这一句打头写箭头（Miyu `cbb5dc0e`）；点一行同 `/jump`
/// 选它（`app/jump.rs`）。还没说过话的不画这一段。
#[derive(Default)]
pub struct Jumps {
    ids: Vec<u64>,
}

impl Part<Side<'_, '_>> for Jumps {
    fn lay(&mut self, side: &Side, width: u16) -> Vec<Line<'static>> {
        let seen = side.seen;
        let count = seen.config.layout.sidebar_jumps;
        let mut spoken: Vec<&crate::transcript::Entry> = seen
            .talk
            .transcript
            .entries
            .iter()
            .rev()
            .filter(|e| e.kind == Kind::User && !e.hidden)
            .take(count)
            .collect();
        // 取的是最近的几条，排成旧的在上、新的在下（同正文）。
        spoken.reverse();
        self.ids = spoken.iter().map(|e| e.id).collect();
        if spoken.is_empty() {
            return Vec::new();
        }
        let room = width.saturating_sub(2);
        let mut lines = vec![Line::styled(
            seen.config.front.side_jumps.clone(),
            theme::side_title(),
        )];
        // 当前这一句的箭头照主题的 `pointer`（两格宽），别的行空同样宽。
        let pointer = crate::sheet::mark(|m| &m.pointer);
        let blank = " ".repeat(unicode_width::UnicodeWidthStr::width(pointer.as_str()));
        let current = current_said(side.seen, side.screen);
        lines.extend(spoken.iter().map(|e| {
            let said = e.listed();
            let first = said
                .lines()
                .find(|l| !l.trim().is_empty())
                .unwrap_or_default();
            // 当前这一句打头写箭头（Miyu 排期第 44 条，2026-11-04 项目主人：「箭头。不要用圆点。」）。
            let lead = if current == Some(e.id) {
                Span::styled(pointer.clone(), theme::accent())
            } else {
                Span::raw(blank.clone())
            };
            Line::from(vec![
                lead,
                Span::styled(super::super::rows::clip(first.trim(), room), theme::dim()),
            ])
        }));
        lines
    }

    fn placed(&mut self, area: Rect, (first, shown): (usize, usize), side: &mut Side) {
        // 标题那一行（第 0 行）不算，同最近会话。
        let start = first.max(1);
        let rows = (first + shown).saturating_sub(start);
        side.screen.areas.jumps = Rect {
            y: area.y + u16::try_from(start - first).unwrap_or(0),
            height: u16::try_from(rows).unwrap_or(0),
            ..area
        };
        side.screen.jumps = self
            .ids
            .iter()
            .skip(start - 1)
            .take(rows)
            .copied()
            .collect();
    }
}

/// 当前这一句（Miyu `ui/pinned.rs` 的 `current`，排期第 44 条）：跟着最新的是最后一句你说的话；往上翻着的是视口第一行那一条
/// 往前最近的一句。正文这一帧先画（`ui/body.rs`），照它排好的行和视口算，不另排。正文没你说的话的是 `None`。
fn current_said(seen: &Seen, screen: &Screen) -> Option<u64> {
    let entries = &seen.talk.transcript.entries;
    let said =
        |i: &usize| entries[*i].kind == Kind::User && !entries[*i].hidden && !entries[*i].queued;
    let end = entries.len().checked_sub(1)?;
    let last = match screen.view.top {
        None => end,
        Some(_) => {
            let id = screen.view.rows.entry_at(screen.view.first)?;
            entries.iter().position(|e| e.id == id)?.min(end)
        }
    };
    let k = (0..=last).rev().find(said)?;
    Some(entries[k].id)
}

/// 这几段，照出厂的先后。
pub fn all() -> (Session, Cwd, Context, Usage, Loops, Jumps, Recent) {
    (
        Session::default(),
        Cwd,
        Context,
        Usage,
        Loops,
        Jumps::default(),
        Recent::default(),
    )
}
