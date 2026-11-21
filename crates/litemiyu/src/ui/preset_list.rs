//! `/preset` 的框（LM-29，`docs/00-设计.md` 第六节第 14 条；状态和按键在 `app/presets.rs`）：和 `/effort` 一个位置、一个样子。
//! 一个预设一行：自带的两个在前，自己定义的照名字排；默认预设行首一个强调色的 `●`，名字后面暗色写「自带」「默认」「当前会话」。
//! 会话的预设已经固定时列表下面多一行字说明。

use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use super::panel::{self, Chrome, Row};
use crate::app::Seen;
use crate::config::texts;
use crate::theme;
use lm_engine::presets::{BARE, FULL};

texts! {
    /// 预设的字：`/preset` 的框、`/new <名字>`、侧边栏。
    #[derive(Debug, Clone)]
    pub struct Texts {
        /// 上边框写的。
        pub title: String,
        /// 下边框的按键提示：会话还没开。
        pub hint: String,
        /// 下边框的按键提示：会话的预设已经固定。
        pub hint_fixed: String,
        /// 引擎还没交回来。
        pub loading: String,
        /// 自带的「全部功能」的名字。
        pub full: String,
        /// 自带的「无功能」的名字。
        pub bare: String,
        /// 名字后面的几个记号：自带、默认预设、当前会话用的。
        pub marks: [String; 3],
        /// 记号之间的分隔。
        pub marks_join: String,
        /// 会话的预设已经固定时列表下面的那一行。
        pub fixed: String,
        /// 侧边栏那一行，`{name}`。
        pub side: String,
        /// 钉着的预设后来改过了，`{name}`。
        pub changed: String,
        /// 钉着的预设后来删掉了，`{name}`。
        pub gone: String,
        /// 设成了默认预设（会话的预设已经固定，只改默认），`{name}`。
        pub default_set: String,
        /// 设成了默认预设，当前会话也切过去了，`{name}`。
        pub default_set_switched: String,
        /// 只切换了当前会话，`{name}`。
        pub switched: String,
        /// 用它新建了一个会话，`{name}`。
        pub new_with: String,
        /// 回答进行中，不能新建会话。
        pub running: String,
        /// 自带的不能删。
        pub built_in_fixed: String,
        /// 再按一次 `d` 才删，`{name}`。
        pub delete_again: String,
        /// 删掉了，`{name}`。
        pub deleted: String,
        /// `/new <名字>`：没有这个预设，`{name}`。
        pub unknown: String,
        /// 应急模式下 `/new` 后面只认自带的两个。
        pub rescue: String,
    }
}

/// 框、排好的行、每一行是第几个预设。
pub fn lines(
    seen: &Seen,
    width: u16,
    max: usize,
) -> (Chrome, Vec<Line<'static>>, Vec<Option<usize>>) {
    let config = seen.config;
    let texts = &config.text().presets;
    let fixed = seen.talk.main_transcript().session.is_some();
    let hint = if fixed {
        &texts.hint_fixed
    } else {
        &texts.hint
    };
    let chrome = Chrome::new(&texts.title, Vec::new()).hint(hint);
    let note = |text: &str| {
        panel::item(
            false,
            vec![Span::styled(text.to_string(), theme::faint())],
            None,
            width,
        )
    };
    let Some((list, listed)) = seen
        .boxes
        .presets
        .as_ref()
        .and_then(|list| Some((list, list.listed.as_ref()?)))
    else {
        return (chrome, vec![note(&texts.loading)], vec![None]);
    };
    // 当前会话用的那一个：开了的照引擎说的，还没开的照定好了的下一个、默认预设。
    let current = match (&seen.talk.preset, fixed) {
        (Some(pinned), _) => Some(pinned.name.clone()),
        (None, true) => None,
        (None, false) => Some(
            seen.link
                .next_preset
                .clone()
                .unwrap_or_else(|| listed.effective_default().to_string()),
        ),
    };
    let default = listed.effective_default();
    let mut rows: Vec<Row> = listed
        .names()
        .enumerate()
        .map(|(i, name)| {
            let shown = match name {
                FULL => texts.full.clone(),
                BARE => texts.bare.clone(),
                name => crate::cli::clean(name),
            };
            let marks: Vec<&str> = [
                (name == FULL || name == BARE, &texts.marks[0]),
                (name == default, &texts.marks[1]),
                (current.as_deref() == Some(name), &texts.marks[2]),
            ]
            .into_iter()
            .filter(|(on, _)| *on)
            .map(|(_, mark)| mark.as_str())
            .collect();
            let mut content = vec![Span::raw(shown)];
            if !marks.is_empty() {
                content.push(Span::styled(
                    format!("  {}", marks.join(&texts.marks_join)),
                    theme::faint(),
                ));
            }
            (
                Some(i),
                panel::item_with(
                    i == list.selected,
                    panel::current_lead(&config.layout.current_mark, name == default),
                    content,
                    None,
                    width,
                ),
            )
        })
        .collect();
    // 说明和列表之间空一行；说明要占的行先留出来，放不下的少露几个预设。
    let notes: Vec<Row> = match fixed {
        true => std::iter::once(String::new())
            .chain(wrapped(&texts.fixed, usize::from(width).saturating_sub(2)))
            .map(|line| (None, note(&line)))
            .collect(),
        false => Vec::new(),
    };
    let room = max.saturating_sub(notes.len()).max(1);
    rows = panel::fit(rows, Some(list.selected), room);
    rows.extend(notes);
    rows.truncate(max);
    let (map, lines) = rows.into_iter().unzip();
    (chrome, lines, map)
}

/// 一句话照 `width` 列折成几行：照字折（中文没有空格），不拆开一个字。
fn wrapped(text: &str, width: usize) -> Vec<String> {
    let width = width.max(2);
    let mut lines = vec![String::new()];
    let mut used = 0;
    for c in text.chars() {
        let w = c.to_string().width();
        if used + w > width {
            lines.push(String::new());
            used = 0;
        }
        if let Some(line) = lines.last_mut() {
            line.push(c);
        }
        used += w;
    }
    lines
}
