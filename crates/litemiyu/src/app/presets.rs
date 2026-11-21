//! 预设在对话界面这一头（LM-29，`docs/00-设计.md` 第六节第 14 条）：`/preset` 的框、`/new <预设名>` 和它的 `Tab` 补全、
//! 侧边栏和 `/status` 写的当前会话的预设（LM-33 起输入框下面那一行不写预设名）。
//!
//! `/preset` 的框照 `/effort` 的样子（开着不能打字，`j`、`k` 同 `↓`、`↑`）：自带的两个在前，自己定义的照名字排，一行一个；
//! 列表很短，不做打字筛选。按键：
//!
//! - `Enter`：把选中的设为默认预设，同时切换当前会话；
//! - `s`：只切换当前会话，不改默认预设；
//! - `e`：编辑选中的；`a`：新建。打开的就是配置界面「预设」那一页的表单（`settings/presets.rs`），`Esc` 回到这个框。
//!   在自带的两个上按 `e` 是以它为底新建一份；
//! - `d`：删除选中的，先提示、再按一次才删；自带的两个不能删；
//! - `Esc`：关闭。
//!
//! 会话发出第一个请求以后预设固定（换工具面会让模型的缓存失效）。会话是第一句话发出去时才开的，所以「固定了」就是「会话
//! 已经开了」（打开的旧会话也是）：这时 `Enter` 只改默认预设，`s` 是用选中的预设新建一个会话，框里多一行字说明。还没开会话时
//! 「切换当前会话」就是告诉引擎下一个会话用哪个预设（[`Command::New`]）。
//!
//! 列表向引擎线程要（[`Command::Presets`]）：它照配置文件读，写默认预设、删预设也在那一头排着办，读到的一定是写完以后的。
//! `/new <预设名>` 和 `Tab` 补全要当场知道有哪些预设，在界面线程里读一次配置文件（只认预设那几项，`lm_engine::presets::read`）。
//! 平常不开这个框、不打 `/new <名字>` 的什么都不读。应急模式不读配置：`/preset` 不列（`rescue.rs`），`/new` 只认自带的两个。

use crossterm::event::{KeyCode, KeyEvent, MouseEvent, MouseEventKind};
use lm_engine::presets::{self, BARE, FULL, Listed};
use lm_engine::store::paths::{Env, Paths};
use ratatui::layout::Position;

use super::{App, Panel};
use crate::core::{Command, PinState, Pinned, Update};
use crate::settings::Outcome;

/// 开着的 `/preset` 框。
#[derive(Debug, Default)]
pub struct PresetList {
    /// 引擎交回来的那一份；还没交回来的是 `None`。
    pub listed: Option<Listed>,
    /// 选中第几行（自带的两个是第 0、1 行）。
    pub selected: usize,
    /// 交回来以后选中叫这个名字的那一行（从表单回来时停在刚改的那一个上）。
    pub want: Option<String>,
    /// 按过一次 `d`、等再按一次确认删除的那个预设。
    pub deleting: Option<String>,
}

impl PresetList {
    /// 一行行的名字：自带的两个在前，自己定义的照名字排。还没交回来的是空的。
    pub fn names(&self) -> Vec<&str> {
        self.listed
            .as_ref()
            .map(|listed| listed.names().collect())
            .unwrap_or_default()
    }

    /// 选中的那一个的名字。
    pub fn picked(&self) -> Option<String> {
        self.names().get(self.selected).map(ToString::to_string)
    }
}

impl App {
    /// `/preset`：开框，向引擎要列表。配置界面刚关、还有没写完的配置的，先等它写完（最多两秒）再要，列出来的才是改完的。
    pub(super) fn open_presets(&mut self) {
        self.open_presets_at(None);
    }

    /// 同 [`App::open_presets`]，交回来以后选中叫 `want` 的那一行。
    fn open_presets_at(&mut self, want: Option<String>) {
        if !self.reachable() {
            return;
        }
        if self.writes.is_some() {
            self.finish_writes(std::time::Instant::now() + std::time::Duration::from_secs(2));
            self.poll_closed_writes();
        }
        self.boxes.presets = Some(PresetList {
            want,
            ..PresetList::default()
        });
        self.boxes.panel = Some(Panel::Presets);
        self.link.core.send(Command::Presets);
    }

    /// 当前会话的预设是不是已经固定了：会话已经开了。
    pub fn preset_fixed(&self) -> bool {
        self.talk.main_transcript().session.is_some()
    }

    /// 引擎交来的两样：列出来的预设、当前会话钉着的预设，办了交回 `None`；别的原样交回、接着往下走。
    pub(super) fn preset_update(&mut self, update: Update) -> Option<Update> {
        match update {
            Update::Presets(listed) => {
                // 默认预设跟着换（框关了以后才到的也记：回车设默认以后框当场就关）。
                self.link.default_preset = Some(listed.effective_default())
                    .filter(|name| *name != FULL)
                    .map(str::to_string);
                if let Some(list) = &mut self.boxes.presets {
                    let now = list
                        .want
                        .take()
                        .or_else(|| list.picked())
                        .or_else(|| self.talk.preset.as_ref().map(|p| p.name.clone()))
                        .or_else(|| self.link.next_preset.clone())
                        .unwrap_or_else(|| listed.effective_default().to_string());
                    let at = listed.names().position(|name| name == now);
                    let last = listed.names().count().saturating_sub(1);
                    list.selected = at.unwrap_or(list.selected).min(last);
                    list.listed = Some(listed);
                }
                None
            }
            Update::Preset(pinned) => {
                self.talk.preset = Some(pinned);
                None
            }
            other => Some(other),
        }
    }

    /// 框开着时的按键。
    pub(super) fn presets_key(&mut self, key: KeyEvent) {
        let Some(list) = &mut self.boxes.presets else {
            self.boxes.panel = None;
            return;
        };
        let last = list.names().len().saturating_sub(1);
        // 等着确认删除：再按一次 `d` 才删，按别的键不删、键照常办。
        let deleting = list.deleting.take();
        // `j`、`k` 在前头换成了 `↓`、`↑`（开着不能打字的框，`vim.rs`）。
        match key.code {
            KeyCode::Up => list.selected = list.selected.saturating_sub(1),
            KeyCode::Down => list.selected = (list.selected + 1).min(last),
            _ => {}
        }
        match key.code {
            KeyCode::Esc => self.close_presets(),
            KeyCode::Enter => self.preset_default(),
            KeyCode::Char('s') => self.preset_switch(),
            KeyCode::Char('e') => self.preset_edit(false),
            KeyCode::Char('a') => self.preset_edit(true),
            KeyCode::Char('d') => self.preset_delete(deleting),
            _ => {}
        }
    }

    /// 鼠标在框上：悬停选中那一行，点一下等于在它上面按回车（设为默认预设，会话还没开的同时切过去）；同 `/model`、
    /// `/effort`、`/sessions` 的框（它们也是悬停选中、点一下就办）。交回归不归它。
    pub(super) fn presets_mouse(&mut self, mouse: MouseEvent, at: Position) -> bool {
        if self.boxes.panel != Some(Panel::Presets) {
            return false;
        }
        let areas = self.screen.areas;
        if !areas.menu.contains(at) {
            return false;
        }
        let row = at.y.checked_sub(areas.menu_text.y).map(usize::from);
        let index = row.and_then(|row| self.screen.panel_rows.get(row).copied().flatten());
        if let (Some(index), Some(list)) = (
            index.filter(|_| areas.menu_text.contains(at)),
            &mut self.boxes.presets,
        ) {
            list.selected = index;
            list.deleting = None;
            if matches!(mouse.kind, MouseEventKind::Down(_)) {
                self.preset_default();
            }
        }
        true
    }

    /// 关框。
    fn close_presets(&mut self) {
        self.boxes.panel = None;
        self.boxes.presets = None;
    }

    /// 预设 `name` 给人看的名字：自带的两个照界面语言，自己定义的原样（控制字符换掉）。
    pub fn preset_shown(&self, name: &str) -> String {
        let texts = &self.config.text().presets;
        match name {
            FULL => texts.full.clone(),
            BARE => texts.bare.clone(),
            _ => crate::cli::clean(name),
        }
    }

    /// `Enter`：把选中的设为默认预设；会话还没开的同时切过去。
    fn preset_default(&mut self) {
        let Some(name) = self.boxes.presets.as_ref().and_then(PresetList::picked) else {
            return;
        };
        self.link.core.send(Command::PresetDefault(name.clone()));
        let texts = &self.config.text().presets;
        let said = match self.preset_fixed() {
            true => &texts.default_set,
            false => &texts.default_set_switched,
        };
        let said = said.replace("{name}", &self.preset_shown(&name));
        if !self.preset_fixed() {
            self.new_session(Some(name));
        }
        self.close_presets();
        self.hint(said, true);
    }

    /// `s`：只切换当前会话，不改默认预设。会话已经开了的，用选中的预设新建一个会话（回答进行中的先说要打断）。
    fn preset_switch(&mut self) {
        let Some(name) = self.boxes.presets.as_ref().and_then(PresetList::picked) else {
            return;
        };
        let texts = &self.config.text().presets;
        if self.preset_fixed() && self.talk.main_transcript().running.is_some() {
            let said = texts.running.clone();
            return self.hint(said, false);
        }
        let said = match self.preset_fixed() {
            true => &texts.new_with,
            false => &texts.switched,
        };
        let said = said.replace("{name}", &self.preset_shown(&name));
        self.close_presets();
        self.new_session(Some(name));
        self.hint(said, true);
    }

    /// `d`：删除选中的。自带的两个不能删；第一次按只提示，`again` 是上一次按 `d` 时等着确认的那个，还是它才删。
    fn preset_delete(&mut self, again: Option<String>) {
        let Some(name) = self.boxes.presets.as_ref().and_then(PresetList::picked) else {
            return;
        };
        let texts = &self.config.text().presets;
        if name == FULL || name == BARE {
            let said = texts.built_in_fixed.clone();
            return self.hint(said, false);
        }
        let shown = self.preset_shown(&name);
        if again.as_deref() != Some(name.as_str()) {
            let said = texts.delete_again.replace("{name}", &shown);
            if let Some(list) = &mut self.boxes.presets {
                list.deleting = Some(name);
            }
            return self.hint(said, false);
        }
        let said = texts.deleted.replace("{name}", &shown);
        // 还没开的会话正要用它的：回到默认预设。会话已经开了的不动它（它照造它时记下的那一张走）。
        if !self.preset_fixed() && self.link.next_preset.as_deref() == Some(name.as_str()) {
            self.link.next_preset = None;
            self.link.core.send(Command::New { preset: None });
        }
        self.link.core.send(Command::PresetDelete(name));
        self.hint(said, true);
    }

    /// `e`、`a`：打开配置界面「预设」那一页的表单（`settings/presets.rs`）。`new` 的新建一个；不是的编辑选中的（自带的两个是
    /// 以它为底新建一份）。表单里按 `Esc` 回对话，这个框接着开着（[`App::presets_back`]）。
    fn preset_edit(&mut self, new: bool) {
        let picked = self.boxes.presets.as_ref().and_then(PresetList::picked);
        let target = match (new, picked) {
            (true, _) | (false, None) => crate::settings::PresetTarget::New(None),
            (false, Some(name)) if name == FULL || name == BARE => {
                crate::settings::PresetTarget::New(Some(name))
            }
            (false, Some(name)) => crate::settings::PresetTarget::Edit(name),
        };
        self.open_settings(Outcome::Back);
        // 配置界面开不了的：原因已经提示了，框照旧开着。
        if let Some(settings) = &mut self.settings {
            settings.open_preset(target);
            self.boxes.presets_return = true;
        }
    }

    /// 配置界面关了：是从 `/preset` 的框进去的，回到这个框，重新要一次列表，停在刚才编辑的那一个上。
    pub(super) fn presets_back(&mut self, edited: Option<String>) {
        if !std::mem::take(&mut self.boxes.presets_return) {
            return;
        }
        let want = edited.or_else(|| self.boxes.presets.as_ref().and_then(PresetList::picked));
        self.open_presets_at(want);
    }

    /// 读一次配置文件里的预设（界面线程里，只认预设那几项）：`/new <名字>` 和它的补全用。应急模式不读配置，只有自带的两个。
    fn presets_read(&self) -> Listed {
        if self.rescue.is_some() {
            return Listed {
                default: FULL.to_string(),
                custom: Vec::new(),
            };
        }
        Paths::locate(&Env::current())
            .map(|paths| presets::read(&paths))
            .unwrap_or_default()
    }

    /// `/new`，后面的字是预设的名字：不写的照默认预设，`bare` 无功能，`full` 全部功能，别的是自己定义的预设。没有这个预设的
    /// 弹一句、不开。
    pub(super) fn new_session_with(&mut self, words: Option<&str>) {
        let Some(name) = words else {
            return self.new_session(None);
        };
        if !self.presets_read().has(name) {
            let texts = &self.config.text().presets;
            let said = match self.rescue.is_some() {
                true => texts.rescue.clone(),
                false => texts.unknown.replace("{name}", &crate::cli::clean(name)),
            };
            return self.hint(said, false);
        }
        self.new_session(Some(name.to_string()));
    }

    /// 输入框里是 `/new <打了一半的名字>` 时按 `Tab`：补成预设的名字。只有一个对得上的补全它；有好几个的补到它们共同的开头，
    /// 再按一次换下一个。交回办没办（不是这种情形的交回假，`Tab` 照常给输入框）。
    pub(super) fn complete_preset(&mut self) -> bool {
        let text = self.typing.input.editor.text().to_string();
        let Some(typed) = new_argument(&text, &self.config.commands) else {
            return false;
        };
        let listed = self.presets_read();
        let names: Vec<&str> = listed.names().collect();
        if let Some(next) = completed(typed, &names) {
            let command = &text[..text.len() - typed.len()];
            self.typing.input.editor.take();
            self.typing.input.paste(&format!("{command}{next}"));
        }
        true
    }
}

/// 还没钉下预设的会话（还没开，或者第一句话刚发出去、引擎还没说）会用哪个预设：定好了的下一个（`--preset`、`/new <名字>`、
/// `/preset` 里选的），没定的照默认预设。要开的旧会话还没补完时不知道（它用的是它自己钉着的）。
pub fn ahead(talk: &super::talk::Talk, link: &super::link::Link) -> Option<Pinned> {
    if talk.main_transcript().session.is_some() || talk.opening != super::Opening::None {
        return None;
    }
    let name = link
        .next_preset
        .as_deref()
        .or(link.default_preset.as_deref())
        .unwrap_or(FULL);
    Some(Pinned {
        name: name.to_string(),
        state: PinState::Same,
    })
}

/// 侧边栏会话那一段写的预设（`ui/sidebar.rs`）：「预设 名字」，名字照 [`named`]。引擎还没说的没有。
pub fn side(pinned: Option<&Pinned>, text: &crate::config::Texts) -> Option<String> {
    let shown = named(pinned?, text);
    Some(text.presets.side.replace("{name}", &shown))
}

/// 会话钉着的预设给人看的名字：自带的两个照界面语言，后来改了、删了的带上说明。侧边栏、`/status` 共用。
pub fn named(pinned: &Pinned, text: &crate::config::Texts) -> String {
    let texts = &text.presets;
    let name = match pinned.name.as_str() {
        FULL => texts.full.clone(),
        BARE => texts.bare.clone(),
        name => crate::cli::clean(name),
    };
    match pinned.state {
        PinState::Same => name,
        PinState::Changed => texts.changed.replace("{name}", &name),
        PinState::Gone => texts.gone.replace("{name}", &name),
    }
}

/// 输入框里的字是 `/new <名字>`（名字可以还没打）时那个名字：命令是 `/new`（或它的别名），后面正好一个空格，名字里
/// 没有空白。
fn new_argument<'a>(text: &'a str, commands: &crate::commands::Commands) -> Option<&'a str> {
    let name = crate::commands::typed(text)?;
    let spec = commands.find(name)?;
    if spec.run != crate::commands::Run::New {
        return None;
    }
    let typed = text[1 + name.len()..].strip_prefix(' ')?;
    (!typed.contains(char::is_whitespace)).then_some(typed)
}

/// 照打了一半的 `typed` 补：对得上的（以它开头）只有一个的是它；有好几个的先补到共同的开头，没有更长的共同开头的是第一个。
/// `typed` 已经正好是一个预设的名字的，换成列表里的下一个（到头回第一个）：连按 `Tab` 一个个过。一个都对不上的没有。
fn completed(typed: &str, names: &[&str]) -> Option<String> {
    if let Some(at) = names.iter().position(|name| *name == typed) {
        return Some(names[(at + 1) % names.len()].to_string());
    }
    let matching: Vec<&str> = names
        .iter()
        .copied()
        .filter(|name| name.starts_with(typed))
        .collect();
    let first = matching.first()?;
    let lead = common(matching.iter().copied());
    match lead.len() > typed.len() {
        true => Some(lead),
        false => Some((*first).to_string()),
    }
}

/// 几个名字共同的开头（照字符算）。
fn common<'a>(mut names: impl Iterator<Item = &'a str>) -> String {
    let Some(first) = names.next() else {
        return String::new();
    };
    let mut lead: Vec<char> = first.chars().collect();
    for name in names {
        let same = lead
            .iter()
            .zip(name.chars())
            .take_while(|(a, b)| **a == *b)
            .count();
        lead.truncate(same);
    }
    lead.into_iter().collect()
}

#[cfg(test)]
mod tests;
