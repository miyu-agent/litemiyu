//! 画「供应商和模型」的悬浮窗（Miyu 终端 `ui/settings/popup.rs`、`fields.rs`、`catalog.rs`，蓝图「配置页」第 11a、14 到 20
//! 条）：编辑窗（一行一项：名字、值、来源）、选默认模型的窗、问一句的窗、目录窗。盖在整屏正中，背后调暗。

use ratatui::layout::Position;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use super::super::frame::Frame;
use super::floating::{self, Lines, fit};
use crate::settings::models::catalog::{CatalogPick, Row as CatalogRow};
use crate::settings::models::forms::{Field, Focus, Form, Row, Target, Val, window_text};
use crate::settings::models::nav::Use;
use crate::settings::models::pick::{Choice, Item, Pick};
use crate::settings::models::texts::ModelTexts;
use crate::settings::models::{Models, Popup};
use crate::settings::{Hit, Hits};
use crate::theme;

/// 名字那一栏最少多宽（连左边两格）；这一窗里有更宽的名字时照最宽的那个加两格空（LM-9补：英文的「Context window」比它宽）。
const LABEL: u16 = 14;
/// 悬浮窗多宽、宽一些的（模型、池、选模型）多宽、最高占几成（Miyu `layout.json` 的 `settings`）。
const POPUP_WIDTH: u16 = 66;
const POPUP_WIDE: u16 = 78;
const POPUP_HEIGHT_PERCENT: u16 = 86;

/// 画开着的悬浮窗：背后整屏调暗；交回输入光标的位置。
pub fn draw(
    frame: &mut Frame,
    (models, hits): (&Models, &Hits),
    texts: &ModelTexts,
    import: &str,
) -> Option<Position> {
    let popup = models.popup.as_ref()?;
    let area = frame.area();
    frame
        .buffer_mut()
        .set_style(area, Style::new().add_modifier(Modifier::DIM));
    let wide = !matches!(
        popup,
        Popup::Form(Form {
            target: Target::Provider(_),
            ..
        }) | Popup::Confirm(_)
    );
    let width = if wide { POPUP_WIDE } else { POPUP_WIDTH }.min(area.width.saturating_sub(2));
    let max_height = (area.height * POPUP_HEIGHT_PERCENT / 100)
        .max(6)
        .min(area.height);
    let mut lines = Lines::default();
    match popup {
        Popup::Form(form) => form_lines(&mut lines, form, models, texts, width),
        Popup::Pick(pick) => pick_lines(&mut lines, pick, models, texts, width),
        Popup::Catalog(pick) => catalog_lines(&mut lines, pick, texts, (width, import)),
        Popup::Confirm(confirm) => {
            lines.title(confirm.title.clone(), String::new());
            for (text, dim) in &confirm.lines {
                let style = if *dim { theme::dim() } else { Style::new() };
                lines.plain(Line::styled(text.clone(), style));
            }
            lines.buttons.clone_from(&confirm.buttons);
            lines.button = Some(confirm.sel);
            lines.hints.clone_from(&texts.popup_hints[3]);
        }
    }
    floating::centered(frame, area, (width, max_height), &lines, hits)
}

fn form_lines(lines: &mut Lines, form: &Form, models: &Models, texts: &ModelTexts, width: u16) {
    let shown = |id: &str| {
        models
            .data
            .provider(id)
            .map_or(id.to_string(), |p| p.shown().to_string())
    };
    let (title, sub) = match &form.target {
        // 只填 key 的窗：标题后面写目录里的名字。
        Target::Provider(None) => (
            texts.titles[0].clone(),
            form.key_only.clone().unwrap_or_default(),
        ),
        Target::Provider(Some(id)) => {
            (texts.titles[1].replace("{name}", &shown(id)), String::new())
        }
        Target::Model(p, None) => (texts.titles[2].replace("{name}", &shown(p)), String::new()),
        Target::Model(p, Some(m)) => (m.clone(), shown(p)),
    };
    lines.title(title, sub);
    let env = form.choice(Field::Auth) == Some("env");
    let label_of = |row: &Row| {
        let name = if row.field == Field::Key && env {
            "env_key"
        } else {
            row.field.name()
        };
        texts.field(name)
    };
    // 名字那一栏：左边两格、最宽的名字、再空两格，不比 LABEL 窄。
    let column = form
        .rows
        .iter()
        .enumerate()
        .filter(|(i, _)| form.shows(*i))
        .map(|(_, row)| label_of(row).width() + 4)
        .fold(usize::from(LABEL), usize::max);
    let value_width = usize::from(width).saturating_sub(column + 14);
    for (i, row) in form.rows.iter().enumerate() {
        let focused = form.focus == Focus::Row(i);
        if form.key_only.is_some() && !form.shows(i) {
            continue;
        }
        if !form.shows(i) {
            continue;
        }
        let editing = focused && form.editing.is_some();
        let (value, caret) = value(row, form, texts, focused, value_width);
        let mut spans = Vec::new();
        let label = label_of(row);
        let pad = column.saturating_sub(label.width() + 2);
        // 选中的那一项名字加粗、用正文色，和没选中的暗字分得开。
        let style = if focused {
            Style::new().add_modifier(Modifier::BOLD)
        } else {
            theme::hover()
        };
        spans.push(Span::styled(format!("  {label}{}", " ".repeat(pad)), style));
        let start: usize = spans.iter().map(|s| s.content.width()).sum();
        spans.extend(value);
        let (source, style) = source(row, texts);
        let used: usize = spans.iter().map(|s| s.content.width()).sum();
        let right = usize::from(width.saturating_sub(4));
        if !source.is_empty() && used + source.width() + 2 <= right {
            spans.push(Span::raw(" ".repeat(right - used - source.width())));
            spans.push(Span::styled(source, style));
        }
        if editing {
            let x = u16::try_from(start)
                .unwrap_or(u16::MAX)
                .saturating_add(caret);
            lines.caret = Some((lines.body.len(), x));
        }
        lines.target(Line::from(spans), focused, Hit::FormRow(i));
    }
    lines.error.clone_from(&form.error);
    // 只填 key 的窗不放按钮：回车存、Esc 关。
    if form.key_only.is_none() {
        lines.buttons = texts.buttons.iter().map(|b| (b.clone(), false)).collect();
        lines.keys = texts.button_keys.to_vec();
    }
    lines.button = match form.focus {
        Focus::Button(b) => Some(b),
        Focus::Row(_) => None,
    };
    let which = usize::from(form.editing.is_some());
    lines.hints.clone_from(&texts.popup_hints[which]);
}

/// 一行筛的字（选模型、目录窗的第一行）。
fn search_head(lines: &mut Lines, (search, typing): (&str, bool), texts: &ModelTexts) {
    if typing {
        lines.caret = Some((0, 3 + u16::try_from(search.width()).unwrap_or(u16::MAX)));
    }
    lines.plain(if typing || !search.is_empty() {
        Line::raw(format!(" / {search}"))
    } else {
        Line::styled(format!(" {}", texts.pick[1]), theme::dim())
    });
}

fn pick_lines(lines: &mut Lines, pick: &Pick, models: &Models, texts: &ModelTexts, width: u16) {
    let vision = pick.usage == Use::Vision;
    let sub = if vision {
        texts.pick[0].clone()
    } else {
        String::new()
    };
    lines.title(texts.uses[usize::from(vision)].clone(), sub);
    search_head(lines, (&pick.search, pick.typing), texts);
    let current = match pick.usage {
        Use::Chat => models.data.chat.clone(),
        Use::Vision => models.data.vision.clone(),
    };
    let right = usize::from(width.saturating_sub(4));
    for (i, item) in pick.items(&models.data).into_iter().enumerate() {
        match item {
            Item::Head(name) => {
                lines.plain(Line::styled(format!(" {name}"), theme::md_heading()));
            }
            Item::Choice {
                reference,
                label,
                tag,
            } => {
                let mark = if current.as_deref() == Some(reference.as_str()) {
                    "● ".to_string()
                } else {
                    "  ".to_string()
                };
                let tag = match tag {
                    Choice::Model(window, sees) => {
                        let mut parts: Vec<String> = window.map(window_text).into_iter().collect();
                        if sees {
                            parts.push(texts.tags[0].clone());
                        }
                        parts.join(" ")
                    }
                };
                let text = fit(&label, right.saturating_sub(tag.width() + 6));
                let used = 2 + mark.width() + text.width();
                let pad = right.saturating_sub(used + tag.width());
                let line = Line::from(vec![
                    Span::raw("  "),
                    Span::styled(mark, theme::accent()),
                    Span::raw(text),
                    Span::raw(" ".repeat(pad)),
                    Span::styled(tag, theme::dim()),
                ]);
                lines.target(line, i == pick.sel, Hit::PickRow(i));
            }
        }
    }
    lines.hints.clone_from(&texts.popup_hints[2]);
}

/// 目录窗：标题同编辑窗的「添加供应商」，最上面一行筛的字，下面一家一行（接不上的暗着），接着「更多供应商…」「自定义…」，
/// 机器上有 Miyu 的最后一行「从 Miyu 导入…」（暗色接一句它做什么）。
fn catalog_lines(
    lines: &mut Lines,
    pick: &CatalogPick,
    texts: &ModelTexts,
    (width, import): (u16, &str),
) {
    lines.title(texts.titles[0].clone(), String::new());
    search_head(lines, (&pick.search, pick.typing), texts);
    let rows = pick.rows();
    let empty = !rows.iter().any(|r| matches!(r, CatalogRow::Provider(_)));
    if empty && !pick.search.is_empty() {
        lines.plain(Line::styled(format!(" {}", texts.catalog[3]), theme::dim()));
    }
    let room = usize::from(width.saturating_sub(6));
    for (i, row) in rows.iter().enumerate() {
        let (label, style) = match row {
            CatalogRow::Provider(p) if p.supported => (p.name.as_str(), Style::new()),
            CatalogRow::Provider(p) => (p.name.as_str(), theme::faint()),
            CatalogRow::More => (texts.catalog[0].as_str(), Style::new()),
            CatalogRow::Custom => (texts.catalog[1].as_str(), Style::new()),
            CatalogRow::Import => (texts.catalog[4].as_str(), Style::new()),
        };
        let mut spans = vec![Span::styled(format!("  {}", fit(label, room)), style)];
        if matches!(row, CatalogRow::Import) && !import.is_empty() {
            let left = room.saturating_sub(label.width() + 2);
            spans.push(Span::styled(
                format!("  {}", fit(import, left)),
                theme::dim(),
            ));
        }
        lines.target(Line::from(spans), i == pick.sel, Hit::PickRow(i));
    }
    lines.hints.clone_from(&texts.popup_hints[2]);
}

/// 一项的值写成几段字，和正在改时光标在值里的第几列。
fn value(
    row: &Row,
    form: &Form,
    texts: &ModelTexts,
    focused: bool,
    width: usize,
) -> (Vec<Span<'static>>, u16) {
    let dim = theme::dim();
    let secret_mark = "•";
    match &row.val {
        Val::Text { text, secret } => {
            if let Some(editor) = form.editing.as_ref().filter(|_| focused) {
                let typed = editor.text();
                let shown = if *secret {
                    secret_mark.repeat(typed.chars().count())
                } else {
                    typed.to_string()
                };
                let before = &typed[..editor.cursor().min(typed.len())];
                let at = if *secret {
                    before.chars().count()
                } else {
                    before.width()
                };
                return (
                    vec![Span::raw(shown)],
                    u16::try_from(at).unwrap_or(u16::MAX),
                );
            }
            if !text.is_empty() {
                let shown = if *secret {
                    secret_mark.repeat(8)
                } else {
                    fit(text, width)
                };
                return (vec![Span::raw(shown)], 0);
            }
            let hint = match row.inherit.as_deref() {
                Some(name @ ("key_set" | "key_unset")) => {
                    texts.hint(name).unwrap_or(name).to_string()
                }
                Some(inherited) => inherited.to_string(),
                None => texts
                    .hint(row.field.name())
                    .or(texts.hint("empty"))
                    .unwrap_or_default()
                    .to_string(),
            };
            (vec![Span::styled(fit(&hint, width), dim)], 0)
        }
        Val::Choice { options, at } => {
            let current = at.map_or("", |i| options[i].as_str());
            let label = if row.field == Field::Effort && current.is_empty() {
                texts.option("effort_default")
            } else {
                texts.option(current)
            };
            let style = if at.is_none() { dim } else { Style::new() };
            if focused {
                let arrow = theme::accent();
                let spans = vec![
                    Span::styled("‹ ", arrow),
                    Span::styled(label.to_string(), style),
                    Span::styled(" ›", arrow),
                ];
                (spans, 0)
            } else {
                (vec![Span::styled(label.to_string(), style)], 0)
            }
        }
        Val::Multi { options, on } => {
            let inherited = row.inherit.clone().unwrap_or_default();
            let mut spans = Vec::new();
            for (j, option) in options.iter().enumerate() {
                let ticked = match on {
                    Some(on) => on[j],
                    None => inherited.split(',').any(|h| h == option),
                };
                let mut style = if on.is_none() { dim } else { Style::new() };
                if focused && form.cursor == j {
                    style = style.add_modifier(Modifier::UNDERLINED);
                }
                let mark = if ticked { "[x] " } else { "[ ] " };
                spans.push(Span::styled(
                    format!("{mark}{}", texts.option(option)),
                    style,
                ));
                spans.push(Span::raw("  "));
            }
            (spans, 0)
        }
        Val::Fixed(text) => {
            let shown = texts.hint(text).unwrap_or(text).to_string();
            (vec![Span::styled(fit(&shown, width), dim)], 0)
        }
    }
}

/// 右边写的来源：自己写了的黄字「自定义」，继承的暗着写从哪来。
fn source(row: &Row, texts: &ModelTexts) -> (String, Style) {
    if row.own() && row.source.is_some() {
        return (texts.source("own").to_string(), theme::warn());
    }
    match &row.source {
        Some(from) => (texts.source(from).to_string(), theme::dim()),
        None => (String::new(), Style::new()),
    }
}
