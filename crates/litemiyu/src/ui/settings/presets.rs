//! 画「预设」页（`settings/presets.rs`，LM-29、LM-33）：列表一个预设一行，照技能页、MCP 页的样子（名字照最长的对齐、暗色的
//! 记号，选中的左边竖条、整行铺底）；预设的窗浮在页上（Miyu `ui/settings/more_popups.rs` 的 `preset`、`ui/feature_tree.rs`）：
//! 同别的悬浮窗一个底、一个排法（`floating.rs`），标题是预设的名字，右上角暗色写光标那一项的工具名、服务名、技能名；名字
//! 一行，空一行，下面一组组：组名加粗、后面暗色「开着的/一共」，一项 `[*] 名字`，放得下两列就排两列（左列补齐到最宽那一格、
//! 空四格接右列），放不下排一列。光标那一格自己画（竖条、铺底色），不铺整行。

use ratatui::layout::Position;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use super::floating::{self, Lines};
use crate::settings::{
    EditWindow as Edit, PresetGrid as Grid, PresetSlot as Slot, PresetTree as Tree, Settings,
};
use crate::theme;
use crate::ui::frame::Frame;
use crate::ui::rows::clip;
use lm_engine::presets::{BARE, FULL};

/// 两列之间空几格（Miyu `feature_tree::GAP`）。
const GAP: usize = 4;

/// 列表画出来的：一行行、选中的在第几行、右下角的第几个和一共几个。
pub struct Drawn {
    /// 一行行。
    pub lines: Vec<Line<'static>>,
    /// 选中的那一行在 `lines` 里是第几行。
    pub focus: usize,
    /// 选中的是第几个（从 0 数）、一共几个。
    pub place: (usize, usize),
}

/// 列表：自带的两个在前，自己定义的照名字排；名字后面暗色写「自带」「默认」（Miyu 的预设页只写「默认」，不写说明）。
pub fn lines(settings: &Settings, at: usize, width: u16) -> Drawn {
    let texts = &settings.texts.presets;
    let listed = settings.presets_listed();
    let default = listed.effective_default().to_string();
    let names: Vec<&str> = listed.names().collect();
    let shown: Vec<String> = names
        .iter()
        .map(|name| settings.preset_shown(name))
        .collect();
    let longest = shown.iter().map(|name| name.width()).max().unwrap_or(0);
    let at = at.min(names.len().saturating_sub(1));
    let lines = names
        .iter()
        .zip(&shown)
        .enumerate()
        .map(|(i, (name, shown))| {
            let mut said: Vec<&str> = Vec::new();
            if *name == FULL || *name == BARE {
                said.push(&texts.marks[0]);
            }
            if *name == default {
                said.push(&texts.marks[1]);
            }
            let said = said.join(&settings.texts.separator);
            row(i == at, (shown.as_str(), longest), &said, width)
        })
        .collect();
    Drawn {
        lines,
        focus: at,
        place: (at, names.len()),
    }
}

/// 列表的一行：竖条、名字（照 `names` 列对齐）、暗色的记号；放不下的截掉，选中的整行铺底。
fn row(chosen: bool, (name, names): (&str, usize), after: &str, width: u16) -> Line<'static> {
    let mut spans = vec![floating::lead(chosen, true)];
    let pad = " ".repeat(names.saturating_sub(name.width()) + 2);
    spans.push(Span::raw(format!("{name}{pad}")));
    let used: usize = spans.iter().map(|span| span.content.width()).sum();
    let room = usize::from(width).saturating_sub(used);
    let said = clip(after, u16::try_from(room).unwrap_or(u16::MAX));
    let rest = room.saturating_sub(said.width());
    spans.push(Span::styled(said, theme::dim()));
    spans.push(Span::raw(" ".repeat(rest)));
    let line = Line::from(spans);
    match chosen {
        true => line.patch_style(theme::row_focus()),
        false => line,
    }
}

/// 画开着的预设的窗：整屏调暗，在正中画；名字那一行正在改的（`edit`，改名的编辑窗）字写在那一行里、交回输入光标的
/// 位置（Miyu：名字就在窗里那一行改，不另开小窗）。`vim`：按键模式是 Vim，提示里的箭头写成 `j/k`、`h/l`。
pub fn window(
    frame: &mut Frame,
    settings: &Settings,
    edit: Option<&Edit>,
    vim: bool,
) -> Option<Position> {
    let (form, _) = settings.preset_form()?;
    let texts = &settings.texts.presets;
    let area = frame.area();
    frame
        .buffer_mut()
        .set_style(area, Style::new().add_modifier(Modifier::DIM));
    let width = super::popup::WIDE.min(area.width.saturating_sub(2));
    let max_height = (area.height * super::popup::HEIGHT_PERCENT / 100)
        .max(6)
        .min(area.height);
    let tree = settings.preset_tree();
    let here = form.at.as_deref().and_then(|id| tree.find(id));
    let title = crate::cli::clean(&form.name);
    // 右上角暗色写光标那一项的工具名、服务名、技能名（Miyu：只看中文名不知道到底是哪件）；和行首写的一样的不写。
    let room = usize::from(width.saturating_sub(4)).saturating_sub(title.width() + 3);
    let sub = here
        .map(|i| &tree.features[i])
        .filter(|feature| !feature.shown.is_empty() && feature.shown != feature.name)
        .map(|feature| floating::fit(&crate::cli::clean(&feature.name), room))
        .unwrap_or_default();
    let mut lines = Lines::default();
    lines.title(title, sub);
    // 名字一行：正在改的写编辑器里的字、光标停在里面。
    let lead = format!("  {}  ", texts.name_row);
    let value = match edit {
        Some(edit) => {
            let shown = edit.shown();
            let before: String = shown.chars().take(edit.cursor).collect();
            let col = lead.width() + before.width();
            lines.caret = Some((0, u16::try_from(col).unwrap_or(u16::MAX)));
            lines.error.clone_from(&edit.error);
            shown
        }
        None => crate::cli::clean(&form.name),
    };
    lines.row(
        Line::from(vec![Span::styled(lead, theme::dim()), Span::raw(value)]),
        here.is_none() || edit.is_some(),
    );
    lines.plain(Line::default());
    // 窗里左右各留两格。
    let drawn = tree_lines(settings, &tree, here, usize::from(width.saturating_sub(4)));
    form.two.set(drawn.two);
    let first = lines.body.len();
    lines.focus = drawn.focus.filter(|_| edit.is_none()).map(|i| first + i);
    for line in drawn.lines {
        lines.plain(line);
    }
    lines.hints = match edit {
        Some(_) => settings.texts.key_hints.edit.clone(),
        None => {
            let mut hints = texts.window_hints.clone();
            if drawn.two {
                hints.insert(1.min(hints.len()), texts.column_hint.clone());
            }
            hints
        }
    };
    for hint in &mut lines.hints {
        for part in hint.iter_mut() {
            *part = crate::ui::hint_keys::vim(part, vim);
            if vim {
                *part = part.replace("←/→", "h/l");
            }
        }
    }
    floating::centered_from(
        frame,
        area,
        (width, max_height),
        &lines,
        (&settings.hits, &form.top),
    )
}

/// 功能那一块画出来的：一行行（两列已经接好）、光标那一格在第几行、排成了两列没有。
struct TreeLines {
    lines: Vec<Line<'static>>,
    focus: Option<usize>,
    two: bool,
}

/// 照 Miyu `feature_tree::draw` 排：放得下两列（左列最宽那一格、空四格、右列最宽那一格，连各自打头的两格）就两列，
/// 不然一列；`here` 是光标停的那一项，`room` 是能用的宽。
fn tree_lines(settings: &Settings, tree: &Tree, here: Option<usize>, room: usize) -> TreeLines {
    let cell = |slot: Slot| cell(settings, tree, slot);
    let wide = |grid: &Grid, c: usize| {
        grid.cols.get(c).map_or(0, |col| {
            col.iter()
                .map(|slot| width_of(&cell(*slot)))
                .max()
                .unwrap_or(0)
        })
    };
    let two = Grid::new(tree, true);
    let need = 2 + wide(&two, 0) + GAP + 2 + wide(&two, 1);
    let (grid, is_two) = if two.cols.len() > 1 && need <= room {
        (two, true)
    } else {
        (Grid::new(tree, false), false)
    };
    let left_w = wide(&grid, 0);
    let rows = grid.cols.iter().map(Vec::len).max().unwrap_or(0);
    let mut lines = Vec::with_capacity(rows);
    let mut focus = None;
    for i in 0..rows {
        let mut spans = Vec::new();
        for (c, col) in grid.cols.iter().enumerate() {
            let slot = col.get(i).copied();
            let chosen = here.is_some_and(|f| slot == Some(Slot::Feature(f)));
            if chosen {
                focus = Some(i);
            }
            let content = slot.map(&cell).unwrap_or_default();
            // 左列补齐到最宽那一格、再空几格接右列；右列、一列的不补。
            let (fill, gap) = if c == 0 && grid.cols.len() > 1 {
                (left_w.saturating_sub(width_of(&content)), GAP)
            } else {
                (0, 0)
            };
            if chosen {
                let bg = theme::row_focus();
                let lead = floating::lead(true, true);
                spans.push(Span::styled(
                    lead.content.into_owned(),
                    lead.style.patch(bg),
                ));
                spans.extend(content.into_iter().map(|span| span.patch_style(bg)));
                spans.push(Span::styled(" ".repeat(fill), bg));
            } else {
                spans.push(Span::raw("  "));
                spans.extend(content);
                spans.push(Span::raw(" ".repeat(fill)));
            }
            spans.push(Span::raw(" ".repeat(gap)));
        }
        lines.push(Line::from(spans));
    }
    TreeLines {
        lines,
        focus,
        two: is_two,
    }
}

/// 一格里写的（不带打头那两格）：组名加粗、后面暗色「开着的/一共」；一组什么都没有的暗色「还没有…」；一项 `[*] 名字`
/// （关着的勾和名字一起暗），后面写为什么不可用（开着却用不了的、已经没有的黄字）。
fn cell(settings: &Settings, tree: &Tree, slot: Slot) -> Vec<Span<'static>> {
    let texts = &settings.texts.presets;
    match slot {
        Slot::Group(group) => {
            let (on, all) = tree.tally(group);
            vec![
                Span::styled(
                    texts.groups[group.min(2)].clone(),
                    Style::new().add_modifier(Modifier::BOLD),
                ),
                Span::styled(format!("  {on}/{all}"), theme::dim()),
            ]
        }
        Slot::Empty(group) => {
            let said = texts.empty[group.saturating_sub(1).min(1)].clone();
            vec![Span::styled(said, theme::dim())]
        }
        Slot::Blank => Vec::new(),
        Slot::Feature(i) => {
            let feature = &tree.features[i];
            let (mark, name) = match feature.on {
                true => ("[*]", (theme::accent(), Style::new())),
                false => ("[ ]", (theme::dim(), theme::dim())),
            };
            let label = match feature.shown.is_empty() {
                true => crate::cli::clean(&feature.name),
                false => crate::cli::clean(&feature.shown),
            };
            let mut out = vec![
                Span::styled(mark, name.0),
                Span::raw(" "),
                Span::styled(label, name.1),
            ];
            let why = if feature.missing {
                Some((texts.missing.clone(), theme::warn()))
            } else if feature.global_off {
                let said = match feature.group {
                    1 => texts.mcp_off.clone(),
                    _ => texts.skill_off.clone(),
                };
                Some((
                    said,
                    if feature.on {
                        theme::warn()
                    } else {
                        theme::dim()
                    },
                ))
            } else {
                None
            };
            if let Some((said, style)) = why {
                out.push(Span::raw("  "));
                out.push(Span::styled(said, style));
            }
            out
        }
    }
}

/// 几段字一共多宽。
fn width_of(spans: &[Span]) -> usize {
    spans.iter().map(|span| span.content.width()).sum()
}
