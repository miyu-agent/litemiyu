//! 画「供应商和模型」（Miyu 终端 `ui/settings/mod.rs` 的 `models`、`tabs`、`footer`、`status`，蓝图「配置页」第 3 到 10 条）：
//! 面包屑、分页、几栏、状态行、细线、两行按键提示（第二行右边写第几个）；悬浮窗盖在上面（`model_popups.rs`）。

use ratatui::layout::{Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use super::super::frame::Frame;
use super::columns;
use super::floating::hint_line_within;
use crate::settings::models::Models;
use crate::settings::models::nav::{PAGES, Page};
use crate::settings::models::texts::ModelTexts;
use crate::settings::{Hit, Hits, Settings, Tone};
use crate::theme;

/// 画这一页；在筛的交回输入光标的位置。
pub fn draw(
    frame: &mut Frame,
    stage: Rect,
    settings: &Settings,
    models: &Models,
) -> Option<Position> {
    let texts = &settings.texts.models;
    super::crumb(frame, stage, &settings.texts.title, Some(&texts.entry));
    if stage.height < 8 {
        return None;
    }
    tabs(
        frame,
        Rect::new(stage.x, stage.y + 2, stage.width, 1),
        (models, &settings.hits),
        texts,
    );
    let body = Rect::new(
        stage.x,
        stage.y + 4,
        stage.width,
        stage.height.saturating_sub(8),
    );
    columns::draw(frame, body, (models, &settings.hits), texts);
    let hints = match models.nav.page {
        Page::Providers => &texts.key_hints[1],
        Page::Defaults => &texts.key_hints[2],
    };
    let col = models.nav.focus(&models.data);
    let len = models.nav.len(col, &models.data);
    let counter = format!(
        "{}/{len}",
        if len == 0 {
            0
        } else {
            models.nav.selected(col) + 1
        }
    );
    let typing = models.nav.search.as_ref().is_some_and(|s| s.typing);
    let first = if typing { &texts.key_hints[4] } else { hints };
    let second = (!typing).then_some(texts.key_hints[3].as_slice());
    footer(
        frame,
        stage,
        status(settings, models, texts),
        (first, second),
        Some(counter),
    );
    models
        .nav
        .search
        .as_ref()
        .filter(|s| s.typing)
        .map(|search| {
            let y = stage.bottom().saturating_sub(4);
            let x = stage.x + 2 + u16::try_from(search.text.width()).unwrap_or(u16::MAX);
            Position::new(x.min(stage.right().saturating_sub(1)), y)
        })
}

/// 分页：当前页铺强调色的底、加粗（Miyu 2026-10-07：只画下划线三页看不出哪页是哪页）。
fn tabs(frame: &mut Frame, row: Rect, (models, hits): (&Models, &Hits), texts: &ModelTexts) {
    let mut x = row.x;
    let current = PAGES.iter().position(|p| *p == models.nav.page);
    let sep = crate::sheet::sep();
    let sep_w = u16::try_from(sep.width()).unwrap_or(3);
    for (i, name) in texts.pages.iter().enumerate() {
        let style = if current == Some(i) {
            theme::picked_bar().add_modifier(Modifier::BOLD)
        } else {
            theme::dim()
        };
        let label = format!(" {name} ");
        let width = u16::try_from(label.width()).unwrap_or(u16::MAX);
        if x + width > row.right() {
            break;
        }
        frame.buffer_mut().set_string(x, row.y, &label, style);
        hits.push(Rect::new(x, row.y, width, 1), Hit::Tab(i));
        x += width;
        if i + 1 < texts.pages.len() && x + sep_w <= row.right() {
            frame
                .buffer_mut()
                .set_string(x, row.y, &sep, theme::faint());
            x += sep_w;
        }
    }
}

/// 下面四行：状态行、细线、一两行按键提示（最后一行右边写第几个）。
pub fn footer(
    frame: &mut Frame,
    stage: Rect,
    (text, style): (String, Style),
    (first, second): (&[[String; 2]], Option<&[[String; 2]]>),
    counter: Option<String>,
) {
    if stage.height < 4 {
        return;
    }
    let bottom = stage.bottom();
    let buf = frame.buffer_mut();
    buf.set_line(
        stage.x,
        bottom - 4,
        &Line::from(vec![Span::styled(text, style)]),
        stage.width,
    );
    let rule = "─".repeat(usize::from(stage.width));
    buf.set_string(stage.x, bottom - 3, &rule, theme::faint());
    let (hint_y, second_y) = if second.is_some() {
        (bottom - 2, bottom - 1)
    } else {
        (bottom - 2, bottom - 2)
    };
    // 「第几个/一共几个」写在最后一行右边：那一行的提示给它让出地方（再空两格）。
    let room = |y: u16| {
        let taken = counter
            .as_ref()
            .filter(|_| y == second_y)
            .map_or(0, |c| c.width() + 2);
        usize::from(stage.width).saturating_sub(taken)
    };
    buf.set_line(
        stage.x,
        hint_y,
        &hint_line_within(first, room(hint_y)),
        stage.width,
    );
    if let Some(second) = second {
        let line = hint_line_within(second, room(second_y));
        buf.set_line(stage.x, second_y, &line, stage.width);
    }
    if let Some(counter) = counter {
        let w = u16::try_from(counter.width()).unwrap_or(u16::MAX);
        if w < stage.width {
            buf.set_string(stage.right() - w, second_y, &counter, theme::dim());
        }
    }
}

/// 状态行的颜色（Miyu：平常的黄、成了绿、出错红、正在办暗）。LiteMiyu 试一家时写强调色，看得出在动。
pub fn tone(tone: Tone) -> Style {
    match tone {
        Tone::Note => theme::warn(),
        Tone::Good => theme::good(),
        Tone::Bad => theme::error(),
        Tone::Busy => theme::dim(),
    }
}

/// 状态行左边写什么：在筛的写筛的字；有话的写话；没有的照这一页写模型、池有几个。
fn status(settings: &Settings, models: &Models, texts: &ModelTexts) -> (String, Style) {
    if let Some(search) = &models.nav.search {
        let style = if search.typing {
            Style::new()
        } else {
            theme::dim()
        };
        return (format!("/ {}", search.text), style);
    }
    if let Some((text, said)) = &settings.status {
        return (text.clone(), tone(*said));
    }
    (models.quiet(texts), theme::warn())
}
