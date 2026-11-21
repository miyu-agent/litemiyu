//! 「预设」页（LM-29，`docs/00-设计.md` 第六节第 13、14 条）：新建、编辑、删除预设，设默认预设。不照清单画，主菜单里排在
//! MCP 页前面。对话里 `/preset` 的框按 `e`、`a` 打开的就是这一页的表单（[`Settings::open_preset`]），不另做一套编辑界面。
//!
//! 照 Miyu 终端的预设页（`settings/pages/presets.rs`、`preset_edit.rs`）：进页先是预设的列表；`Enter` 进一个预设的表单，
//! 表单里名字一行、一个个功能一行，空格、`Enter` 开关，改一项存一项，不用另按保存；`a` 新建先填名字；`d` 先问再删。
//! 和 Miyu 不同的：
//!
//! - 表单画在页里（同技能页、MCP 页的一行行），不是悬浮窗；功能只有三组（内置工具、MCP 服务、技能），不分两列，没有
//!   `Ctrl+A` 全开全关、说明、图标。「没写的项目」那一行就是 Miyu 预设文件里的 `unlisted`：它管没有单独设置过的每一项，
//!   也管以后新装的 MCP 服务、新加的技能；先把它关上、再打开要的几项，就是「只开这几样」。
//! - 自带的两个（全部功能、无功能）不能改、不能删：在它们上面按 `Enter` 是以它为底新建一份（Miyu 的出厂预设能改）。
//! - 列表里空格把选中的设为默认预设（Miyu 的默认预设在通用页；LiteMiyu 通用页「默认预设」那一项也在，同一个键）。
//! - 一项后面写它为什么不可用：MCP 页、技能页的全局开关关着的，预设里开了也不可用。名单里写了、现在已经没有的工具、
//!   服务、技能单列一组，`Enter` 把它从名单里去掉。
//!
//! 预设写在 `config.toml` 的 `[presets.<名字>]`（`lm_engine::presets`），照平常改配置的路子存（[`Settings::save_edits`]）。
//! 读技能目录、工具的显示名只在开表单时做一次。

use crossterm::event::{KeyCode, KeyEvent};
use lm_engine::presets::{self, BARE, DEFAULT, FULL, Listed};
use lm_engine::settings::Edit;
use lm_kernel::config::Value;
use lm_kernel::config::mcp::configured;
use lm_kernel::policy::PresetPin;

use super::popup::{self, Confirm};
use super::texts::fill;
use super::{Outcome, Popup, Settings, Target, Tone, View, Window, writer};

/// 新建的预设（`a`，没有以谁为底）一开始「其余项目」开不开：`false` 是全部关闭，再打开要的几样（2026-11-21 项目主人要这个
/// 功能就是为了「只开 shell 和 web 相关」）；`true` 是全部打开（Miyu 的做法：功能不写就是全开）。要改只改这一行；以自带的
/// 为底新建的不看它（全部功能为底的开，无功能为底的关）。
const NEW_PRESET_UNLISTED: bool = false;

/// 从对话里的 `/preset` 框进来要办的。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PresetTarget {
    /// 新建一个：以自带的哪一个为底（`full`、`bare`）；`None` 是全开的一份。先开填名字的窗。
    New(Option<String>),
    /// 编辑叫这个名字的那一个。
    Edit(String),
}

/// 「预设」页开着时的状态。
#[derive(Debug, Default)]
pub struct Presets {
    /// 开着的表单；没有的在看列表。
    pub form: Option<Form>,
    /// 是从对话里的 `/preset` 框直接进来的：表单里按 `Esc`、填名字的窗取消了，直接回对话。
    direct: bool,
    /// 这一下按键以后要回对话。
    leave: bool,
    /// 最后编辑、新建的那个预设的名字：回到 `/preset` 的框时停在它上面。
    pub edited: Option<String>,
}

/// 一个预设的表单。
#[derive(Debug)]
pub struct Form {
    /// 在编辑哪个预设。
    pub name: String,
    /// 光标在第几行（[`Settings::preset_rows`]）。
    pub at: usize,
    /// 现在有的内置工具、MCP 服务、技能：开表单时读一次。
    known: Known,
}

/// 现在有的三样：编号、给人看的名字。
#[derive(Debug, Default)]
struct Known {
    tools: Vec<(String, String)>,
    mcp: Vec<(String, String)>,
    skills: Vec<(String, String)>,
}

/// 表单里的一行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormRow {
    /// 名字。
    Name,
    /// 没写的项目开不开（`unlisted`）。
    Unlisted,
    /// 一组的小标题：内置工具、MCP 服务、技能、已经没有的（第 0 到 3 组）。
    Group(usize),
    /// 一项。
    Feature(Feature),
}

/// 表单里的一项。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Feature {
    /// 编号：工具名、`mcp-<服务名>`、`skill-<技能名>`。
    pub id: String,
    /// 行首写的名字（工具名、服务名、技能名）。
    pub name: String,
    /// 名字后面暗色写的：工具、技能给人看的名字。
    pub shown: String,
    /// 在这个预设里开着没有。
    pub on: bool,
    /// 名单里单独写了它（没写的跟着「没写的项目」那一行）。
    pub explicit: bool,
    /// MCP 页、技能页的全局开关关着：预设里开了也不可用。
    pub global_off: bool,
    /// 名单里写了、现在已经没有了。
    pub missing: bool,
}

impl FormRow {
    /// 选不选得中：小标题跳过。
    pub fn selectable(&self) -> bool {
        !matches!(self, FormRow::Group(_))
    }
}

impl Settings {
    /// 配置里这一刻的预设和默认预设（照界面上已经算好的那一份配置，存了还没写完盘的也算）。
    pub fn presets_listed(&self) -> Listed {
        let values = self.snapshot.resolved.values();
        let default = match values.get(DEFAULT) {
            Some(Value::Text(name)) => name.to_string(),
            _ => FULL.to_string(),
        };
        Listed {
            default,
            custom: presets::custom(&values),
        }
    }

    /// 预设 `name` 给人看的名字：自带的两个照界面语言，自己定义的原样。
    pub fn preset_shown(&self, name: &str) -> String {
        match name {
            FULL => self.texts.presets.built_in[0].clone(),
            BARE => self.texts.presets.built_in[1].clone(),
            _ => crate::cli::clean(name),
        }
    }

    /// 进「预设」页：停在列表。
    pub(super) fn enter_presets(&mut self) {
        self.presets = Some(Presets::default());
    }

    /// 「预设」页在主菜单里是第几项。
    fn presets_entry(&self) -> usize {
        (0..self.entries())
            .find(|&entry| self.extra(entry) == Some(super::Extra::Presets))
            .unwrap_or(0)
    }

    /// 从对话里的 `/preset` 框进来：直接停在「预设」页，照 `target` 开表单或者开填名字的窗。办完按 `Esc` 直接回对话。
    pub fn open_preset(&mut self, target: PresetTarget) {
        let page = self.presets_entry();
        self.note_models();
        self.presets = Some(Presets {
            direct: true,
            ..Presets::default()
        });
        self.status = None;
        let listed = self.presets_listed();
        match target {
            PresetTarget::Edit(name) => {
                let at = listed.names().position(|n| n == name).unwrap_or(0);
                self.view = View::Page { page, at };
                match listed.custom.iter().any(|pin| pin.name == name) {
                    true => self.open_preset_form(&name),
                    // 没有这个预设了（刚被别的窗口删了）：回对话。
                    false => self.leave_presets(),
                }
            }
            PresetTarget::New(base) => {
                self.view = View::Page { page, at: 0 };
                self.ask_new(base);
            }
        }
    }

    /// 最后编辑、新建的那个预设（回到 `/preset` 的框时停在它上面）。
    pub fn preset_edited(&self) -> Option<String> {
        self.presets.as_ref().and_then(|open| open.edited.clone())
    }

    /// 这一下按键以后要不要回对话（从 `/preset` 的框直接进来、办完了的）：问一次就放下。
    pub(super) fn presets_leaving(&mut self) -> bool {
        self.presets
            .as_mut()
            .is_some_and(|open| std::mem::take(&mut open.leave))
    }

    /// 从 `/preset` 的框直接进来的：记下要回对话。
    fn leave_presets(&mut self) {
        if let Some(open) = &mut self.presets {
            open.leave = true;
        }
    }

    /// 「预设」页的按键。
    pub(super) fn presets_key(&mut self, page: usize, at: usize, key: KeyEvent) -> Outcome {
        // 按一个键先清掉上一句，正在办的留着（同别的页）。
        self.status = self.status.take().filter(|(_, tone)| *tone == Tone::Busy);
        match self
            .presets
            .as_ref()
            .is_some_and(|open| open.form.is_some())
        {
            true => self.preset_form_key(key),
            false => self.preset_list_key(page, at, key),
        }
        match self.presets_leaving() {
            true => Outcome::Back,
            false => Outcome::Stay,
        }
    }

    /// 列表：上下挪，`Enter` 编辑（自带的是以它为底新建），`a` 新建，`d` 删除，空格设为默认预设，`Esc` `q` `←` 回主菜单。
    fn preset_list_key(&mut self, page: usize, at: usize, key: KeyEvent) {
        let listed = self.presets_listed();
        let names: Vec<String> = listed.names().map(str::to_string).collect();
        let at = at.min(names.len().saturating_sub(1));
        let picked = names.get(at).cloned().unwrap_or_default();
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Left | KeyCode::Char('h') => {
                self.presets = None;
                self.view = View::Menu(page);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let at = (at + 1).min(names.len().saturating_sub(1));
                self.view = View::Page { page, at };
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.view = View::Page {
                    page,
                    at: at.saturating_sub(1),
                };
            }
            KeyCode::Enter | KeyCode::Right | KeyCode::Char('l' | 'e') => match picked.as_str() {
                FULL | BARE => self.ask_new(Some(picked.clone())),
                _ => self.open_preset_form(&picked),
            },
            KeyCode::Char('a') => self.ask_new(None),
            KeyCode::Char('d') => self.ask_delete(&picked),
            KeyCode::Char(' ') => self.set_default(&picked),
            _ => {}
        }
    }

    /// 把预设 `name` 设为默认预设。
    fn set_default(&mut self, name: &str) {
        if self.warn_broken() {
            return;
        }
        let said = fill(
            &self.texts.presets.default_set,
            &[("name", &self.preset_shown(name))],
        );
        let edit = Edit::Set(DEFAULT.to_string(), Value::Text(name.to_string().into()));
        self.save_preset(vec![edit], said);
    }

    /// 存预设的几项：存成了状态行说 `said`，没存成的说原因。交回收没收下。
    fn save_preset(&mut self, edits: Vec<Edit>, said: String) -> bool {
        match self.save_edits(edits, None, writer::After::Say(said)) {
            Ok(()) => true,
            Err(why) => {
                self.status = Some((fill(&self.texts.failed, &[("why", &why)]), Tone::Bad));
                false
            }
        }
    }

    /// `d`：自带的不能删；自己定义的先问一句（停在「取消」）。
    fn ask_delete(&mut self, name: &str) {
        if name == FULL || name == BARE {
            self.status = Some((self.texts.presets.built_in_fixed.clone(), Tone::Note));
            return;
        }
        if self.warn_broken() {
            return;
        }
        let [ask, said, yes, no] = &self.texts.presets.delete_ask;
        let confirm = Confirm {
            title: fill(ask, &[("name", &self.preset_shown(name))]),
            lines: vec![said.clone()],
            buttons: vec![(yes.clone(), true), (no.clone(), false)],
            at: 1,
        };
        self.popup = Some(Popup::new(
            Window::Confirm(confirm),
            Target::PresetDelete(name.to_string()),
        ));
    }

    /// 问过了：选了「删除」的从配置里去掉这个预设；它正好是默认预设的，默认预设回到全部功能。
    pub(super) fn preset_delete(&mut self, name: &str, chosen: Option<usize>) {
        self.popup = None;
        if chosen != Some(0) {
            return;
        }
        let listed = self.presets_listed();
        let Some(pin) = listed.custom.iter().find(|pin| pin.name == name) else {
            return;
        };
        let said = fill(
            &self.texts.presets.deleted,
            &[("name", &self.preset_shown(name))],
        );
        let edits = presets::removed(pin, &listed.default);
        if self.save_preset(edits, said)
            && let View::Page { page, at } = self.view
        {
            // 光标不越过最后一行。
            let last = self.presets_listed().names().count().saturating_sub(1);
            self.view = View::Page {
                page,
                at: at.min(last),
            };
        }
    }

    /// `a`、在自带的上面按 `Enter`：开填名字的窗。`base` 是以自带的哪一个为底。
    fn ask_new(&mut self, base: Option<String>) {
        if self.warn_broken() {
            if self.presets.as_ref().is_some_and(|open| open.direct) {
                self.leave_presets();
            }
            return;
        }
        let title = match base.as_deref() {
            Some(base) => fill(
                &self.texts.presets.new_from,
                &[("name", &self.preset_shown(base))],
            ),
            None => self.texts.presets.new.clone(),
        };
        let edit = popup::Edit::new(title, String::new(), false);
        self.popup = Some(Popup::new(Window::Edit(edit), Target::PresetNew(base)));
    }

    /// 名字 `name` 用不了的原因（`except` 是改名时原来的名字，和它一样的不算重名）；能用的是 `None`。
    fn name_problem(&self, name: &str, except: Option<&str>) -> Option<String> {
        let texts = &self.texts.presets;
        if name.is_empty() {
            return Some(texts.name_empty.clone());
        }
        if name == FULL || name == BARE {
            return Some(texts.name_built_in.clone());
        }
        if !presets::valid_name(name) {
            return Some(fill(
                &texts.name_bad,
                &[("max", &presets::NAME_CHARS.to_string())],
            ));
        }
        if Some(name) != except && self.presets_listed().has(name) {
            return Some(texts.name_taken.clone());
        }
        None
    }

    /// 窗里填的名字（去掉两头的空白）；名字用不了的把原因写在窗里、窗留着，交回 `None`。
    fn typed_name(&mut self, except: Option<&str>) -> Option<String> {
        let name = match &self.popup {
            Some(Popup {
                window: Window::Edit(edit),
                ..
            }) => edit.text.trim().to_string(),
            _ => return None,
        };
        if let Some(why) = self.name_problem(&name, except) {
            if let Some(Popup {
                window: Window::Edit(edit),
                ..
            }) = &mut self.popup
            {
                edit.error = Some(why);
            }
            return None;
        }
        Some(name)
    }

    /// 填好了新预设的名字：照 `base` 写一份（没有底的、以全部功能为底的全开，以无功能为底的全关），开它的表单。
    pub(super) fn preset_new(&mut self, base: Option<&str>) {
        let Some(name) = self.typed_name(None) else {
            return;
        };
        self.popup = None;
        let pin = PresetPin {
            name: name.clone(),
            unlisted: match base {
                Some(BARE) => false,
                Some(_) => true,
                None => NEW_PRESET_UNLISTED,
            },
            features: std::collections::BTreeMap::new(),
        };
        let said = fill(
            &self.texts.presets.created,
            &[("name", &self.preset_shown(&name))],
        );
        if !self.save_preset(presets::written(&pin), said) {
            return;
        }
        if let View::Page { page, .. } = self.view {
            let at = self.presets_listed().names().position(|n| n == name);
            self.view = View::Page {
                page,
                at: at.unwrap_or(0),
            };
        }
        self.open_preset_form(&name);
    }

    /// 填名字的窗取消了：从 `/preset` 的框直接来新建的回对话。
    pub(super) fn preset_cancelled(&mut self, target: &Target) {
        let direct_new = matches!(target, Target::PresetNew(_))
            && self
                .presets
                .as_ref()
                .is_some_and(|open| open.direct && open.form.is_none());
        if direct_new {
            self.leave_presets();
        }
    }

    /// 表单里改好了名字：把原来那一张的每一项挪到新名字下面；它是默认预设的，默认预设跟着改。
    pub(super) fn preset_rename(&mut self, old: &str) {
        let Some(name) = self.typed_name(Some(old)) else {
            return;
        };
        self.popup = None;
        if name == old {
            return;
        }
        let listed = self.presets_listed();
        let Some(pin) = listed.custom.iter().find(|pin| pin.name == old) else {
            return;
        };
        let mut edits = presets::removed(pin, "");
        edits.extend(presets::written(&PresetPin {
            name: name.clone(),
            ..pin.clone()
        }));
        if listed.default == old {
            edits.push(Edit::Set(
                DEFAULT.to_string(),
                Value::Text(name.clone().into()),
            ));
        }
        let said = self.saved_said();
        if !self.save_preset(edits, said) {
            return;
        }
        if let Some(open) = &mut self.presets {
            open.edited = Some(name.clone());
            if let Some(form) = &mut open.form {
                form.name.clone_from(&name);
            }
        }
        if let View::Page { page, .. } = self.view {
            let at = self.presets_listed().names().position(|n| n == name);
            self.view = View::Page {
                page,
                at: at.unwrap_or(0),
            };
        }
    }

    /// 存成了说的那一句：预设改了以后新开的会话才用上。
    fn saved_said(&self) -> String {
        self.texts.presets.saved.clone()
    }

    /// 开预设 `name` 的表单：读一次现在有的内置工具、MCP 服务、技能，光标停在名字那一行。
    fn open_preset_form(&mut self, name: &str) {
        let known = self.known();
        if let Some(open) = &mut self.presets {
            open.edited = Some(name.to_string());
            open.form = Some(Form {
                name: name.to_string(),
                at: 0,
                known,
            });
        }
    }

    /// 现在有的内置工具（连给人看的名字）、配置里的 MCP 服务、找得到的技能。
    fn known(&self) -> Known {
        let human = crate::human::Human::load(&self.language);
        let tools = presets::TOOLS
            .iter()
            .map(|tool| ((*tool).to_string(), human.tool_name(tool)))
            .collect();
        let (servers, _) = configured(&self.snapshot.text);
        let mcp = servers
            .iter()
            .map(|server| (server.feature_id(), String::new()))
            .collect();
        let (found, _) = lm_engine::skills::find(self.service.paths());
        let skills = found
            .into_iter()
            .map(|skill| {
                let head = skill.head;
                let shown = head
                    .display_name
                    .get(&self.language)
                    .or_else(|| head.display_name.get("en"))
                    .cloned()
                    .unwrap_or_default();
                (lm_kernel::policy::skill::feature_id(&head.name), shown)
            })
            .collect();
        Known { tools, mcp, skills }
    }

    /// 开着的表单在编辑的那一张（照这一刻的配置）；表单没开、预设没有了的是 `None`。
    pub fn preset_form(&self) -> Option<(&Form, PresetPin)> {
        let form = self.presets.as_ref()?.form.as_ref()?;
        let pin = self
            .presets_listed()
            .custom
            .into_iter()
            .find(|pin| pin.name == form.name)?;
        Some((form, pin))
    }

    /// 表单的一行行：名字、没写的项目，再是内置工具、MCP 服务、技能各一组（没有的那一组不列），最后是名单里写了、现在已经
    /// 没有的。
    pub fn preset_rows(&self) -> Vec<FormRow> {
        let Some((form, pin)) = self.preset_form() else {
            return Vec::new();
        };
        let values = self.snapshot.resolved.values();
        let global = lm_engine::skills::switches::of(&values);
        let mut rows = vec![FormRow::Name, FormRow::Unlisted];
        let mut listed = std::collections::BTreeSet::new();
        let groups = [&form.known.tools, &form.known.mcp, &form.known.skills];
        for (group, known) in groups.into_iter().enumerate() {
            if known.is_empty() {
                continue;
            }
            rows.push(FormRow::Group(group));
            for (id, shown) in known {
                listed.insert(id.as_str());
                let name = id
                    .strip_prefix("mcp-")
                    .or_else(|| id.strip_prefix("skill-"))
                    .filter(|_| group > 0)
                    .unwrap_or(id);
                rows.push(FormRow::Feature(Feature {
                    id: id.clone(),
                    name: name.to_string(),
                    shown: shown.clone(),
                    on: pin.on(id),
                    explicit: pin.features.contains_key(id),
                    global_off: group > 0 && !lm_engine::skills::switches::on(&global, id),
                    missing: false,
                }));
            }
        }
        let gone: Vec<&String> = pin
            .features
            .keys()
            .filter(|id| !listed.contains(id.as_str()))
            .collect();
        if !gone.is_empty() {
            rows.push(FormRow::Group(3));
            rows.extend(gone.into_iter().map(|id| {
                FormRow::Feature(Feature {
                    id: id.clone(),
                    name: id.clone(),
                    shown: String::new(),
                    on: pin.on(id),
                    explicit: true,
                    global_off: false,
                    missing: true,
                })
            }));
        }
        rows
    }

    /// 表单：上下挪（跳过小标题），空格、`Enter` 改这一行（名字开填名字的窗，别的当场开关、存），`d` 把单独设置过的一项改回
    /// 跟着「没写的项目」，`Esc` `q` `←` 回列表（从 `/preset` 的框直接进来的回对话）。
    fn preset_form_key(&mut self, key: KeyEvent) {
        let rows = self.preset_rows();
        let Some((name, at)) = self
            .preset_form()
            .map(|(form, _)| (form.name.clone(), form.at))
        else {
            // 在编辑的那一张没有了（别的窗口删了）：回列表。
            return self.close_form();
        };
        let at = at.min(rows.len().saturating_sub(1));
        let moved = |at: usize, settings: &mut Settings| {
            if let Some(form) = settings
                .presets
                .as_mut()
                .and_then(|open| open.form.as_mut())
            {
                form.at = at;
            }
        };
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Left | KeyCode::Char('h') => {
                self.close_form();
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let next = (at + 1..rows.len()).find(|&i| rows[i].selectable());
                moved(next.unwrap_or(at), self);
            }
            KeyCode::Up | KeyCode::Char('k') => {
                let before = (0..at).rev().find(|&i| rows[i].selectable());
                moved(before.unwrap_or(at), self);
            }
            KeyCode::Enter | KeyCode::Char(' ') => match rows.get(at) {
                Some(FormRow::Name) => {
                    if self.warn_broken() {
                        return;
                    }
                    let title = self.texts.presets.rows[0].clone();
                    let edit = popup::Edit::new(title, name.clone(), false);
                    self.popup = Some(Popup::new(Window::Edit(edit), Target::PresetRename(name)));
                }
                Some(FormRow::Unlisted) => {
                    let on = self.preset_form().is_some_and(|(_, pin)| pin.unlisted);
                    let value = if on { "off" } else { "on" };
                    let edit = Edit::Set(presets::unlisted_key(&name), Value::Text(value.into()));
                    self.form_save(vec![edit]);
                }
                // 现在已经没有的：从名单里去掉。
                Some(FormRow::Feature(feature)) if feature.missing => {
                    self.form_save(vec![Edit::Unset(presets::feature_key(&name, &feature.id))]);
                }
                Some(FormRow::Feature(feature)) => {
                    let key = presets::feature_key(&name, &feature.id);
                    self.form_save(vec![Edit::Set(key, Value::Bool(!feature.on))]);
                }
                _ => {}
            },
            KeyCode::Char('d') => match rows.get(at) {
                Some(FormRow::Feature(feature)) if feature.explicit => {
                    self.form_save(vec![Edit::Unset(presets::feature_key(&name, &feature.id))]);
                }
                Some(FormRow::Feature(_)) => {
                    self.status = Some((self.texts.presets.follows.clone(), Tone::Note));
                }
                _ => {}
            },
            _ => {}
        }
    }

    /// 表单里改了一项：存。
    fn form_save(&mut self, edits: Vec<Edit>) {
        if self.warn_broken() {
            return;
        }
        let said = self.saved_said();
        self.save_preset(edits, said);
        // 去掉了一行的：光标不越过最后一行。
        let last = self.preset_rows().len().saturating_sub(1);
        if let Some(form) = self.presets.as_mut().and_then(|open| open.form.as_mut()) {
            form.at = form.at.min(last);
        }
    }

    /// 关表单：回列表；从 `/preset` 的框直接进来的回对话。
    fn close_form(&mut self) {
        let Some(open) = &mut self.presets else {
            return;
        };
        open.form = None;
        if open.direct {
            open.leave = true;
        }
    }

    /// 通用页「默认预设」那一项（`sessions.default`）：照现在有的预设开选择窗。
    pub(super) fn choose_default_preset(&mut self, title: String) {
        let listed = self.presets_listed();
        let names: Vec<String> = listed.names().map(|name| self.preset_shown(name)).collect();
        let current = listed.names().position(|name| name == listed.default);
        let choose = popup::Choose::new(title, names, current);
        self.popup = Some(Popup::new(
            Window::Choose(choose),
            Target::Item(DEFAULT.to_string()),
        ));
    }

    /// 「默认预设」的选择窗里选的第 `at` 个是哪个预设。
    pub(super) fn default_preset_at(&self, at: usize) -> Option<String> {
        self.presets_listed().names().nth(at).map(str::to_string)
    }
}

#[cfg(test)]
mod tests;
