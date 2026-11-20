//! 画第一次运行的开头（Miyu 终端 `ui/oobe/stage.rs`、`pick.rs`、`theme_step.rs`、`screens.rs` 的版面）：第二行是进度（走过的
//! `●` 暗色、这一步 `◉` 强调色加粗、没到的 `○`，中间 ` ── `），内容一栏左右居中、上下居中；倒数第二行居中暗色写按键提示
//! （按键模式是 Vim 的「↑↓」写成「j/k」，Miyu `hint_keys::vim`）。完成那一屏不画进度、不写按键，正中三行：标题、摘要、
//! 「按 Enter 开始对话」（Miyu 的星点、吉祥物不搬）。配色方案那一步的选择窗照 Miyu 摆在格子下面、后面不压暗（看得见当场换成
//! 什么样）；「更多供应商…」照 Miyu 引导自己的浮窗（`popup.rs`）盖在上面；从 Miyu 导入的勾选窗照配置界面的窗盖在上面。

mod content;
mod model;
mod more;
mod popup;

use ratatui::layout::{Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use content::Content;

use crate::settings::opening::{ICON_SETS, Opening, Phase, Slot, Step, ThemeSlot};
use crate::settings::{Settings, Window};
use crate::theme;
use crate::ui::frame::Frame;

/// 内容那一栏最宽几列（Miyu `oobe.json` 的 `stage.content_width`）。
const CONTENT: u16 = 60;

/// 图标那一步中间那一行列哪几件工具的图标（Miyu `oobe.json` 的 `sample_icons`）。Miyu 那一份的 `grep`、`todowrite`
/// LiteMiyu 砍了（发版前那一轮），图标表里没有它们、画出来是通用的扳手：换成还在的 `write`、`shell`，位置、个数不变（LM-8）。
const SAMPLE_ICONS: [&str; 10] = [
    "edit",
    "read",
    "write",
    "history",
    "subagent",
    "jobs",
    "send_message",
    "sessions",
    "ask_user",
    "shell",
];

/// 试连接、在存时转轮的几帧。
const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

/// 画开头；编辑窗、格子在编辑时交回光标的位置。
pub fn draw(frame: &mut Frame, settings: &Settings, opening: &Opening) -> Option<Position> {
    let area = frame.area();
    if area.height < 8 || area.width < 20 {
        return None;
    }
    if opening.step == Step::Done {
        done(frame, area, settings, opening);
        return None;
    }
    let texts = settings.opening_texts();
    let buffer = frame.buffer_mut();
    buffer.set_line(
        area.x + 2,
        area.y + 1,
        &progress(opening, &texts.steps),
        area.width.saturating_sub(2),
    );
    let width = CONTENT.min(area.width.saturating_sub(4));
    let top_room = area.height.saturating_sub(6);
    let built = content(settings, opening, width, top_room);
    let height = built.len().min(top_room);
    let x = area.x + (area.width - width) / 2;
    let y = area.y + 3 + top_room.saturating_sub(height) / 2;
    // 放不下的照光标往下挪。
    let skip = built
        .focus
        .map_or(0, |f| (f + 2).saturating_sub(height))
        .min(built.len().saturating_sub(height));
    for (i, line) in built
        .lines
        .iter()
        .skip(usize::from(skip))
        .take(usize::from(height))
        .enumerate()
    {
        let row = y + u16::try_from(i).unwrap_or(u16::MAX);
        buffer.set_line(x, row, line, width);
    }
    let hint = crate::ui::hint_keys::vim(&hint(settings, opening), vim(settings));
    let hint_width = u16::try_from(hint.width())
        .unwrap_or(area.width)
        .min(area.width);
    // 居中照 ratatui 的 `Alignment::Center`（Miyu 用它）：半宽减半宽，奇数宽的比 `(宽 - 字宽) / 2` 靠右一格。
    buffer.set_line(
        area.x + (area.width / 2).saturating_sub(hint_width / 2),
        area.y + area.height - 2,
        &Line::styled(hint, theme::faint()),
        hint_width,
    );
    let mut caret = built.caret.and_then(|(row, col)| {
        let row = row.checked_sub(skip)?;
        (row < height).then(|| Position::new(x + col, y + row))
    });
    let block = super::block(area);
    // 「更多供应商…」（Miyu `ui/oobe/mod.rs` 的 `popup`）。
    if opening.step == Step::Model
        && let Some(more) = &opening.model.more
    {
        let rect = popup::rect(area);
        let inner = popup::body(rect);
        let (title, body, hint) = more::popup(more, texts, (inner.width, inner.height));
        return popup::draw(frame, rect, (&title, &body, &hint));
    }
    if let Some(models) = settings.models.as_ref() {
        caret = super::model_popups::draw(
            frame,
            (models, &settings.hits),
            &settings.texts.models,
            &texts.import[1],
        )
        .or(caret);
    }
    let Some(popup) = settings.popup.as_ref() else {
        return caret;
    };
    // 配色方案那一步的选择窗：摆在格子下面、不压暗（Miyu `theme_step.rs` 的 `picker`）。
    if opening.step == Step::Theme && matches!(popup.window, Window::Choose(_)) {
        let content = Rect::new(x, y, width, area.bottom().saturating_sub(y + 2));
        let rows = built.focus.unwrap_or(0).saturating_sub(skip) + 1;
        picker(frame, content, rows, popup, settings);
        return caret;
    }
    super::popup::draw(
        frame,
        block,
        (popup, &settings.hits),
        (&settings.texts, vim(settings)),
    )
    .or(caret)
}

/// 配色方案的选择窗：宽照配置页的窗（Miyu `popup_wide`），在这一格下面（放不下的往上挪到放得下）。
fn picker(
    frame: &mut Frame,
    content: Rect,
    rows: u16,
    popup: &crate::settings::Popup,
    settings: &Settings,
) {
    let width = 78.min(content.width);
    let lines = super::popup::lines(popup, &settings.texts, width, vim(settings));
    let full = lines.height(width, content.height);
    let below = (content.y + rows + 1).min(content.bottom());
    let top = below
        .min(content.bottom().saturating_sub(full))
        .max(content.y);
    let height = lines.height(width, content.bottom().saturating_sub(top));
    let area = Rect::new(content.x, top, width, height);
    super::floating::centered(frame, area, (width, height), &lines, &settings.hits);
}

/// 按键模式是不是 Vim。
fn vim(settings: &Settings) -> bool {
    super::vim(settings)
}

/// 这一屏最下面写什么按键（Miyu `ui/oobe/mod.rs` 的 `hint`）。
fn hint(settings: &Settings, opening: &Opening) -> String {
    let keys = &settings.opening_texts().keys;
    match opening.step {
        Step::Model if let Some(more) = &opening.model.more => {
            more::keys(more, settings.opening_texts()).clone()
        }
        Step::Language => keys.first.clone(),
        Step::Model => match &opening.model.phase {
            Phase::Form(form) if form.editing() => keys.editing.clone(),
            Phase::Form(form) if form.slot() == Some(Slot::Driver) => keys.driver.clone(),
            Phase::Form(form) if form.slot() == Some(Slot::Test) => keys.test.clone(),
            Phase::Form(form) if form.slot() == Some(Slot::Anyway) => keys.anyway.clone(),
            Phase::Form(_) | Phase::Testing(_) => keys.form.clone(),
            Phase::Models(models) if models.filter.editing() => keys.searching.clone(),
            Phase::Models(_) | Phase::Saving(_) => keys.models.clone(),
            _ => keys.pick.clone(),
        },
        Step::Theme if settings.popup.is_some() => settings
            .texts
            .key_hints
            .choose
            .iter()
            .map(|[k, d]| format!("{k} {d}"))
            .collect::<Vec<_>>()
            .join(" · "),
        Step::Theme if opening.theme == ThemeSlot::Next => keys.pick.clone(),
        Step::Theme => keys.theme.clone(),
        _ => keys.pick.clone(),
    }
}

/// 进度那一行。
fn progress(opening: &Opening, steps: &[String; 5]) -> Line<'static> {
    let now = opening.step.index().unwrap_or(steps.len());
    let mut spans = Vec::new();
    for (i, name) in steps.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(" ── ", theme::faint()));
        }
        spans.push(match i.cmp(&now) {
            std::cmp::Ordering::Less => Span::styled(format!("● {name}"), theme::dim()),
            std::cmp::Ordering::Equal => Span::styled(
                format!("◉ {name}"),
                theme::accent().add_modifier(Modifier::BOLD),
            ),
            std::cmp::Ordering::Greater => Span::styled(format!("○ {name}"), theme::faint()),
        });
    }
    Line::from(spans)
}

/// 中间那一块（Miyu `ui/oobe/mod.rs` 的 `content`）。
fn content(settings: &Settings, opening: &Opening, width: u16, height: u16) -> Content {
    let texts = settings.opening_texts();
    let mut c = match opening.step {
        Step::Language => {
            let words = &texts.language;
            let mut c = Content::sized(width, height);
            c.heading(&words.title, &words.sub);
            for (i, row) in opening.language.rows.iter().enumerate() {
                c.row(
                    i == opening.language.selected,
                    vec![Span::raw(row.clone())],
                    None,
                );
            }
            c
        }
        Step::Keys => {
            let words = &texts.controls;
            let mut c = Content::sized(width, height);
            c.heading(&words.title, &words.sub);
            let rows = [
                (&words.arrows, &words.arrows_note),
                (&words.vim, &words.vim_note),
            ];
            for (i, (name, note)) in rows.into_iter().enumerate() {
                c.row(
                    i == opening.keys.selected,
                    vec![
                        Span::raw(name.clone()),
                        Span::styled(format!("  {note}"), theme::dim()),
                    ],
                    None,
                );
            }
            c
        }
        Step::Icons => icons(settings, opening, width, height),
        Step::Model => {
            let beat = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_millis() / 80);
            let spinner = SPINNER[usize::try_from(beat % 10).unwrap_or(0)];
            model::content(settings, &opening.model, (width, height), spinner)
        }
        Step::Theme => theme_step(settings, opening, width, height),
        Step::Done => Content::sized(width, height),
    };
    // 状态行的话（导入了几家、在后台列模型、没存成）写在这一块最下面。接模型那一步正在试、试通了（「✓ 连接成功，找到 N 个
    // 模型」那一屏）的时候不写好消息、在忙的那一句：后台列模型的「X 连得上，已获取 N 个模型」「正在获取…」和上面那一行是一个
    // 意思（第二十六趟验收时报的两句同义的话；Miyu 引导这一屏只有上面那一句，后台列模型不说话）。出错的照写。
    let tested = opening.step == Step::Model
        && matches!(
            opening.model.phase,
            Phase::Testing(_) | Phase::Models(_) | Phase::Saving(_)
        );
    if let Some((said, tone)) = settings
        .status
        .as_ref()
        .filter(|(_, tone)| !tested || *tone == crate::settings::Tone::Bad)
    {
        c.blank();
        c.note(said, super::models::tone(*tone));
    }
    c
}

/// 图标那一步（Miyu `pick::icons`）：中间一行间隔着的几个图标（照 Nerd Font 那一套），下面两行选项。
fn icons(settings: &Settings, opening: &Opening, width: u16, height: u16) -> Content {
    let words = &settings.opening_texts().icons;
    let mut c = Content::sized(width, height);
    c.heading(&words.title, &words.sub);
    if let Ok(set) = crate::config::named_icons(ICON_SETS[0]) {
        let gap = "    ";
        let mut row = String::new();
        for glyph in SAMPLE_ICONS.iter().map(|t| set.tool_in(t)) {
            let next = if row.is_empty() {
                glyph.to_string()
            } else {
                format!("{row}{gap}{glyph}")
            };
            if next.width() > usize::from(width) {
                break;
            }
            row = next;
        }
        let pad = usize::from(width).saturating_sub(row.width()) / 2;
        c.lines.push(Line::from(vec![
            Span::raw(" ".repeat(pad)),
            Span::styled(row, theme::accent().add_modifier(Modifier::BOLD)),
        ]));
        c.blank();
    }
    for (i, label) in opening.icons.rows.iter().enumerate() {
        c.row(
            i == opening.icons.selected,
            vec![Span::raw(label.clone())],
            None,
        );
    }
    c
}

/// 配色方案那一步（Miyu `theme_step.rs`）：明暗、铺底色（下面暗色一句）两格，「下一步 →」。
fn theme_step(settings: &Settings, opening: &Opening, width: u16, height: u16) -> Content {
    let texts = settings.opening_texts();
    let words = &texts.theme;
    let mut c = Content::sized(width, height);
    c.cue.clone_from(&texts.keys.enter_pick);
    c.heading(&words.title, &words.sub);
    let label_w = words.rows.iter().map(|r| r.width()).max().unwrap_or(0);
    let on = |slot| opening.theme == slot && settings.popup.is_none();
    let mode = match settings.looks("tui.theme_mode") {
        Some(lm_kernel::config::Value::Text(now)) => crate::settings::opening::MODES
            .iter()
            .position(|m| *m == now.as_ref()),
        _ => None,
    }
    .unwrap_or(0);
    c.chip(
        &words.rows[0],
        label_w,
        &words.modes[mode],
        on(ThemeSlot::Mode),
    );
    c.blank();
    let off = matches!(
        settings.looks("tui.theme_background"),
        Some(lm_kernel::config::Value::Bool(false))
    );
    c.chip(
        &words.rows[1],
        label_w,
        &words.ground[usize::from(off)],
        on(ThemeSlot::Ground),
    );
    c.note_at(2 + label_w + 2, &words.ground_note, theme::dim());
    c.blank();
    c.action(on(ThemeSlot::Next), &format!("{} →", words.next));
    c
}

/// 完成那一屏（Miyu `screens.rs` 的 `done`，星点、吉祥物不搬）：正中写标题、空一行摘要、空一行「按 Enter 开始对话」。
fn done(frame: &mut Frame, area: Rect, settings: &Settings, opening: &Opening) {
    let words = &settings.opening_texts().done;
    let y = area.y + area.height.saturating_sub(5) / 2;
    let summary = settings.opening_summary(opening);
    let rows = [
        (
            0,
            words.title.clone(),
            Style::new().add_modifier(Modifier::BOLD),
        ),
        (2, summary, theme::dim()),
        (4, words.start.clone(), theme::accent()),
    ];
    for (dy, text, style) in rows {
        let w = u16::try_from(text.width())
            .unwrap_or(area.width)
            .min(area.width);
        frame.buffer_mut().set_line(
            area.x + (area.width / 2).saturating_sub(w / 2),
            y + dy,
            &Line::styled(text, style),
            w,
        );
    }
}

#[cfg(test)]
mod tests {
    /// 图标那一步列的几件都是 LiteMiyu 还有的工具，在 Nerd Font 那一套里各有自己的图标（LM-8：原来 Miyu 那一份里的 `grep`、
    /// `todowrite` 砍了，画出来是通用的扳手）。
    #[test]
    fn the_sample_icons_are_tools_that_still_have_their_own_glyph() {
        let set = crate::config::named_icons(super::ICON_SETS[0]).expect("出厂的图标读得出");
        for tool in super::SAMPLE_ICONS {
            assert!(set.tools.contains_key(tool), "{tool} 没有自己的图标");
        }
    }
}
