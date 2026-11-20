//! 正文顶上钉着的那一行（Miyu 终端 `ui/pinned.rs`，蓝图 `tui.md`「跳到说过的话」第 5 条、「正文」第 1 条的子条；2026-11-20
//! 项目主人：「向上滚动，上方能看到最近一次的prompt」）：往上翻过（没跟着最新的）、视口最上面那一段对应的你说的话已经整句
//! 滚出去了，顶上盖一行写那句话（贴左边的竖线，竖线和字都是暗色，铺浅一档的底），下面再空一行，正文从第三行露起；点一下
//! 跳回那句话（同 `/jump`）。跟着最新的、那句话还露着的、正文区不到 4 行高的不画。
//!
//! 正文只留几屏（`app/window.rs`）：一段从你说的一句话起，放掉、读回都是整段，所以视口第一行落在常驻的条目上时，它那一段
//! 开头的那句话也常驻，照常驻的条目往前找就是它。视口第一行落在放掉的段上（空档，还没读回）的这一帧不钉；找到的那句话和
//! 视口第一行之间隔着放掉的段的（那一段开头的话被撤掉了这类）也不钉，免得钉错一句。
//!
//! 和 Miyu 不一样的：LiteMiyu 没有挂载位的配置（Miyu `[[tui.top]]` 能把它藏起来），一直画。

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use super::Frame;
use super::row_cache::Rows;
use crate::app::{Screen, Seen};
use crate::theme;
use crate::transcript::{Kind, Transcript};

/// 钉着的那一行连它下面空的一行一共盖住正文最上面几行（Miyu `343bb02e`，2026-11-11 Miyu 项目主人：「这里应该空一行，也就是
/// 从第三行开始」）。长回答停在开头时也让出这么多行（`body.rs` 的 `first_row`）。
pub(super) const ROWS: usize = 2;

/// 正文区至少这么高才钉。
const MIN_HEIGHT: u16 = 4;

/// 画在正文区 `area` 的第一行上，下面再空一行；`first` 是这一帧露出来的第一行。
pub(super) fn draw(
    frame: &mut Frame,
    area: Rect,
    (rows, first): (&Rows, usize),
    seen: &Seen,
    screen: &mut Screen,
) {
    screen.areas.pinned = Rect::default();
    screen.pinned = None;
    if screen.view.top.is_none() || area.height < MIN_HEIGHT {
        return;
    }
    let t = &seen.talk.transcript;
    let Some(k) = said_above(t, rows, first) else {
        return;
    };
    let entry = &t.entries[k];
    let row = Rect::new(area.x, area.y, area.width, 1);
    // 先把这一行清空再写：只盖字的话，句子后面露出底下正文那一行剩下的字（Miyu `41457865`）。
    frame.render_widget(ratatui::widgets::Clear, row);
    frame.buffer_mut().set_style(row, theme::panel());
    let bar = &seen.config.layout.user_bar;
    let lead = format!("{} ", bar.trim_end());
    let room = area
        .width
        .saturating_sub(u16::try_from(lead.width()).unwrap_or(0));
    let said = entry.listed();
    let said = said
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or_default();
    let line = Line::from(vec![
        Span::styled(lead, theme::dim()),
        Span::styled(super::rows::clip(said.trim(), room), theme::dim()),
    ]);
    frame
        .buffer_mut()
        .set_line(area.x, area.y, &line, area.width);
    // 下面空一行（照正文的底色），正文从第三行露起。
    let gap = Rect::new(area.x, area.y + 1, area.width, 1);
    frame.render_widget(ratatui::widgets::Clear, gap);
    screen.areas.pinned = row;
    screen.pinned = Some(entry.id);
}

/// 视口第一行（第 `first` 行）那一条往前最近的一句你说的话（别处来的算、撤掉的不算）在 `entries` 里第几条：它整句都滚出
/// 视口上面了才交回。视口第一行落在放掉的段上、那句话还有一行露着、中间隔着放掉的段的是 `None`。
fn said_above(t: &Transcript, rows: &Rows, first: usize) -> Option<usize> {
    if rows.gap_at(first).is_some() {
        return None;
    }
    let id = rows.entry_at(first)?;
    let at = t.entries.iter().position(|e| e.id == id)?;
    let k = (0..=at)
        .rev()
        .find(|&i| t.entries[i].kind == Kind::User && !t.entries[i].hidden)?;
    // 那句话还露着（还有一行在视口里）：不钉，免得同一句写两遍。
    if rows.end_of(t.entries[k].id).is_none_or(|end| end > first) {
        return None;
    }
    let (from, to) = (t.entries[k].piece as usize, t.entries[at].piece as usize);
    (from..=to).all(|p| t.pieces().resident(p)).then_some(k)
}
