//! 接模型那一步的那一块（照 Miyu 终端 `ui/oobe/model.rs` 搬）：已配置、选一家、填的那一屏、在试、选模型、在存。

use ratatui::style::Style;
use ratatui::text::Span;
use unicode_width::UnicodeWidthStr;

use super::content::Content;
use crate::settings::Settings;
use crate::settings::opening::model::DRIVERS;
use crate::settings::opening::{Form, ModelStep, Phase, Row, Slot};
use crate::theme;

/// 照走到哪一屏拼，最多 `height` 行高（模型多的列表照它开窗）。`spinner` 是试连接、在存时转轮这一帧的字。
pub fn content(
    settings: &Settings,
    step: &ModelStep,
    (width, height): (u16, u16),
    spinner: &str,
) -> Content {
    let texts = settings.opening_texts();
    let words = &texts.model;
    let mut c = Content::sized(width, height);
    c.cue.clone_from(&texts.keys.enter_edit);
    match &step.phase {
        Phase::Ready(keep) => {
            c.heading(&words.ready_title, "");
            let chat = step.chat.clone().unwrap_or_default();
            c.note(&words.ready.replace("{name}", &chat), theme::good());
            c.blank();
            c.row(*keep, vec![Span::raw(words.keep.clone())], None);
            c.row(!*keep, vec![Span::raw(words.change.clone())], None);
        }
        Phase::List => list(&mut c, settings, step),
        Phase::Form(form) => form_lines(&mut c, settings, step, form, None),
        Phase::Testing(form) => form_lines(&mut c, settings, step, form, Some(spinner)),
        Phase::Models(models) | Phase::Saving(models) => {
            c.heading(&settings.form_title(&models.form), &words.pick_sub);
            // 列出了几个说几个；列不出、拿填的模型名发了一句通了的说那个模型能用。
            let line = if models.listed > 0 {
                words
                    .connected
                    .replace("{count}", &models.listed.to_string())
            } else {
                let model = models.names.first().map_or("", String::as_str);
                words.connected_request.replace("{model}", model)
            };
            c.note(&format!("✓ {line}"), theme::good());
            c.blank();
            c.search(
                &words.search,
                words.search.width(),
                &models.filter,
                &words.search_cue,
            );
            c.blank();
            let saving = matches!(step.phase, Phase::Saving(_));
            let rows = models
                .matches()
                .into_iter()
                .map(|name| vec![Span::raw(name.clone())])
                .collect();
            // 下面留给转轮、红字的几行。
            let below = 2 * (u16::from(saving) + u16::from(step.error.is_some()));
            let more = (words.more_above.as_str(), words.more_below.as_str());
            c.window(rows, models.cursor, below, more);
            if saving {
                c.blank();
                c.note(&format!("{spinner} {}", words.saving), theme::dim());
            }
        }
    }
    notes(&mut c, step);
    c
}

/// 选一家：常用的几家、其他；LiteMiyu 多的「从 Miyu 导入…」。
fn list(c: &mut Content, settings: &Settings, step: &ModelStep) {
    let texts = settings.opening_texts();
    let words = &texts.model;
    c.heading(&words.title, &words.sub);
    // 取模型目录的那一句（LM-9：程序不带目录，走到这一步才去取）：在取、取到了、没取到。
    if let Some((said, tone)) = settings.catalog_note() {
        c.note(said, crate::ui::settings::models::tone(*tone));
        c.blank();
    }
    for (i, row) in step.rows.iter().enumerate() {
        let selected = i == step.cursor;
        match row {
            Row::Other => {
                c.blank();
                c.section(&words.other);
            }
            Row::Provider(p) => {
                let left = vec![Span::styled(
                    p.name.clone(),
                    if p.supported {
                        Style::new()
                    } else {
                        theme::faint()
                    },
                )];
                let right = if p.configured.is_some() {
                    Some(Span::styled(words.configured.clone(), theme::good()))
                } else if p.env.is_some() {
                    Some(Span::styled(words.found.clone(), theme::accent()))
                } else {
                    None
                };
                c.row(selected, left, right);
            }
            Row::More => c.row(selected, vec![Span::raw(words.more.clone())], None),
            Row::Custom => {
                let left = vec![
                    Span::raw(words.custom.clone()),
                    Span::styled(format!("  {}", words.custom_note), theme::dim()),
                ];
                c.row(selected, left, None);
            }
            Row::Import => {
                let left = vec![
                    Span::raw(texts.import[0].clone()),
                    Span::styled(format!("  {}", texts.import[1]), theme::dim()),
                ];
                c.row(selected, left, None);
            }
        }
    }
}

/// 填密钥、自定义；在试的时候下面一行转轮「正在连接…」。
fn form_lines(
    c: &mut Content,
    settings: &Settings,
    step: &ModelStep,
    form: &Form,
    testing: Option<&str>,
) {
    let words = &settings.opening_texts().model;
    let sub = if form.provider.is_none() {
        &words.custom_sub
    } else {
        &words.key_sub
    };
    c.heading(&settings.form_title(form), sub);
    let slots = form.slots();
    let labels = [
        &words.url,
        &words.driver,
        &words.id,
        &words.name,
        &words.key,
        &words.model_name,
    ];
    let label_w = labels.iter().map(|l| l.width()).max().unwrap_or(0);
    // 不用填的那几种：写 key 从哪来。
    if let Some(p) = form.provider.as_ref().filter(|p| p.ready()) {
        let how = match (&p.configured, &p.env) {
            (Some(_), _) => words.configured.clone(),
            (None, Some(env)) => words.key_env.replace("{env}", env),
            _ => words.key_none.clone(),
        };
        c.note_at(2, &format!("{}  {how}", words.key), theme::dim());
    }
    let auto_id = step.auto_id(form, &settings.providers());
    let focus = form.slot().filter(|_| testing.is_none());
    for (i, slot) in slots.into_iter().enumerate() {
        // 格子之间空一行。
        if i > 0 {
            c.blank();
        }
        let focused = focus == Some(slot);
        match slot {
            Slot::Url => c.field(&words.url, label_w, &form.url, "", focused),
            Slot::Name => c.field(&words.name, label_w, &form.name, &words.name_hint, focused),
            // 空着的暗色写照地址自动起的编号：不改就用它。
            Slot::Id => c.field(
                &words.id,
                label_w,
                &form.id,
                auto_id.as_deref().unwrap_or_default(),
                focused,
            ),
            Slot::Driver => {
                let at = form.driver.min(DRIVERS.len() - 1);
                let name = words.drivers.get(at).map_or("", String::as_str);
                c.switch(&words.driver, label_w, name, focused);
            }
            Slot::Key => c.field(&words.key, label_w, &form.key, "", focused),
            Slot::Test => c.action(focused, &format!("{} →", words.test)),
            Slot::Anyway => c.action(focused, &words.save_anyway),
            Slot::Model => {
                c.field(
                    &words.model_name,
                    label_w,
                    &form.model,
                    &words.model_example,
                    focused,
                );
                c.note_at(2, &words.cost, theme::dim());
            }
        }
    }
    if let Some(spinner) = testing {
        c.blank();
        c.note(&format!("{spinner} {}", words.testing), theme::accent());
    }
}

/// 红字、黄字。
fn notes(c: &mut Content, step: &ModelStep) {
    if let Some(warn) = &step.warn {
        c.blank();
        c.note(warn, theme::warn());
    }
    if let Some(error) = &step.error {
        c.blank();
        c.note(error, theme::error());
    }
}
