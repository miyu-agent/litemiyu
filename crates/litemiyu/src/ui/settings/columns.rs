//! 三页的几栏（Miyu 终端 `ui/settings/columns.rs`，蓝图「配置页」第 7 到 10、21、22 条）：栏头、一行行（右边暗着写数目），
//! 默认模型页右边的详情。

use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use super::super::frame::Frame;
use super::floating::{bar, fit};
use crate::settings::models::Models;
use crate::settings::models::forms::window_text;
use crate::settings::models::nav::{Col, Org, USES, Use, orgs, shown_name};
use crate::settings::models::texts::ModelTexts;
use crate::settings::{Hit, Hits};
use crate::theme;

/// 栏宽（Miyu `layout.json` 的 `settings`）。
const PROVIDER_WIDTH: u16 = 24;
const ORG_WIDTH: u16 = 15;
const USE_WIDTH: u16 = 30;
const COLUMN_GAP: u16 = 3;

/// 一行：左边的字、右边暗着写的数目。
struct Item {
    text: String,
    tag: String,
}

/// 焦点那一栏至少要露出几列（栏头、选中的那一行看得出是哪一个）。
const MIN_FOCUSED: u16 = 12;

/// 画这一页的几栏。
pub fn draw(frame: &mut Frame, body: Rect, (models, hits): (&Models, &Hits), texts: &ModelTexts) {
    let cols = models.nav.cols(&models.data);
    let focus = models.nav.focus(&models.data);
    // 窄窗口放不下焦点那一栏的：从左边去掉几栏，焦点那一栏露得出来（Miyu 照左起画，窄于 60 列左右时模型栏画不出来、焦点落在
    // 看不见的栏上；这里多了这一步）。
    let fixed = |col: &Col| match col {
        Col::Provider => PROVIDER_WIDTH,
        Col::Org => ORG_WIDTH,
        Col::Use => USE_WIDTH,
        Col::Model => 0,
    };
    let at = cols.iter().position(|c| *c == focus).unwrap_or(0);
    let start = (0..=at)
        .find(|&from| {
            let before: u16 = cols[from..at]
                .iter()
                .map(|c| fixed(c).saturating_add(COLUMN_GAP))
                .sum();
            before.saturating_add(MIN_FOCUSED) <= body.width
        })
        .unwrap_or(at);
    let mut x = body.x;
    for (i, col) in cols.iter().enumerate().skip(start) {
        let last = i + 1 == cols.len() && *col != Col::Use;
        let width = match col {
            Col::Provider => PROVIDER_WIDTH,
            Col::Org => ORG_WIDTH,
            // 窄窗口里用途一栏让出一半给右边的详情（Miyu 照固定宽，详情挤得只剩几个字）。
            Col::Use => USE_WIDTH.min(body.width / 2),
            Col::Model => body.right().saturating_sub(x),
        };
        let width = if last {
            body.right().saturating_sub(x)
        } else {
            width.min(body.right().saturating_sub(x))
        };
        if width < 4 {
            break;
        }
        let area = Rect::new(x, body.y, width, body.height);
        let (title, items, empty) = items(*col, models, texts);
        column(
            frame,
            area,
            (models, hits),
            *col,
            (*col == focus, &title),
            &items,
            &empty,
        );
        x += width + COLUMN_GAP;
    }
    if focus == Col::Use && x < body.right() {
        let area = Rect::new(
            x,
            body.y + 2,
            body.right() - x,
            body.height.saturating_sub(2),
        );
        detail(frame, area, models, texts);
    }
}

/// 一栏的栏头、行、空着时写的。模型、用途那几栏右边不写东西（Miyu 2026-10-07：窗口、「图」没必要）。
fn items(col: Col, models: &Models, texts: &ModelTexts) -> (String, Vec<Item>, String) {
    let view = &models.data;
    let nav = &models.nav;
    let filtered = nav
        .search
        .as_ref()
        .is_some_and(|s| s.col == Some(col) && !s.text.is_empty());
    let empty = texts.empty[usize::from(filtered)].clone();
    let item = |text: String, tag: String| Item { text, tag };
    let title = |i: usize| texts.cols[i].clone();
    match col {
        Col::Provider => {
            let rows = nav.providers(view).into_iter();
            let rows = rows.map(|p| item(p.shown().to_string(), p.models.len().to_string()));
            (title(0), rows.collect(), empty)
        }
        Col::Org => {
            let orgs = nav.provider(view).map(orgs).unwrap_or_default();
            let rows = orgs.into_iter().map(|(o, n)| {
                let text = match o {
                    Org::All => texts.orgs[0].clone(),
                    Org::Named(name) => name,
                    Org::Other => texts.orgs[1].clone(),
                };
                item(text, n.to_string())
            });
            (title(1), rows.collect(), empty)
        }
        Col::Model => {
            let org = nav.org(view);
            let rows = nav.models(view).into_iter();
            let rows = rows.map(|m| item(shown_name(m, org.as_ref()).to_string(), String::new()));
            // 用不了的一家说成人话、告诉怎么办：没地址的填地址。
            let empty = match nav.provider(view).filter(|p| p.models.is_empty()) {
                Some(p) => crate::settings::models::no_models(p, texts),
                None => empty,
            };
            (title(2), rows.collect(), empty)
        }
        Col::Use => {
            let rows = USES
                .iter()
                .enumerate()
                .map(|(i, _)| item(texts.uses[i].clone(), String::new()));
            (title(3), rows.collect(), empty)
        }
    }
}

/// 画一栏：栏头（焦点栏强调色加粗），空一行，下面一行行，照选中的那行滚。
fn column(
    frame: &mut Frame,
    area: Rect,
    (models, hits): (&Models, &Hits),
    col: Col,
    (focused, title): (bool, &str),
    items: &[Item],
    empty: &str,
) {
    let head = if focused {
        theme::accent().add_modifier(Modifier::BOLD)
    } else {
        theme::dim()
    };
    let inner = usize::from(area.width.saturating_sub(2));
    frame
        .buffer_mut()
        .set_string(area.x + 2, area.y, fit(title, inner), head);
    let rows = usize::from(area.height.saturating_sub(2));
    if rows == 0 {
        return;
    }
    if items.is_empty() {
        frame
            .buffer_mut()
            .set_string(area.x + 2, area.y + 2, fit(empty, inner), theme::dim());
        return;
    }
    let sel = models.nav.selected(col).min(items.len() - 1);
    let top = (sel + 1).saturating_sub(rows);
    for (offset, item) in items.iter().skip(top).take(rows).enumerate() {
        let y = area.y + 2 + u16::try_from(offset).unwrap_or(u16::MAX);
        let rect = Rect::new(area.x, y, area.width, 1);
        if top + offset == sel {
            let ground = if focused {
                theme::row_focus()
            } else {
                theme::row_dim()
            };
            frame.buffer_mut().set_style(rect, ground);
            if focused {
                bar(frame, rect);
            }
        }
        row(frame, rect, item);
        hits.push(rect, Hit::Row(col, top + offset));
    }
}

/// 一行的字，右边暗着写的靠右。
fn row(frame: &mut Frame, rect: Rect, item: &Item) {
    let inner = usize::from(rect.width.saturating_sub(3));
    // 右边的字放不下时截掉加「…」，名字至少留一半。
    let tag = if item.tag.width() + 1 + item.text.width().min(inner / 2) > inner {
        fit(
            &item.tag,
            inner.saturating_sub(item.text.width().min(inner / 2) + 1),
        )
    } else {
        item.tag.clone()
    };
    let tag_width = tag.width();
    let room = inner.saturating_sub(if tag_width > 0 { tag_width + 1 } else { 0 });
    let line = Line::from(Span::raw(fit(&item.text, room)));
    frame
        .buffer_mut()
        .set_line(rect.x + 2, rect.y, &line, rect.width.saturating_sub(3));
    if tag_width > 0 && tag_width < inner {
        let x = rect
            .right()
            .saturating_sub(1 + u16::try_from(tag_width).unwrap_or(u16::MAX));
        frame.buffer_mut().set_string(x, rect.y, &tag, theme::dim());
    }
}

/// 默认模型页右边：现在用的是什么；模型的写窗口、能收什么；用途一句。
fn detail(frame: &mut Frame, area: Rect, models: &Models, texts: &ModelTexts) {
    let view = &models.data;
    let usage = models.nav.usage();
    let at = USES.iter().position(|u| *u == usage).unwrap_or(0);
    let used = match usage {
        Use::Chat => view.chat.clone(),
        Use::Vision => view.vision.clone(),
    };
    let mut lines: Vec<(String, Line)> = Vec::new();
    let reference = used.unwrap_or_default();
    let model = view.model(&reference).map(|(_, m)| m);
    let gone = !reference.is_empty() && model.is_none();
    let now = if gone { theme::error() } else { Style::new() };
    // 值对不上任何一个列着的模型：那一家报得出自己的列表的，是供应商已经不提供这个模型了（LM-50）；别的（那一家没配、
    // 报不出列表又哪儿都没写）照旧写「不存在」。
    let withdrawn = reference
        .split_once('/')
        .and_then(|(id, _)| view.provider(id))
        .is_some_and(|provider| provider.reported);
    let shown = match (gone, withdrawn) {
        (true, true) => format!("{reference}{}", texts.withdrawn),
        (true, false) => format!("{reference}{}", texts.gone),
        (false, _) => reference.clone(),
    };
    lines.push((texts.detail[0].clone(), Line::styled(shown, now)));
    if let Some(m) = model {
        lines.push((
            texts.detail[1].clone(),
            Line::raw(m.window().map(window_text).unwrap_or_default()),
        ));
        let inputs: Vec<String> = m
            .inputs()
            .iter()
            .map(|i| texts.option(i).to_string())
            .collect();
        lines.push((texts.detail[2].clone(), Line::raw(inputs.join("、"))));
    }
    lines.push((
        texts.detail[3].clone(),
        Line::styled(texts.use_notes[at].clone(), theme::dim()),
    ));
    put_lines(frame, area, lines);
}

/// 左边暗色的名字、右边的字，一行一样。
fn put_lines(frame: &mut Frame, area: Rect, lines: Vec<(String, Line)>) {
    let label_width = lines.iter().map(|(l, _)| l.width()).max().unwrap_or(0);
    let label_width = u16::try_from(label_width)
        .unwrap_or(u16::MAX)
        .saturating_add(2);
    for (i, (label, line)) in lines.into_iter().enumerate() {
        let y = area.y + u16::try_from(i).unwrap_or(u16::MAX);
        if y >= area.bottom() {
            break;
        }
        frame
            .buffer_mut()
            .set_string(area.x, y, &label, theme::dim());
        let width = area.width.saturating_sub(label_width);
        frame
            .buffer_mut()
            .set_line(area.x + label_width, y, &line, width);
    }
}
