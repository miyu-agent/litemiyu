//! 画「预设」页（`settings/presets.rs`，LM-29）：列表一个预设一行，照技能页、MCP 页的样子（名字照最长的对齐、暗色的状态，
//! 选中的左边竖条、整行铺底）；表单照一页的画法（小标题强调色加粗，一项一行，名字、加粗的值、暗色的说明）。

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use crate::settings::texts::fill;
use crate::settings::{FormRow, Settings};
use crate::theme;
use crate::ui::rows::clip;
use lm_engine::presets::{BARE, FULL};

/// 这一页画出来的：一行行、选中的在第几行、右下角的第几个和一共几个、是不是表单（按键提示照它挑）。
pub struct Drawn {
    /// 一行行。
    pub lines: Vec<Line<'static>>,
    /// 选中的那一行在 `lines` 里是第几行。
    pub focus: usize,
    /// 选中的是第几个（从 0 数）、一共几个。
    pub place: (usize, usize),
    /// 画的是表单。
    pub form: bool,
}

/// 画列表或者表单；列表的光标在第 `at` 行。
pub fn lines(settings: &Settings, at: usize, width: u16) -> Drawn {
    match settings.preset_form() {
        Some((form, _)) => form_lines(settings, form.at, width),
        None => list_lines(settings, at, width),
    }
}

/// 一行：竖条、名字（照 `names` 列对齐）、加粗的值、暗色的说明；放不下的截掉，选中的整行铺底。
fn row(
    chosen: bool,
    (name, names): (&str, usize),
    value: &str,
    after: (&str, Style),
    width: u16,
) -> Line<'static> {
    let mut spans = vec![super::floating::lead(chosen, true)];
    let pad = " ".repeat(names.saturating_sub(name.width()) + 2);
    spans.push(Span::raw(format!("{name}{pad}")));
    if !value.is_empty() {
        spans.push(Span::styled(
            format!("{value}  "),
            Style::new().add_modifier(Modifier::BOLD),
        ));
    }
    let used: usize = spans.iter().map(|span| span.content.width()).sum();
    let room = usize::from(width).saturating_sub(used);
    let said = clip(after.0, u16::try_from(room).unwrap_or(u16::MAX));
    let rest = room.saturating_sub(said.width());
    spans.push(Span::styled(said, after.1));
    spans.push(Span::raw(" ".repeat(rest)));
    let line = Line::from(spans);
    match chosen {
        true => line.patch_style(theme::row_focus()),
        false => line,
    }
}

/// 列表：自带的两个在前，自己定义的照名字排；名字后面写「自带」「默认」，自己定义的再写一句它开了什么。
fn list_lines(settings: &Settings, at: usize, width: u16) -> Drawn {
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
            let built_in = *name == FULL || *name == BARE;
            let mut said: Vec<String> = Vec::new();
            if *name == default {
                said.push(texts.marks[1].clone());
            }
            match listed.custom.iter().find(|pin| pin.name == *name) {
                Some(pin) => {
                    let on = pin.features.values().filter(|on| **on).count();
                    let off = pin.features.len() - on;
                    said.push(texts.summary[usize::from(!pin.unlisted)].clone());
                    // 单独设置过的几项：一项都没有的那一截不写。
                    for (count, part) in [on, off].into_iter().zip(&texts.summary_parts) {
                        if count > 0 {
                            said.push(fill(part, &[("n", &count.to_string())]));
                        }
                    }
                }
                None if built_in => said.push(texts.marks[0].clone()),
                None => {}
            }
            let said = said.join(&settings.texts.separator);
            row(
                i == at,
                (shown.as_str(), longest),
                "",
                (said.as_str(), theme::dim()),
                width,
            )
        })
        .collect();
    Drawn {
        lines,
        focus: at,
        place: (at, names.len()),
        form: false,
    }
}

/// 表单：名称、没写的项目，再一组组列；小标题上面空一行。
fn form_lines(settings: &Settings, at: usize, width: u16) -> Drawn {
    let texts = &settings.texts.presets;
    let rows = settings.preset_rows();
    let Some((form, pin)) = settings.preset_form() else {
        return Drawn {
            lines: Vec::new(),
            focus: 0,
            place: (0, 0),
            form: true,
        };
    };
    let toggle = |on: bool| settings.texts.toggle[usize::from(!on)].as_str();
    let longest = rows
        .iter()
        .map(|row| match row {
            FormRow::Name => texts.rows[0].width(),
            FormRow::Unlisted => texts.rows[1].width(),
            FormRow::Feature(feature) => feature.name.width(),
            FormRow::Group(_) => 0,
        })
        .max()
        .unwrap_or(0);
    let at = at.min(rows.len().saturating_sub(1));
    let mut lines = Vec::new();
    let mut focus = 0;
    for (i, one) in rows.iter().enumerate() {
        let chosen = i == at;
        if chosen {
            focus = lines.len();
        }
        match one {
            FormRow::Group(group) => {
                lines.push(Line::default());
                lines.push(Line::styled(
                    format!("  {}", texts.groups[*group]),
                    theme::accent().add_modifier(Modifier::BOLD),
                ));
            }
            FormRow::Name => {
                let name = crate::cli::clean(&form.name);
                lines.push(row(
                    chosen,
                    (texts.rows[0].as_str(), longest),
                    &name,
                    ("", theme::dim()),
                    width,
                ));
            }
            FormRow::Unlisted => lines.push(row(
                chosen,
                (texts.rows[1].as_str(), longest),
                toggle(pin.unlisted),
                (texts.unlisted_note.as_str(), theme::dim()),
                width,
            )),
            FormRow::Feature(feature) => {
                // 说明：给人看的名字，再是为什么不可用、是不是跟着「没写的项目」。
                let mut said: Vec<&str> = Vec::new();
                if !feature.shown.is_empty() && feature.shown != feature.name {
                    said.push(feature.shown.as_str());
                }
                let warn = feature.missing || (feature.global_off && feature.on);
                if feature.missing {
                    said.push(texts.missing.as_str());
                } else if feature.global_off {
                    said.push(match feature.id.starts_with("mcp-") {
                        true => texts.mcp_off.as_str(),
                        false => texts.skill_off.as_str(),
                    });
                }
                if !feature.explicit {
                    said.push(texts.follows_mark.as_str());
                }
                let said = said.join(&settings.texts.separator);
                let style = match warn {
                    true => theme::warn(),
                    false => theme::dim(),
                };
                let name = crate::cli::clean(&feature.name);
                lines.push(row(
                    chosen,
                    (name.as_str(), longest),
                    toggle(feature.on),
                    (said.as_str(), style),
                    width,
                ));
            }
        }
    }
    let selectable = |rows: &[FormRow]| rows.iter().filter(|row| row.selectable()).count();
    Drawn {
        lines,
        focus,
        place: (selectable(&rows[..at]), selectable(&rows)),
        form: true,
    }
}
