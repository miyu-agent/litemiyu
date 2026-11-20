//! `/status` 会话状态框（Miyu 终端 `ui/status_box.rs`，蓝图 `tui.md`「会话状态 `/status`」）：左上角框线上写会话标题，一行一项，
//! 左边一列名字对齐、右边写值，放不下的折到下一行、照值那一列对齐。两段之间空一行：会话 ID、当前工作目录；模型、上下文、
//! 压缩、用量、缓存命中。还没开会话的标题写「新会话」，只写当前工作目录、模型。点框下面那一行用量那一格也开它。
//!
//! 和 Miyu 不一样的：LiteMiyu 没有的行不写——人格、预设、权限模式（没有这几样）、各模型（引擎不分模型记用量）、记忆（没有
//! 记忆库）、会话钉的压缩线和会话上限（界面不接，2026-11-14 项目主人定）。花费照底栏那一格（连同子代理，`Talk::spent`），
//! 没有数的（模型没价格）不写。
//!
//! 先从界面收齐要写的（[`facts`]），再排（[`items`]、[`rows`]）：排的那一半不碰界面，单元测试直接给数。

use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use super::panel::{self, Chrome};
use crate::app::Seen;
use crate::config::{Bar, Config, texts};
use crate::core::Usage;
use crate::meter;
use crate::theme;

texts! {
    /// 框里的字。
    #[derive(Debug, Clone)]
    pub struct Texts {
        /// 还没开会话时框线上写的。
        pub new_title: String,
        /// 左边一列的名字。
        pub keys: Keys,
        /// 思考程度，`{effort}`。
        pub effort: String,
        /// 窗口，`{window}`。
        pub window: String,
        /// 平均出字速度，`{rate}`。
        pub average: String,
        /// 上下文用了多少，`{used}`、`{window}`、`{percent}`。
        pub context: String,
        /// 压缩过几次，`{n}`。
        pub compactions: String,
        /// 压缩线，`{line}`。
        pub compaction_line: String,
        /// 共多少，`{tokens}`。
        pub total: String,
        /// 输入、输出，`{input}`、`{output}`。
        pub split: String,
        /// 辅助请求，`{tokens}`。
        pub aux: String,
        /// 缓存断了几次，`{n}`。
        pub breaks: String,
        /// 下边框的按键提示。
        pub hint: String,
    }
}

texts! {
    /// 左边一列的名字。
    #[derive(Debug, Clone)]
    pub struct Keys {
        pub id: String,
        pub cwd: String,
        pub model: String,
        pub context: String,
        pub compaction: String,
        pub usage: String,
        pub hit: String,
        pub cost: String,
    }
}

/// 框里要写的，从界面收齐的。
#[derive(Debug, Clone, Default)]
pub struct Facts {
    /// 框线上的标题。
    pub title: String,
    /// 完整的会话编号；还没开会话的是 `None`。
    pub session: Option<String>,
    /// 当前工作目录，家目录写成 `~`。
    pub cwd: String,
    /// 模型、端点。
    pub model: Option<(String, String)>,
    pub effort: Option<String>,
    pub window: Option<u64>,
    /// 平均出字速度，每秒几个 token。
    pub speed: Option<u64>,
    /// 上下文用了多少；压缩线。
    pub context: u64,
    pub line: Option<u64>,
    /// 压缩过几次、缓存断了几次。
    pub compactions: u64,
    pub breaks: u64,
    /// 会话累计（同底栏那一格）。
    pub usage: Usage,
    /// 花了多少，写好的（`$0.42 + ¥1.30`）；没有数的是 `None`（同底栏那一格）。
    pub spent: Option<String>,
}

/// 一行：名字和值（值是几段字，亮的、暗的）；`None` 是空行。
pub type Item = Option<(String, Vec<Span<'static>>)>;

/// 从界面收齐要写的。
pub fn facts(seen: &Seen) -> Facts {
    let text = seen.config.text();
    let t = &seen.talk.transcript;
    let title = match &t.session {
        None => text.status.new_title.clone(),
        Some(_) => t
            .title
            .clone()
            .unwrap_or_else(|| text.sessions.untitled.clone()),
    };
    let cwd = seen.talk.workdir();
    Facts {
        title,
        session: t.session.clone(),
        cwd: crate::local::home_short(&cwd.to_string_lossy()),
        model: t.shown_model(),
        effort: t.effort().map(str::to_string),
        window: t.limits.window,
        speed: t.speed.rounded(),
        context: t.context,
        line: t.limits.compaction_line,
        compactions: t.cache.compactions,
        breaks: t.cache.breaks,
        usage: seen.talk.usage_total(),
        spent: seen.spent(),
    }
}

/// 一行一项，照 Miyu 样板的先后；没有的那一行不写，空了的那一段连它后面的空行一起不写。
pub fn items(f: &Facts, words: &Texts, bar: &Bar, room: usize) -> Vec<Item> {
    let keys = &words.keys;
    let opened = f.session.is_some();
    let dim = |s: String| Span::styled(s, theme::dim());
    let tail = |s: String| Span::styled(format!("{}{s}", crate::sheet::sep()), theme::dim());
    let plain = |s: String| vec![Span::raw(s)];
    let mut groups: [Vec<(String, Vec<Span<'static>>)>; 2] = Default::default();

    // 第一段：会话 ID、当前工作目录。
    let first = &mut groups[0];
    if let Some(id) = &f.session {
        first.push((keys.id.clone(), plain(id.clone())));
    }
    first.push((keys.cwd.clone(), plain(f.cwd.clone())));

    // 第二段：模型、上下文、压缩、用量、缓存命中。
    let second = &mut groups[1];
    if let Some((model, endpoint)) = &f.model {
        let mut value = vec![
            Span::raw(model.clone()),
            dim(format!("{}{endpoint}", crate::sheet::sep())),
        ];
        if let Some(effort) = &f.effort {
            value.push(tail(words.effort.replace("{effort}", effort)));
        }
        if let Some(window) = f.window {
            value.push(tail(
                words.window.replace("{window}", &meter::short(window)),
            ));
        }
        if let Some(rate) = f.speed {
            value.push(tail(words.average.replace("{rate}", &rate.to_string())));
        }
        second.push((keys.model.clone(), value));
    }
    if opened && f.context > 0 {
        second.push((keys.context.clone(), context(f, words, bar, room)));
    }
    if opened && (f.compactions > 0 || f.line.is_some()) {
        let n = f.compactions.to_string();
        let mut value = vec![Span::raw(words.compactions.replace("{n}", &n))];
        if let Some(line) = f.line {
            value.push(tail(
                words.compaction_line.replace("{line}", &meter::short(line)),
            ));
        }
        second.push((keys.compaction.clone(), value));
    }
    let u = &f.usage;
    let input = u.input();
    if opened && input + u.output + u.aux > 0 {
        let sum = meter::short(input + u.output + u.aux);
        let split = words
            .split
            .replace("{input}", &meter::short(input))
            .replace("{output}", &meter::short(u.output));
        let mut value = vec![
            Span::raw(words.total.replace("{tokens}", &sum)),
            tail(split),
        ];
        if u.aux > 0 {
            value.push(tail(words.aux.replace("{tokens}", &meter::short(u.aux))));
        }
        second.push((keys.usage.clone(), value));
        let rate = format!("{}%", meter::hit_rate(u.cache_read, input));
        let mut hit = vec![Span::styled(rate, theme::good())];
        if f.breaks > 0 {
            hit.push(tail(words.breaks.replace("{n}", &f.breaks.to_string())));
        }
        second.push((keys.hit.clone(), hit));
    }
    if let Some(spent) = f.spent.clone().filter(|_| opened) {
        second.push((keys.cost.clone(), plain(spent)));
    }

    // 上下的空行由框留，这里只在段和段之间空一行。
    let mut out = Vec::new();
    for group in groups.into_iter().filter(|g| !g.is_empty()) {
        if !out.is_empty() {
            out.push(None);
        }
        out.extend(group.into_iter().map(Some));
    }
    out
}

/// 上下文那一格：一根条（压缩线落的那一格标黄，同 Miyu 侧边栏）接用了多少。
fn context(f: &Facts, words: &Texts, marks: &Bar, room: usize) -> Vec<Span<'static>> {
    let used = meter::short(f.context);
    let Some(window) = f.window else {
        return vec![Span::raw(used)];
    };
    let mut spans = bar(f.context, window, f.line, marks.width.min(room / 3), marks);
    let value = words
        .context
        .replace("{used}", &used)
        .replace("{window}", &meter::short(window))
        .replace("{percent}", &meter::percent_tenths(f.context, window));
    spans.push(Span::raw(format!("  {value}")));
    spans
}

/// 上下文那根进度条（Miyu 终端 `ui/sidebar.rs` 的 `bar`，2026-09-30 项目主人定）：用了的强调色、没用的暗；压缩线落在的那一格
/// 换成警示色（黄），字不变。`line` 是压缩线，没有的不标。用过就至少亮一格，不然用得少时看不出有这根条。侧边栏上下文那一段
/// 也画这一根（`sidebar.rs`）。
pub(super) fn bar(
    used: u64,
    window: u64,
    line: Option<u64>,
    room: usize,
    marks: &Bar,
) -> Vec<Span<'static>> {
    let full = if window == 0 || used == 0 {
        0
    } else {
        (((used as f64 / window as f64) * room as f64).round() as usize).max(1)
    }
    .min(room);
    let mark = line
        .filter(|_| window > 0 && room > 0)
        .map(|l| (((l as f64 / window as f64) * room as f64).floor() as usize).min(room - 1));
    (0..room)
        .map(|i| {
            let (glyph, style) = if i < full {
                (&marks.full, theme::accent())
            } else {
                (&marks.empty, theme::dim())
            };
            let style = if mark == Some(i) {
                theme::warn()
            } else {
                style
            };
            Span::styled(glyph.clone(), style)
        })
        .collect()
}

/// 排好的一行行（滚动前的全部）：名字一列照最长的那个对齐，值放不下的折到下一行、照值那一列对齐。
pub fn rows(f: &Facts, config: &Config, width: u16) -> Vec<Line<'static>> {
    let room = usize::from(width).saturating_sub(2).max(1);
    let items = items(f, &config.text().status, &config.layout.bar, room);
    let key_width = items
        .iter()
        .flatten()
        .map(|(k, _)| k.width())
        .max()
        .unwrap_or(0)
        + 2;
    let value_room = room.saturating_sub(key_width).max(1);
    let mut out = Vec::new();
    for item in items {
        let Some((key, value)) = item else {
            out.push(Line::raw(""));
            continue;
        };
        for (n, piece) in wrap(value, value_room).into_iter().enumerate() {
            let head = if n == 0 { key.as_str() } else { "" };
            let pad = " ".repeat(key_width.saturating_sub(head.width()));
            let mut spans = vec![Span::styled(format!("{head}{pad}"), theme::dim())];
            spans.extend(piece);
            out.push(Line::from(spans));
        }
    }
    out
}

/// 框、这一屏的行、每一行是第几个（都选不中）。
pub fn lines(
    seen: &Seen,
    scroll: usize,
    width: u16,
    max: usize,
) -> (Chrome, Vec<Line<'static>>, Vec<Option<usize>>) {
    let f = facts(seen);
    let chrome = Chrome {
        title: vec![Span::raw(f.title.clone())],
        hint: Some(seen.config.text().status.hint.clone()),
    };
    let lines: Vec<Line<'static>> = rows(&f, seen.config, width)
        .into_iter()
        .skip(scroll)
        .take(max)
        .map(|line| panel::item(false, line.spans, None, width))
        .collect();
    let map = vec![None; lines.len()];
    (chrome, lines, map)
}

/// 照纯文字写整个框（`c` 复制）：第一行标题，下面照框里的样子。
pub fn plain(f: &Facts, config: &Config, width: u16) -> String {
    let mut out = vec![f.title.clone()];
    let body = rows(f, config, width);
    out.extend(body.iter().map(|l| l.to_string().trim_end().to_string()));
    out.join("\n").trim_end().to_string()
}

/// 几段字照显示宽度折成几行，每行不超过 `room` 列。
fn wrap(spans: Vec<Span<'static>>, room: usize) -> Vec<Vec<Span<'static>>> {
    use unicode_segmentation::UnicodeSegmentation;
    let mut lines: Vec<Vec<Span<'static>>> = vec![Vec::new()];
    let mut used = 0;
    for span in spans {
        let mut piece = String::new();
        for g in span.content.graphemes(true) {
            let w = g.width();
            if used + w > room && used > 0 {
                if !piece.is_empty() {
                    let done = Span::styled(std::mem::take(&mut piece), span.style);
                    lines
                        .last_mut()
                        .into_iter()
                        .for_each(|l| l.push(done.clone()));
                }
                lines.push(Vec::new());
                used = 0;
            }
            piece.push_str(g);
            used += w;
        }
        if !piece.is_empty() {
            let done = Span::styled(piece, span.style);
            lines
                .last_mut()
                .into_iter()
                .for_each(|l| l.push(done.clone()));
        }
    }
    lines
}

#[cfg(test)]
mod tests;
