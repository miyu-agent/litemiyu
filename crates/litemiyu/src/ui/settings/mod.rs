//! 画配置界面（Miyu 终端 `ui/settings/`，蓝图「配置页」第 3 到 7 条）：整块居中占八成、窄于 100 列的用满宽，不画边框；
//! 面包屑、这一页的内容、状态行、细线、两行按键提示，右下角写选中的是第几个；窗盖在上面。只画，不改数据。

mod columns;
mod extras;
pub(in crate::ui) mod floating;
mod model_popups;
mod models;
mod opening;
mod popup;
mod presets;

use ratatui::layout::{Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use super::frame::Frame;
use super::rows::clip;
use crate::settings::{Entry, Extra, Hit, Popup, Row, Settings, Target, View, Window};
use crate::theme;

/// 画一帧；编辑窗开着时交回光标的位置。
pub fn draw(frame: &mut Frame, settings: &Settings) -> Option<Position> {
    settings.hits.clear();
    // 下一个窗从头露起（Miyu `ui/settings/mod.rs`：没有窗开着时 `popup_top` 归零；窗里照上一帧接着露，`floating.rs`）。
    if settings.popup.is_none() && settings.models.as_ref().is_none_or(|m| m.popup.is_none()) {
        settings.hits.set_top(0);
    }
    if let Some(open) = &settings.opening {
        return opening::draw(frame, settings, open);
    }
    let area = block(frame.area());
    if area.height < 6 || area.width < 20 {
        return None;
    }
    let texts = &settings.texts;
    if let (View::Page { page, .. }, Some(models)) = (settings.view, &settings.models)
        && settings.is_models(page)
    {
        let caret = models::draw(frame, area, settings, models);
        let caret = model_popups::draw(
            frame,
            (models, &settings.hits),
            &texts.models,
            &texts.opening.import[1],
        )
        .or(caret);
        return settings
            .popup
            .as_ref()
            .and_then(|open| {
                popup::draw(frame, area, (open, &settings.hits), (texts, vim(settings)))
            })
            .or(caret);
    }
    // 面包屑（Miyu `crumb`）：主菜单 `● 配置` 强调色加粗；进了页「配置 —」暗色，`◉ 页名` 强调色、页名加粗。
    let mut crumb = vec![Span::styled("● ", theme::accent())];
    let inside = matches!(settings.view, View::Page { .. });
    crumb.push(Span::styled(
        texts.title.clone(),
        if inside {
            theme::dim()
        } else {
            theme::accent().add_modifier(Modifier::BOLD)
        },
    ));
    // 内容、要露出来的那一行、右下角的第几个和一共几个、按键提示、光标那一项的说明（LM-38）。
    let (lines, focus, (index, count), hints, about) = match settings.view {
        View::Menu(at) => (
            menu_lines(settings, at, area.width),
            at * 2 + 1,
            (at, settings.entries()),
            &texts.key_hints.menu,
            None,
        ),
        View::Page { page, at } => {
            let name = match (settings.page_of(page), settings.extra(page)) {
                (Some(p), _) => Some(settings.schema.pages[p].name.clone()),
                (None, Some(extra)) => Some(extra_name(settings, extra)[0].clone()),
                (None, None) => None,
            };
            if let Some(name) = name {
                crumb.push(Span::styled(" — ", theme::dim()));
                crumb.push(Span::styled("◉ ", theme::accent()));
                crumb.push(Span::styled(
                    name,
                    theme::accent().add_modifier(Modifier::BOLD),
                ));
            }
            if settings.extra(page) == Some(Extra::Presets) {
                let drawn = presets::lines(settings, at, area.width);
                (
                    drawn.lines,
                    drawn.focus,
                    drawn.place,
                    &texts.presets.hints,
                    None,
                )
            } else if let Some(extra) = settings.extra(page) {
                let count = settings.extra_list.entries.len();
                let (lines, cut) = extras::lines(settings, extra, at, area.width);
                (lines, at, (at, count), &texts.extras.hints, cut)
            } else {
                let rows = settings.rows(page);
                let items = |rows: &[Row]| {
                    rows.iter()
                        .filter(|row| !matches!(row, Row::Group(_)))
                        .count()
                };
                let index = items(&rows[..at.min(rows.len())]);
                let lines = page_lines(settings, &rows, at, area.width);
                // 组名之间空一行：选中的那一行在第几行照画出来的数。
                let focus = rows[..at.min(rows.len())]
                    .iter()
                    .enumerate()
                    .filter(|(i, row)| *i > 0 && matches!(row, Row::Group(_)))
                    .count()
                    + at;
                let about = match rows.get(at) {
                    Some(Row::Item(item)) => {
                        Some(settings.schema.items[*item].words.description.clone())
                    }
                    Some(Row::Prompt) => Some(texts.prompt.description.clone()),
                    _ => None,
                };
                (
                    lines,
                    focus,
                    (index, items(&rows)),
                    &texts.key_hints.page,
                    about,
                )
            }
        }
    };
    let buffer = frame.buffer_mut();
    buffer.set_line(area.x, area.y, &Line::from(crumb), area.width);
    // 内容在面包屑下空一行，到状态行上面。
    let body = Rect {
        x: area.x,
        y: area.y + 2,
        width: area.width,
        height: area.height.saturating_sub(6),
    };
    // 光标那一项的说明（LM-38，同 Miyu 终端 11-20 推荐的改法）：列表下面空一行，暗色折行写全；列表连说明放不下（窗口矮）的
    // 不写说明，列表照旧占满。窗开着时（选择窗、编辑窗、预设的窗）不写：窗自己有标题，页在后面调暗。
    let windowed = settings.popup.is_some() || settings.preset_window_open();
    let about = about
        .filter(|text| !text.is_empty() && !windowed)
        .map(|text| about_rows(&text, area.width))
        .filter(|rows| lines.len() + 1 + rows.len() <= usize::from(body.height));
    if let Some(rows) = &about {
        let top = body.y + u16::try_from(lines.len() + 1).unwrap_or(u16::MAX);
        for (i, row) in rows.iter().enumerate() {
            let y = top + u16::try_from(i).unwrap_or(u16::MAX);
            buffer.set_line(
                body.x,
                y,
                &Line::styled(row.clone(), theme::dim()),
                body.width,
            );
        }
    }
    // 主菜单照光标滚（矮窗口能滚，有意不同，B5）；一页里照上一帧的顶接着露、选中的碰到上下边才挪（Miyu `pages.rs` 的 `paint`
    // 用 `menu::edge`）。
    let skip = match settings.view {
        View::Menu(_) => focus.saturating_sub(usize::from(body.height).saturating_sub(2)),
        View::Page { page, .. } => {
            let (was, top) = settings.page_top.get();
            let prev = if was == page { top } else { 0 };
            let top = crate::menu::edge(focus, None, prev, lines.len(), usize::from(body.height));
            settings.page_top.set((page, top));
            top
        }
    };
    for (i, line) in lines
        .iter()
        .skip(skip)
        .take(usize::from(body.height))
        .enumerate()
    {
        let y = body.y + u16::try_from(i).unwrap_or(u16::MAX);
        buffer.set_line(body.x, y, line, body.width);
        // 主菜单一项两行，点哪一行都是这一项（Miyu `Hit::Menu`）。
        if matches!(settings.view, View::Menu(_)) {
            let width = u16::try_from(line.width())
                .unwrap_or(u16::MAX)
                .min(body.width);
            let rect = Rect::new(body.x, y, width, 1);
            settings.hits.push(rect, Hit::Menu((skip + i) / 2));
        }
    }
    let bottom = area.y + area.height;
    // 状态行：有话写话，没话的不写（LM-38：原来照 Miyu 在别的页也写「供应商和模型」那一页选中那一家的「暂无模型 · 保存后
    // 按 r 获取」「已获取 N 个模型」，那几页按 r 什么都不做，同 Miyu 终端 11-20 项目主人报的「设置页底下一直写」）。
    if let Some((said, tone)) = &settings.status {
        let (said, style) = (said.as_str(), models::tone(*tone));
        buffer.set_line(
            area.x,
            bottom - 4,
            &Line::styled(clip(said, area.width), style),
            area.width,
        );
    }
    let rule = "─".repeat(usize::from(area.width));
    buffer.set_line(
        area.x,
        bottom - 3,
        &Line::styled(rule, theme::faint()),
        area.width,
    );
    // 窗开着时底下照旧是这一页的按键提示和第几个（同 Miyu：窗自己的按键提示写在窗里）。
    buffer.set_line(area.x, bottom - 2, &hint_line(hints), area.width);
    // 右下角第几个，和按键提示同一行（Miyu `pages.rs` 的 `draw` 交给 `footer` 的 `counter`）；一项都没有的写「0/0」；主菜单不写。
    if inside {
        let place = format!(
            "{}/{count}",
            if count == 0 {
                0
            } else {
                index.min(count - 1) + 1
            }
        );
        let width = u16::try_from(place.width()).unwrap_or(0);
        let x = area.x + area.width.saturating_sub(width);
        buffer.set_line(x, bottom - 2, &Line::styled(place, theme::dim()), width);
    }
    // 预设的窗浮在页上（`presets.rs`）；改名字的编辑窗不另画，字写在窗里名字那一行（同 Miyu）。别的窗（删除先问的）盖在它上面。
    let renaming = settings.popup.as_ref().and_then(|open| match open {
        Popup {
            window: Window::Edit(edit),
            target: Target::PresetRename(_),
        } => Some(edit),
        _ => None,
    });
    let caret = presets::window(frame, settings, renaming, vim(settings));
    if renaming.is_some() && settings.preset_window_open() {
        return caret;
    }
    settings
        .popup
        .as_ref()
        .and_then(|open| popup::draw(frame, area, (open, &settings.hits), (texts, vim(settings))))
        .or(caret)
}

/// 整块的位置（Miyu `stage`，`layout.json` 的 `settings`）：宽八成、高八成六居中；窄于 100 列的用满宽，四边各留一格。
fn block(screen: Rect) -> Rect {
    if screen.width < 100 {
        return Rect::new(
            screen.x + screen.width.min(1),
            screen.y + screen.height.min(1),
            screen.width.saturating_sub(2),
            screen.height.saturating_sub(2),
        );
    }
    let width = screen.width * 80 / 100;
    let height = screen.height * 86 / 100;
    Rect::new(
        screen.x + (screen.width - width) / 2,
        screen.y + (screen.height - height) / 2,
        width,
        height,
    )
}

/// 面包屑（Miyu `crumb`）：`● 配置`，进了一页再接 ` — ◉ 页名`。
fn crumb(frame: &mut Frame, stage: Rect, title: &str, inside: Option<&str>) {
    let mut spans = vec![Span::styled("● ", theme::accent())];
    if let Some(name) = inside {
        spans.push(Span::styled(title.to_string(), theme::dim()));
        spans.push(Span::styled(" — ", theme::dim()));
        spans.push(Span::styled("◉ ", theme::accent()));
        spans.push(Span::styled(
            name.to_string(),
            theme::accent().add_modifier(Modifier::BOLD),
        ));
    } else {
        spans.push(Span::styled(
            title.to_string(),
            theme::accent().add_modifier(Modifier::BOLD),
        ));
    }
    frame
        .buffer_mut()
        .set_line(stage.x, stage.y, &Line::from(spans), stage.width);
}

/// 技能页、MCP 页在主菜单里的名字和底下那一行。
fn extra_name(settings: &Settings, extra: Extra) -> &[String; 2] {
    match extra {
        Extra::Presets => &settings.texts.presets.entry,
        Extra::Skills => &settings.texts.extras.skills,
        Extra::Mcp => &settings.texts.extras.mcp,
    }
}

/// 主菜单：一页两行，名字和暗色的一行组名（这一页画出来的几组）；「供应商和模型」、技能页、MCP 页底下那一行写它管什么。
fn menu_lines(settings: &Settings, at: usize, width: u16) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let pages = settings
        .layout()
        .into_iter()
        .enumerate()
        .map(|(i, entry)| match entry {
            Entry::Extra(extra) => {
                let [name, below] = extra_name(settings, extra).clone();
                (name, below)
            }
            Entry::Page(p) if settings.schema.pages[p].id == "models" => {
                let words = &settings.texts.models;
                (words.entry.clone(), words.entry_note.clone())
            }
            Entry::Page(p) => {
                let groups: Vec<&str> = settings
                    .rows(i)
                    .into_iter()
                    .filter_map(|row| match row {
                        Row::Group(group) => Some(settings.schema.groups[group].name.as_str()),
                        _ => None,
                    })
                    .collect();
                (
                    settings.schema.pages[p].name.clone(),
                    groups.join(&settings.texts.separator),
                )
            }
        });
    // 一项两行：选中的两行铺选中的底色、左边强调色 `┃`；宽至少 46 列，照最长的说明放宽（Miyu `pages::menu`）。
    let pages: Vec<(String, String)> = pages.collect();
    let room = pages
        .iter()
        .map(|(name, below)| name.width().max(below.width()) + 4)
        .max()
        .unwrap_or(0)
        .max(46);
    for (i, (name, below)) in pages.into_iter().enumerate() {
        let chosen = i == at;
        for (n, (text, style)) in [(name, Style::new()), (below, theme::dim())]
            .into_iter()
            .enumerate()
        {
            // 选中的竖条；没有颜色可靠的写箭头、只写第一行（Miyu `pages.rs` 的 `menu` 调 `bar`，8a2c7d25）。
            let mut spans = vec![floating::lead(chosen, n == 0)];
            // 窄窗口放不下的截掉加「…」（同 Miyu `pages::menu` 的 `fit`）。
            let room = room.min(usize::from(width));
            let text = floating::fit(&text, room.saturating_sub(2));
            let pad = room.saturating_sub(2 + text.width());
            spans.push(Span::styled(text, style));
            spans.push(Span::raw(" ".repeat(pad)));
            let line = Line::from(spans);
            lines.push(if chosen {
                line.patch_style(theme::row_focus())
            } else {
                line
            });
        }
    }
    lines
}

/// 一页（Miyu `pages::page_lines`）：组名强调色加粗，组和组之间空一行；一项一行，名字照最长的对齐、空两格、值加粗，不写
/// 来源；选中的那一行左边强调色 `┃`、整行铺选中的底色。
fn page_lines(settings: &Settings, rows: &[Row], at: usize, width: u16) -> Vec<Line<'static>> {
    let names = rows
        .iter()
        .filter_map(|row| match row {
            Row::Item(item) => Some(settings.schema.items[*item].words.name.width()),
            Row::Prompt => Some(settings.texts.prompt.name.width()),
            _ => None,
        })
        .max()
        .unwrap_or(0);
    let mut lines = Vec::new();
    for (i, row) in rows.iter().enumerate() {
        // 名字、值（加粗）、值后面暗色的一截。
        let (name, value, after) = match row {
            Row::Group(group) => {
                if i > 0 {
                    lines.push(Line::default());
                }
                let name = settings.schema.groups[*group].name.clone();
                lines.push(Line::styled(
                    format!("  {name}"),
                    theme::accent().add_modifier(Modifier::BOLD),
                ));
                continue;
            }
            Row::Prompt => (
                settings.texts.prompt.name.clone(),
                settings.prompt_state().to_string(),
                String::new(),
            ),
            Row::Item(item) => (
                settings.schema.items[*item].words.name.clone(),
                settings.shown_value(*item),
                String::new(),
            ),
        };
        // 值太长的截掉加「…」（Miyu：`宽 − 名字宽 − 8`）。
        let value = floating::fit(&value, usize::from(width).saturating_sub(names + 2 + 8));
        let chosen = i == at;
        let mut spans = vec![floating::lead(chosen, true)];
        let pad = " ".repeat(names.saturating_sub(name.width()) + 2);
        spans.push(Span::styled(name, Style::new()));
        if !value.is_empty() || !after.is_empty() {
            spans.push(Span::raw(pad));
            spans.push(Span::styled(
                value,
                Style::new().add_modifier(Modifier::BOLD),
            ));
        }
        if !after.is_empty() {
            spans.push(Span::styled(after, theme::dim()));
        }
        let used: usize = spans.iter().map(|span| span.content.width()).sum();
        let text: String = spans.iter().map(|span| span.content.as_ref()).collect();
        if used > usize::from(width) {
            spans = vec![Span::raw(clip(&text, width))];
        } else {
            spans.push(Span::raw(" ".repeat(usize::from(width) - used)));
        }
        let line = Line::from(spans);
        lines.push(if chosen {
            line.patch_style(theme::row_focus())
        } else {
            line
        });
    }
    lines
}

/// 说明照 `width` 列折成几行，行首空两格（同组名、一项的名字）。
fn about_rows(text: &str, width: u16) -> Vec<String> {
    crate::input::pieces(text, width.saturating_sub(2).max(1))
        .into_iter()
        .map(|(piece, _)| format!("  {}", piece.trim_end()))
        .collect()
}

/// 按键模式是不是 Vim（照配置界面这一刻的值：开头里刚选的也算）。
pub(super) fn vim(settings: &Settings) -> bool {
    settings.text_value("tui.keys") != "arrows"
}

/// 按键提示（Miyu `footer`）：键暗色、说明再暗一档，组和组之间空三格。
fn hint_line(hints: &[[String; 2]]) -> Line<'static> {
    let mut spans = Vec::new();
    for [key, what] in hints {
        if !spans.is_empty() {
            spans.push(Span::raw("   "));
        }
        spans.push(Span::styled(key.clone(), theme::dim()));
        spans.push(Span::styled(format!(" {what}"), theme::faint()));
    }
    Line::from(spans)
}

#[cfg(test)]
mod tests;
