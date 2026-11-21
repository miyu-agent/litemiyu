//! 输入框下面那一行：左边模型，右边用量。

use crate::ui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use unicode_width::UnicodeWidthStr;

use crate::app::{Screen, Seen};
use crate::focus::{Button, Focus};
use crate::theme;
use crate::transcript::Link;
use crate::ui::usage_cell::{self, Hit};

/// 左右两段之间至少空几列。
const GAP: usize = 2;

/// 画那一行。左边 `deepseek-flash deepseek`；右边是用量那一格 `5.6k/1M(0.6%) · Σ120M(C99%) · $0.42`（`usage_cell.rs`），
/// 还不知道的那一段不写。有后台命令在跑时，中间的空当正中是后台按钮（Miyu 蓝图「后台命令、子代理和侧边栏」第 1 条）。
///
/// 一行放不下时，右边从右往左整段省，上下文留到最后（2026-11-14 项目主人）：左右两段不许叠在一起，叠上以后宽字被盖掉
/// 一半，下一帧会留下残字。按钮留到最后（同 Miyu `ui/footer.rs` 的 `draw`：先给它留位，右边的格子照 `keep` 让位）。交回
/// 用量那一格、「压缩」、后台按钮在哪。
pub fn draw(frame: &mut Frame, area: Rect, seen: &Seen, screen: &Screen, spin: usize) -> Hits {
    let focus = seen.focus();
    let count = seen.talk.jobs.running_commands();
    let button = (count > 0).then(|| {
        seen.config
            .text()
            .jobs
            .button
            .replace("{count}", &count.to_string())
    });
    let button_w = button.as_ref().map_or(0, |b| b.width() + GAP);
    // 用量那一格（`usage_cell.rs`）：上下文、累计；卡住的红字加「压缩」，正在压的转圈。
    let facts = usage_cell::Facts {
        t: &seen.talk.transcript,
        total: seen.talk.usage_total(),
        spent: seen.spent(),
        texts: &seen.config.front.usage_cell,
        spinner: &seen.config.timeline.spinner,
    };
    let cells: Vec<Part> = usage_cell::pieces(&facts, spin, hovered(screen))
        .into_iter()
        .map(Part::from)
        .collect();
    let widths: Vec<usize> = cells
        .iter()
        .map(|p| p.spans.iter().map(Span::width).sum())
        .collect();
    let cells_meta: Vec<Meta> = cells.iter().map(|p| (p.tight, p.hit)).collect();
    // 预设名（LM-29）：看不到侧边栏时（首页、窄窗口）接在模型后面；位置照布局表的 `preset_tag`，全部功能的不写。
    let preset = (seen.config.layout.preset_tag == "footer"
        && screen.areas.sidebar.width == 0
        && seen.rescue.is_none())
    .then(|| crate::app::presets::tag(seen.talk, seen.link, seen.config))
    .flatten();
    let (left, right, kept) = split(
        [
            left(seen, Model::Full, preset.as_deref()),
            left(seen, Model::Short, preset.as_deref()),
            left(seen, Model::Bare, None),
        ],
        cells,
        button_w,
        area.width,
    );
    frame.render_widget(Paragraph::new(left.clone()), area);
    let right_w = right.width();
    frame.render_widget(Paragraph::new(right.right_aligned()), area);
    let mut hits = hits(area, right_w, (&cells_meta, &widths, &kept));
    let Some(button) = button else {
        return hits;
    };
    let Some(x) = middle(
        left.width(),
        right_w,
        button.width(),
        usize::from(area.width),
    ) else {
        return hits;
    };
    let at = Rect::new(
        area.x + u16::try_from(x).unwrap_or(0),
        area.y,
        u16::try_from(button.width()).unwrap_or(0),
        1,
    );
    let style = button_style(screen, at, focus == Focus::Footer(Button::Background));
    frame.render_widget(Paragraph::new(Line::styled(button, style)), at);
    hits.jobs = at;
    hits
}

/// 按钮的样子：强调色；悬停加粗；焦点在上面反色（Miyu「后台命令、子代理和侧边栏」第 1、2 条，`button_style`）。
fn button_style(screen: &Screen, at: Rect, focused: bool) -> ratatui::style::Style {
    if focused {
        return theme::selected();
    }
    let hovered = screen.pointer.is_some_and(|p| at.contains(p));
    let style = theme::accent();
    if hovered {
        style.add_modifier(ratatui::style::Modifier::BOLD)
    } else {
        style
    }
}

/// 用量那一格（点了开 `/status`）、「压缩」（点了同 `/compact`）在哪；没画的是空的。
#[derive(Debug, Clone, Copy, Default)]
pub struct Hits {
    pub usage: Rect,
    pub compact: Rect,
    /// 后台按钮（点了开后台面板）。
    pub jobs: Rect,
}

/// 指针停在用量那一格的哪一截上（照上一帧的位置）。
fn hovered(screen: &Screen) -> Option<Hit> {
    let p = screen.pointer?;
    if screen.areas.compact.contains(p) {
        Some(Hit::Compact)
    } else if screen.areas.usage_cell.contains(p) {
        Some(Hit::Usage)
    } else {
        None
    }
}

/// 一格和前一格只隔一个空格没有、点了做什么（[`Part`] 的 `tight`、`hit`）。
type Meta = (bool, Option<Hit>);

/// 用量那一格的几截在哪：右边那一段右对齐，留下的格子照先后排、之间 ` · `（`tight` 的一个空格）。
fn hits(area: Rect, right_w: usize, (meta, widths, kept): (&[Meta], &[usize], &[bool])) -> Hits {
    let mut x = usize::from(area.right()).saturating_sub(right_w);
    let mut hits = Hits::default();
    let mut first = true;
    let sep = crate::sheet::sep().width();
    for (i, (&w, _)) in widths
        .iter()
        .zip(kept)
        .enumerate()
        .filter(|(_, (_, k))| **k)
    {
        let (tight, hit) = meta.get(i).copied().unwrap_or_default();
        if !first {
            x += if tight { 1 } else { sep };
        }
        first = false;
        let rect = Rect::new(
            u16::try_from(x).unwrap_or(0),
            area.y,
            u16::try_from(w).unwrap_or(0),
            1,
        );
        match hit {
            Some(Hit::Compact) => hits.compact = rect,
            Some(Hit::Usage) if hits.usage.width == 0 => hits.usage = rect,
            Some(Hit::Usage) => hits.usage = hits.usage.union(rect),
            None => {}
        }
        x += w;
    }
    hits
}

/// 一行怎么分：左边从全的到窄的三种（`lefts`：全的；模型编号只写最后一段的；最后才收掉供应商名），右边的格子全放得下
/// 就用最宽的那种，空出的地方给右边；最窄的也放不下右边全部时再照 [`choose`] 挑格子（2026-10-11 项目主人定：原来模型编号
/// 一长，速度、累计先丢了，中间空着一大截；2026-10-23 项目主人：供应商名最后再收，不然不知道用的是哪家）。
/// `button` 是后台按钮连两边的空占几列（Miyu `split`）。交回左边、右边。
fn split(
    lefts: [Vec<Span<'_>>; 3],
    cells: Vec<Part>,
    button: usize,
    width: u16,
) -> (Line<'static>, Line<'static>, Vec<bool>) {
    let need = joined_width(&cells);
    // 速度这几格（`keep` 不过 [`EARLY`]）比供应商名先丢：丢了它们放得下的，照旧写全。
    let late: Vec<&Part> = cells.iter().filter(|p| p.keep > EARLY).collect();
    let need_late = joined_width_of(&late);
    let fits = |spans: &Vec<Span<'_>>, need: usize| {
        spans.iter().map(Span::width).sum::<usize>() + GAP + button + need <= usize::from(width)
    };
    let [full, short, bare] = lefts;
    let chosen = if fits(&full, need) {
        full
    } else if fits(&short, need_late) {
        short
    } else {
        bare
    };
    let left = left_line(chosen, width);
    let room = usize::from(width).saturating_sub(left.width() + GAP + button);
    let kept = choose(&cells, room);
    let right = Line::from(join(cells, &kept));
    (left, right, kept)
}

/// 这几格照 ` · ` 连起来有几列。
fn joined_width(parts: &[Part]) -> usize {
    joined_width_of(&parts.iter().collect::<Vec<_>>())
}

/// 同 [`joined_width`]，给挑出来的几格。
fn joined_width_of(parts: &[&Part]) -> usize {
    parts
        .iter()
        .enumerate()
        .map(|(i, p)| gap_before(p, i == 0) + p.spans.iter().map(Span::width).sum::<usize>())
        .sum()
}

/// `keep` 不过它的格子（速度）比供应商名先丢。
const EARLY: u8 = 2;

/// 按钮从第几列起：左右两段之间的空当（两边各留 `GAP`）正中；放不下是 `None`（Miyu `middle`）。
fn middle(left: usize, right: usize, button: usize, width: usize) -> Option<usize> {
    let start = left + GAP;
    let end = width.checked_sub(right + GAP)?;
    let gap = end.checked_sub(start)?;
    (gap >= button).then(|| start + (gap - button) / 2)
}

/// 右边的一格：放不下时先丢 `keep` 小的；`tight` 的和前一格只隔一个空格（用量那一格卡住时的几截）；`hit` 是点了做什么。
#[derive(Clone)]
struct Part {
    keep: u8,
    tight: bool,
    hit: Option<Hit>,
    spans: Vec<Span<'static>>,
}

#[cfg(test)]
impl Part {
    /// 只有字的一格（测试里摆格子用）。
    fn new(keep: u8, spans: Vec<Span<'static>>) -> Self {
        Self {
            keep,
            tight: false,
            hit: None,
            spans,
        }
    }
}

impl From<usage_cell::Piece> for Part {
    fn from(p: usage_cell::Piece) -> Self {
        Self {
            keep: p.keep,
            tight: p.tight,
            hit: p.hit,
            spans: p.spans,
        }
    }
}

/// 这一格前面隔几列：头一格不隔，`tight` 的一个空格，别的 ` · `。
fn gap_before(part: &Part, first: bool) -> usize {
    match (first, part.tight) {
        (true, _) => 0,
        (false, true) => 1,
        (false, false) => crate::sheet::sep().width(),
    }
}

/// 照 `keep` 从大到小挑，放得进 `room` 列的留下，头一个放不下的连同比它先丢的都不要（整段省）；留下的照原来的先后，
/// 用 ` · ` 隔开。
#[cfg(test)]
fn fit(parts: Vec<Part>, room: usize) -> Vec<Span<'static>> {
    let kept = choose(&parts, room);
    join(parts, &kept)
}

/// 照 `keep` 从大到小挑：哪几格留下。
fn choose(parts: &[Part], room: usize) -> Vec<bool> {
    // 照 `keep` 从大到小、一样的照原来的先后（共用一份排法，`fuzzy::best_first`）。
    let mut order: Vec<(i64, usize)> = parts
        .iter()
        .enumerate()
        .map(|(i, part)| (i64::from(part.keep), i))
        .collect();
    crate::fuzzy::best_first(&mut order);
    let mut kept = vec![false; parts.len()];
    for (_, i) in order {
        kept[i] = true;
        let chosen: Vec<&Part> = parts
            .iter()
            .zip(&kept)
            .filter(|(_, k)| **k)
            .map(|(p, _)| p)
            .collect();
        // 放不下：这一段和比它先丢的都不画，不拿后面短的补空（2026-11-14 项目主人：「宽度不够就从右往左依次省略」；
        // 原来 2026-10-11 定的是跳过长的、短的照放）。
        if joined_width_of(&chosen) > room {
            kept[i] = false;
            break;
        }
    }
    kept
}

/// 留下的照原来的先后，用 ` · ` 隔开。
fn join(parts: Vec<Part>, kept: &[bool]) -> Vec<Span<'static>> {
    let parts = parts
        .into_iter()
        .zip(kept)
        .filter(|(_, k)| **k)
        .map(|(p, _)| p);
    let mut spans = Vec::new();
    for (i, part) in parts.enumerate() {
        match gap_before(&part, i == 0) {
            0 => {}
            1 => spans.push(Span::raw(" ")),
            _ => spans.push(Span::styled(crate::sheet::sep(), theme::dim())),
        }
        spans.extend(part.spans);
    }
    spans
}

/// 左边那一段：放不下截掉加 `…`（`tui.md`「框下面那一行」）。
fn left_line(spans: Vec<Span<'_>>, width: u16) -> Line<'static> {
    let owned = spans
        .into_iter()
        .map(|s| Span::styled(s.content.into_owned(), s.style))
        .collect();
    Line::from(super::panel::clip_spans(owned, usize::from(width)))
}

/// 模型那一格写多全（[`split`] 从全的往窄里挑）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Model {
    /// 模型编号接暗色的供应商名。
    Full,
    /// 模型编号只写最后一段（`clinepass/cline-pass/deepseek-v4.1-flash` 写 `deepseek-v4.1-flash`），供应商名照写。
    Short,
    /// 只写最后一段，供应商名也收掉（最窄的时候）。
    Bare,
}

/// 左边那一段：模型和端点、思考强度。`model`：模型那一格写多全；一行放不下时用窄的（[`split`]）。
/// `preset` 是接在后面的预设名（最窄的那一种不写）。
fn left<'a>(seen: &Seen<'a>, model: Model, preset: Option<&str>) -> Vec<Span<'a>> {
    let t = &seen.talk.transcript;
    match &t.link {
        // 整句红字折行写在输入框上面（`above.rs`），这里只写前面那一截。
        Link::Down(reason) => vec![Span::styled(super::above::head(reason), theme::error())],
        Link::Ready => {
            let mut spans = Vec::new();
            if let Some((shown, endpoint)) = t.shown_model() {
                spans.extend(model_spans(shown, &endpoint, model));
                // 思考强度：什么都不发的不写。
                if let Some(effort) = t.effort() {
                    // 强调色（2026-10-02 项目主人：和「工作区」那个蓝一样）。
                    spans.push(Span::styled(crate::sheet::sep(), theme::dim()));
                    spans.push(Span::styled(effort.to_string(), theme::accent()));
                }
            }
            if let Some(preset) = preset {
                if !spans.is_empty() {
                    spans.push(Span::styled(crate::sheet::sep(), theme::dim()));
                }
                spans.push(Span::styled(preset.to_string(), theme::dim()));
            }
            spans
        }
    }
}

/// 模型那一格：全的写模型编号接暗色的供应商名；窄的先只写编号最后一段、供应商名照写，最窄才收掉供应商名（2026-10-19 项目主人：
/// 宽度不够时供应商信息也可以隐藏；2026-10-23 改：「最后再缩，而不是一开始就缩」，同名的模型分不出是哪家）。
fn model_spans(model: String, endpoint: &str, how: Model) -> Vec<Span<'static>> {
    let name = match how {
        Model::Full => model,
        Model::Short | Model::Bare => model.rsplit('/').next().unwrap_or(&model).to_string(),
    };
    let mut spans = vec![Span::styled(name, theme::model())];
    if how != Model::Bare {
        spans.push(Span::styled(format!(" {endpoint}"), theme::dim()));
    }
    spans
}

#[cfg(test)]
mod tests;
