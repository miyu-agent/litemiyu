//! 侧边栏（Miyu 终端 `ui/sidebar.rs`，蓝图 `tui.md`「后台命令、子代理和侧边栏」第 7 条，`docs/00-设计.md` 第五节第 12 条）：
//! 不在首页、窗口不少于 `sidebar_from` 列时右边一栏，和主列之间一根暗色竖线，从顶到底；窄了收起，这里的代码不跑。
//!
//! 几段照先后（`parts.rs`）：会话（名字、短编号）、工作区、上下文、用量、跳转消息、文件修改（`files.rs`）、最近会话
//! （`recent.rs`）。一段一个
//! 小标题（蓝色、不加粗），内容缩进两格一律暗色，段和段之间空一行（`stack.rs`）；比侧边栏高的滚轮滚它（`app/mouse.rs`），
//! 指针停在侧边栏上时最右边那一列画滚动条。数都是底栏、`/status` 用的那一份（`Transcript` 的上下文、窗口、压缩线、压缩和
//! 断裂次数，`Talk::usage_total`、`Seen::spent`），不另算、不另要。
//!
//! 侧边栏的字能拖选（`side_select.rs`）：画的时候把写了字的行记下来，选中的那几格反显（同 Miyu）。
//!
//! 和 Miyu 不一样的：头像、吉祥物、人格和预设那两行、扩展包的界面块、挂载位的配置，LiteMiyu 没有这些，不搬；
//! 子代理在输入框下面的子代理状态行（`agents.rs`），侧边栏没有子代理那一段（Miyu 也没有）。Miyu 的「循环」一段不要
//! （2026-11-20 项目主人：「移除右侧边栏的循环部分」）：循环只在输入框上面那一行写下一次几点（`loop_tag.rs`）。

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::app::{Screen, Seen};
use crate::config::{Config, Layout, texts};
use crate::core::Usage;
use crate::side_select::TextRow;
use crate::transcript::Transcript;
use crate::ui::Frame;
use crate::{meter, theme};

pub mod files;
mod parts;
mod stack;

texts! {
    /// 侧边栏会话那几段的字（Miyu `resources/text/*.json` 的 `side_*`）。
    #[derive(Debug, Clone)]
    pub struct Texts {
        /// 短编号，`{id}`。
        pub id: String,
        /// 工作区那一段的小标题。
        pub cwd: String,
        /// 上下文那一段的小标题。
        pub context: String,
        /// 上下文用了多少，`{used}`、`{window}`、`{percent}`。
        pub context_value: String,
        /// 压缩过几次，`{n}`。
        pub compactions: String,
        /// 用量那一段的小标题。
        pub usage: String,
        /// 共多少，`{tokens}`。
        pub total: String,
        /// 输入、输出，`{input}`、`{output}`。
        pub split: String,
        /// 缓存命中率，`{percent}`。
        pub hit: String,
        /// 缓存断了几次，`{n}`。
        pub breaks: String,
        /// 花了多少，`{amounts}`。
        pub cost: String,
    }
}

/// 侧边栏滚轮一格滚几行（同正文的 `body_view::WHEEL`）。
pub const WHEEL: u16 = 3;

/// 分出主列和侧边栏（侧边栏不含竖线）；窗口窄时侧边栏宽是 0。
pub fn split(area: Rect, layout: &Layout) -> (Rect, Rect) {
    if area.width < layout.sidebar_from {
        return (area, Rect::new(area.right(), area.y, 0, area.height));
    }
    let side = layout.sidebar_width;
    let main = Rect::new(area.x, area.y, area.width - side - 1, area.height);
    let sidebar = Rect::new(area.right() - side, area.y, side, area.height);
    (main, sidebar)
}

/// 侧边栏那几段的内容缩进两格（`tui.md`「后台命令、子代理和侧边栏」第 7 条）。
fn item(s: String, style: Style) -> Line<'static> {
    Line::styled(format!("  {s}"), style)
}

/// `session`：会话名当标题（没起名字的也一样蓝），下面短编号（「新会话：人格、工作区」第 3 条）。一并交回短编号在第几行
/// （点它复制完整编号）。
pub(super) fn session_lines(
    t: &Transcript,
    config: &Config,
    width: u16,
) -> (Vec<Line<'static>>, Option<usize>) {
    let text = config.text();
    let name = t.title.as_deref().unwrap_or(&text.sessions.untitled);
    let mut out = vec![Line::styled(
        super::rows::clip(name, width),
        theme::side_title(),
    )];
    let mut id_row = None;
    if let Some(session) = &t.session {
        // 短编号：最后 8 个字符（同 Miyu 核心 C-1，会话列表、正文里写的都是这个）。
        let short = crate::session_id::short(session);
        id_row = Some(out.len());
        out.push(item(text.side.id.replace("{id}", &short), theme::dim()));
    }
    (out, id_row)
}

/// `cwd`：工作目录，照斜杠折行。
pub(super) fn cwd_lines(config: &Config, width: u16, cwd: &str) -> Vec<Line<'static>> {
    let room = usize::from(width).saturating_sub(2).max(1);
    let mut out = vec![Line::styled(
        config.text().side.cwd.clone(),
        theme::side_title(),
    )];
    out.extend(
        wrap_path(cwd, room)
            .into_iter()
            .map(|l| item(l, theme::dim())),
    );
    out
}

/// `context`：上下文用了多少、进度条、压缩过几次；还是 0 的不写。
pub(super) fn context_lines(t: &Transcript, config: &Config, width: u16) -> Vec<Line<'static>> {
    if t.context == 0 {
        return Vec::new();
    }
    let text = &config.text().side;
    let room = usize::from(width).saturating_sub(2).max(1);
    let used = meter::short(t.context);
    let mut out = vec![Line::styled(text.context.clone(), theme::side_title())];
    // 窗口照引擎给的（`Handle::limits`，同底栏、`/status`），界面不自己照模型名查。
    match t.limits.window {
        Some(window) => {
            let value = text
                .context_value
                .replace("{used}", &used)
                .replace("{window}", &meter::short(window))
                .replace("{percent}", &meter::percent_tenths(t.context, window));
            out.push(item(value, theme::dim()));
            let marks = &config.layout.bar;
            let line = t.limits.compaction_line;
            out.push(bar(t.context, window, line, marks.width.min(room), marks));
        }
        None => out.push(item(used, theme::dim())),
    }
    if t.cache.compactions > 0 {
        let compactions = text
            .compactions
            .replace("{n}", &t.cache.compactions.to_string());
        out.push(item(compactions, theme::dim()));
    }
    out
}

/// `usage`：用量连同子代理用的（`Talk::usage_total`，2026-09-30 项目主人）；还是 0 的不写。
pub(super) fn usage_lines(t: &Transcript, total: &Usage, config: &Config) -> Vec<Line<'static>> {
    let text = &config.text().side;
    let input = total.input();
    if input + total.output + total.aux == 0 {
        return Vec::new();
    }
    let mut out = vec![Line::styled(text.usage.clone(), theme::side_title())];
    // 共多少连同辅助请求；输入输出、命中率只照主对话（蓝图「回顾」第 5 条）。
    let sum = meter::short(input + total.output + total.aux);
    out.push(item(text.total.replace("{tokens}", &sum), theme::dim()));
    let split = text
        .split
        .replace("{input}", &meter::short(input))
        .replace("{output}", &meter::short(total.output));
    out.push(item(split, theme::dim()));
    let hit = meter::hit_rate(total.cache_read, input);
    out.push(item(text.hit.replace("{percent}", &hit), theme::dim()));
    if t.cache.breaks > 0 {
        let breaks = text.breaks.replace("{n}", &t.cache.breaks.to_string());
        out.push(item(breaks, theme::dim()));
    }
    out
}

/// 花了多少：「花费 $0.42 + ¥1.30」；一笔都算不出的不写。没有价格的请求不提（2026-10-10 项目主人：「没有费用记录的请求
/// 不需要出现在侧边栏中」）。
pub(super) fn spent_lines(amounts: Option<String>, config: &Config) -> Vec<Line<'static>> {
    amounts
        .map(|amounts| {
            let line = config.text().side.cost.replace("{amounts}", &amounts);
            Line::styled(format!("  {line}"), theme::dim())
        })
        .into_iter()
        .collect()
}

/// 上下文那根进度条：用了的强调色、没用的暗；压缩线落在的那一格换成警示色（黄），字不变（2026-09-30 项目主人定）。
/// `line` 是压缩线，没有的不标。和 `/status` 的那一根同一份（`status_box::bar`），这里前面缩两格。
pub(super) fn bar(
    used: u64,
    window: u64,
    line: Option<u64>,
    room: usize,
    marks: &crate::config::Bar,
) -> Line<'static> {
    let mut spans = vec![Span::raw("  ")];
    spans.extend(super::status_box::bar(used, window, line, room, marks));
    Line::from(spans)
}

/// 路径折行：一行尽量多放，断在这一行最后一个 `/` 后面；一行里没有 `/`（一级目录本身就放不下）的按字折。
pub(super) fn wrap_path(path: &str, room: usize) -> Vec<String> {
    let mut lines = Vec::new();
    // 照字素簇算（Miyu review，2026-10-11：原来逐字符算，`⚠️` 这类字算窄了）。
    let mut rest: Vec<&str> = path.graphemes(true).collect();
    while !rest.is_empty() {
        // 放得进这一行的有几个字。
        let mut used = 0;
        let mut fit = 0;
        for g in &rest {
            let w = g.width();
            if used + w > room {
                break;
            }
            used += w;
            fit += 1;
        }
        if fit == rest.len() {
            lines.push(rest.concat());
            break;
        }
        // 放不完：断在最后一个 `/` 后面（打头的那个不算，不然留下一个孤零零的 `/`）。
        let cut = rest[..fit]
            .iter()
            .rposition(|g| *g == "/")
            .filter(|at| *at > 0)
            .map_or(fit.max(1), |at| at + 1);
        lines.push(rest[..cut].concat());
        rest.drain(..cut);
    }
    lines
}

/// 这一帧没画侧边栏的那几段（窄了、在首页、放不下、滚出去）：上一帧记的点击区、每一行是谁都不留。
pub fn clear(screen: &mut Screen) {
    screen.areas.session_id = Rect::default();
    screen.areas.jumps = Rect::default();
    screen.jumps.clear();
    screen.areas.files = Rect::default();
    screen.files.clear();
    screen.areas.recent = Rect::default();
    screen.recent.clear();
}

/// 画侧边栏：竖线；几段照先后排（`stack.rs`，一段之间空一行），从 `screen.side_top` 行露起，记下能点的那几行。
pub fn draw(frame: &mut Frame, area: Rect, seen: &Seen, screen: &mut Screen) {
    clear(screen);
    if area.width == 0 {
        return;
    }
    let line: Vec<Line> = (0..area.height)
        .map(|_| Line::styled(crate::sheet::mark(|m| &m.side), theme::dim()))
        .collect();
    frame.render_widget(
        Paragraph::new(line),
        Rect::new(area.x - 1, area.y, 1, area.height),
    );
    let inner = area.width.saturating_sub(2);
    let body = Rect::new(area.x + 1, area.y + 1, inner, area.height.saturating_sub(1));
    let (mut session, mut cwd, mut context, mut usage, mut jumps, mut files, mut recent) =
        parts::all();
    let top = screen.side_top;
    let stacked = {
        let mut side = parts::Side {
            seen,
            screen: &mut *screen,
        };
        let mut all: [&mut dyn stack::Part<parts::Side>; 7] = [
            &mut session,
            &mut cwd,
            &mut context,
            &mut usage,
            &mut jumps,
            &mut files,
            &mut recent,
        ];
        stack::stack(frame, body, &mut all, (1, top), &mut side)
    };
    // 滚到哪照排好的限过（排期第 14 条：像正文那样滚，滚轮在 `app/mouse.rs`）。
    screen.side_top = stacked.top;
    // 放不下时指针停在侧边栏上，最右边那一列画滚动条（同正文的，「正文」第 1a 条）：看得出还有、滚到了哪。
    if screen.pointer.is_some_and(|at| area.contains(at)) {
        let x = area.right().saturating_sub(1);
        super::body::bar_at(
            frame,
            (x, body.y, body.height),
            crate::scrollbar::Bar::new(stacked.total, body.height, usize::from(stacked.top)),
        );
    }
    // 写了字的行记下来，给侧边栏的选字用（第 7 条）。
    let text_rows: Vec<TextRow> = stacked
        .written
        .into_iter()
        .map(|(y, x, text, joined)| TextRow { y, x, text, joined })
        .collect();
    // 选中的反色。
    for row in &text_rows {
        if let Some((from, to)) = screen.side_select.cols(row.y) {
            let cells = Rect::new(from, row.y, to - from, 1).intersection(area);
            frame
                .buffer_mut()
                .set_style(cells, Style::new().add_modifier(Modifier::REVERSED));
        }
    }
    screen.side_select.set_rows(text_rows);
}

#[cfg(test)]
mod tests;
