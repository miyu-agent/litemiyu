//! 「预设」页（LM-29，`docs/00-设计.md` 第六节第 13、14 条）：新建、编辑、删除预设。不照清单画，主菜单里排在 MCP 页前面。
//! 对话里 `/preset` 的框按 `e`、`a` 打开的就是这一页和它的预设窗（[`Settings::open_preset`]），不另做一套编辑界面。
//!
//! 照 Miyu 终端的预设页和预设的窗（`settings/pages/keys.rs` 的 `section_key`、`settings/pages/preset_edit.rs`、
//! `features.rs`、`features/grid.rs`，蓝图「配置页」第 38、39 条；LM-33，2026-11-21 项目主人：「编辑预设的操作逻辑和
//! miyu-agent不一致」）：
//!
//! - 列表：`j` `k` 挪，`Enter` 打开这个预设的窗，`a` 新建（先填名字，建好就开它的窗），`d` 先问再删，`Esc` `q` `h` `←` 回
//!   主菜单。默认预设在通用页「默认预设」那一项设（Miyu 也在通用页，`preset.default`），这一页不设。
//! - 预设的窗是浮在页上的窗，不是换一整页：名字一行，空一行，下面内置工具、MCP 服务、技能各一组，放得下排成两列（Miyu 的
//!   功能树），放不下排一列。`j` `k` 在一列里上下走（到顶回名字那一行），`h` `l` 换到另一列离得最近的那一行；空格、`Tab`、
//!   `Enter` 开关这一项，`Ctrl+A` 有没开的就全开、都开着才全关；名字那一行 `Enter` 在窗里改名；`d` 先问再删；`Esc` `q` 关窗。
//!   改一项存一项，不用另按保存。
//!
//! 和 Miyu 不同的：
//!
//! - 自带的两个（全部功能、无功能）不能改、不能删：在它们上面按 `Enter` 是以它为底新建一份（Miyu 的出厂预设能改）。
//! - 功能只有三组，一项就是一件工具、一个 MCP 服务、一个技能，没有功能下面再分工具的那一层；没有描述、编号那两行。
//! - 一项后面写它为什么不可用：MCP 页、技能页的全局开关关着的，预设里开了也不可用。名单里写了、现在已经没有的工具、
//!   服务、技能单列一组，空格、`Enter` 把它从名单里去掉（Miyu 是暗着、光标不停）。
//!
//! 预设写在 `config.toml` 的 `[presets.<名字>]`（`lm_engine::presets`），照平常改配置的路子存（[`Settings::save_edits`]）。
//! 存盘的格式还是 `unlisted` 和 `features`，窗里不露 `unlisted`：一项一项开关写这一项自己的键；`Ctrl+A` 全开写
//! `unlisted = "on"`、全关写 `"off"`，同时去掉现有各项单独写的键。以后新装的 MCP 服务、新加的技能照 `unlisted`：全开过的
//! 预设跟着开，全关过再挑的不跟着开。读技能目录、工具的显示名只在开窗时做一次。

use std::cell::Cell;

use crossterm::event::{KeyCode, KeyEvent};
use lm_engine::presets::{self, BARE, DEFAULT, FULL, Listed};
use lm_engine::settings::Edit;
use lm_kernel::config::Value;
use lm_kernel::config::mcp::configured;
use lm_kernel::policy::PresetPin;

use super::popup::{self, Confirm};
use super::texts::fill;
use super::{Outcome, Popup, Settings, Target, Tone, View, Window, writer};

/// 新建的预设（`a`，没有以谁为底）一开始全部打开（同 Miyu：功能不写就是全开）；要「只开几样」的 `Ctrl+A` 全关再开要的。
/// 以自带的为底新建的不看它（全部功能为底的开，无功能为底的关）。
const NEW_PRESET_UNLISTED: bool = true;

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
    /// 开着的预设的窗；没有的在看列表。
    pub form: Option<Form>,
    /// 是从对话里的 `/preset` 框直接进来的：窗里按 `Esc`、填名字的窗取消了，直接回对话。
    direct: bool,
    /// 这一下按键以后要回对话。
    leave: bool,
    /// 最后编辑、新建的那个预设的名字：回到 `/preset` 的框时停在它上面。
    pub edited: Option<String>,
}

/// 一个预设的窗。
#[derive(Debug)]
pub struct Form {
    /// 在编辑哪个预设。
    pub name: String,
    /// 光标停在哪：`None` 是名字那一行，不然是那一项的编号。
    pub at: Option<String>,
    /// 上一次画的时候排成了两列没有（Miyu `PresetView::two`）：`h` `l` 照它换列。
    pub two: Cell<bool>,
    /// 窗里从第几行露起（照上一帧接着露，选中的碰到上下边才挪）。
    pub top: Cell<usize>,
    /// 现在有的内置工具、MCP 服务、技能：开窗时读一次。
    known: Known,
}

/// 现在有的三样：编号、给人看的名字。
#[derive(Debug, Default)]
struct Known {
    tools: Vec<(String, String)>,
    mcp: Vec<(String, String)>,
    skills: Vec<(String, String)>,
}

/// 名单里写了、现在已经没有的那一组是第几组。
pub const MISSING: usize = 3;

/// 窗里的一项。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Feature {
    /// 编号：工具名、`mcp-<服务名>`、`skill-<技能名>`。
    pub id: String,
    /// 编号去掉 `mcp-`、`skill-` 的那一截：工具名、服务名、技能名（窗的右上角写它）。
    pub name: String,
    /// 给人看的名字：工具、技能的显示名；没有的是空的（照 `name` 写）。
    pub shown: String,
    /// 在第几组：内置工具、MCP 服务、技能、已经没有的（第 0 到 3 组）。
    pub group: usize,
    /// 在这个预设里开着没有。
    pub on: bool,
    /// MCP 页、技能页的全局开关关着：预设里开了也不可用。
    pub global_off: bool,
    /// 名单里写了、现在已经没有了。
    pub missing: bool,
}

/// 窗里功能那一块的一格（Miyu `features::Row`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    /// 一组的名字（第几组），后面写开着几项、一共几项；光标不停。
    Group(usize),
    /// 一组一项都没有：写「还没有…」（第几组），光标不停。
    Empty(usize),
    /// 组和组之间空的一行。
    Blank,
    /// 一项（[`Tree::features`] 的第几个）。
    Feature(usize),
}

/// 窗里功能那一块：一项项，照组的先后。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tree {
    /// 一项项。
    pub features: Vec<Feature>,
    /// 画出来的几组（第几组）：内置工具一直有，MCP 服务、技能没有的也画（写「还没有…」），已经没有的那一组有才画。
    pub groups: Vec<usize>,
}

impl Tree {
    /// 一组的几格：组名（已经没有的那一组不写，同 Miyu 把没装的排在最后一段、不写组名），一项都没有的写「还没有…」，
    /// 再是一项项。
    fn block(&self, group: usize) -> Vec<Slot> {
        let mut out = Vec::new();
        if group < MISSING {
            out.push(Slot::Group(group));
        }
        let members: Vec<Slot> = (0..self.features.len())
            .filter(|&i| self.features[i].group == group)
            .map(Slot::Feature)
            .collect();
        if members.is_empty() {
            out.push(Slot::Empty(group));
        }
        out.extend(members);
        out
    }

    /// 一组里开着几项、一共几项（组名后面写；同 Miyu `features::tally`）。
    pub fn tally(&self, group: usize) -> (usize, usize) {
        self.features
            .iter()
            .filter(|feature| feature.group == group)
            .fold((0, 0), |(on, all), feature| {
                (on + usize::from(feature.on), all + 1)
            })
    }

    /// 编号是 `id` 的那一项是第几个。
    pub fn find(&self, id: &str) -> Option<usize> {
        self.features.iter().position(|feature| feature.id == id)
    }
}

/// 排好的一列或两列（Miyu `features::Grid`），每一列从上往下一格格（组和组之间空一行）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Grid {
    /// 一列或两列。
    pub cols: Vec<Vec<Slot>>,
}

impl Grid {
    /// 排：`two` 的在组和组之间切成左右两列，切在两列行数最接近的地方，先后不变；不然排一列。
    pub fn new(tree: &Tree, two: bool) -> Grid {
        let blocks: Vec<Vec<Slot>> = tree.groups.iter().map(|&g| tree.block(g)).collect();
        let at = if two { split(&blocks) } else { blocks.len() };
        let mut cols = vec![column(&blocks[..at])];
        if at < blocks.len() {
            cols.push(column(&blocks[at..]));
        }
        Grid { cols }
    }

    /// 光标停得上的一项项，照先后：左列从上往下，再右列。
    pub fn stops(&self) -> Vec<usize> {
        self.cols
            .iter()
            .flatten()
            .filter_map(|cell| match cell {
                Slot::Feature(i) => Some(*i),
                _ => None,
            })
            .collect()
    }

    /// 第 `i` 项在第几列第几行。
    pub fn find(&self, i: usize) -> Option<(usize, usize)> {
        self.cols.iter().enumerate().find_map(|(c, col)| {
            col.iter()
                .position(|cell| *cell == Slot::Feature(i))
                .map(|row| (c, row))
        })
    }

    /// 同一列往上一项；到顶了是 `None`（交给外面：回到名字那一行）。
    pub fn up(&self, i: usize) -> Option<usize> {
        let (c, row) = self.find(i)?;
        self.cols[c][..row].iter().rev().find_map(feature_of)
    }

    /// 同一列往下一项；到底了是 `None`。
    pub fn down(&self, i: usize) -> Option<usize> {
        let (c, row) = self.find(i)?;
        self.cols[c][row + 1..].iter().find_map(feature_of)
    }

    /// 换到另一列（`right` 往右）离得最近的那一项；没有另一列的是 `None`。
    pub fn side(&self, i: usize, right: bool) -> Option<usize> {
        let (c, row) = self.find(i)?;
        let other = if right { c + 1 } else { c.checked_sub(1)? };
        self.cols
            .get(other)?
            .iter()
            .enumerate()
            .filter_map(|(k, cell)| feature_of(cell).map(|f| (k, f)))
            .min_by_key(|(k, _)| k.abs_diff(row))
            .map(|(_, f)| f)
    }
}

/// 一格是一项的，交回它是第几项。
fn feature_of(cell: &Slot) -> Option<usize> {
    match cell {
        Slot::Feature(i) => Some(*i),
        _ => None,
    }
}

/// 几组接成一列：组和组之间空一行。
fn column(blocks: &[Vec<Slot>]) -> Vec<Slot> {
    let mut out = Vec::new();
    for (k, block) in blocks.iter().enumerate() {
        if k > 0 {
            out.push(Slot::Blank);
        }
        out.extend(block.iter().copied());
    }
    out
}

/// 在第几组前面切：左列至少一组，切在两列行数（连同组间的空行）最接近的地方，一样近的切在前面（同 Miyu `grid::split`）。
fn split(blocks: &[Vec<Slot>]) -> usize {
    let lines = |part: &[Vec<Slot>]| {
        part.iter().map(Vec::len).sum::<usize>() + part.len().saturating_sub(1)
    };
    (1..=blocks.len())
        .min_by_key(|&at| lines(&blocks[..at]).abs_diff(lines(&blocks[at..])))
        .unwrap_or(blocks.len())
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

    /// 从对话里的 `/preset` 框进来：直接停在「预设」页，照 `target` 开预设的窗或者开填名字的窗（同 Miyu `Settings::one`、
    /// `create_one`：不经过主菜单）。办完按 `Esc` 直接回对话。
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

    /// 预设的窗开着没有。
    pub fn preset_window_open(&self) -> bool {
        self.presets
            .as_ref()
            .is_some_and(|open| open.form.is_some())
    }

    /// 「预设」页的按键。
    pub(super) fn presets_key(&mut self, page: usize, at: usize, key: KeyEvent) -> Outcome {
        // 按一个键先清掉上一句，正在办的留着（同别的页）。
        self.status = self.status.take().filter(|(_, tone)| *tone == Tone::Busy);
        match self.preset_window_open() {
            true => self.preset_form_key(key),
            false => self.preset_list_key(page, at, key),
        }
        match self.presets_leaving() {
            true => Outcome::Back,
            false => Outcome::Stay,
        }
    }

    /// 列表（同 Miyu `section_key`）：上下挪，`Enter` 打开这个预设的窗（自带的是以它为底新建），`a` 新建，`d` 删除，
    /// `Esc` `q` `h` `←` 回主菜单。
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
            KeyCode::Enter => match picked.as_str() {
                FULL | BARE => self.ask_new(Some(picked.clone())),
                _ => self.open_preset_form(&picked),
            },
            KeyCode::Char('a') => self.ask_new(None),
            KeyCode::Char('d') => self.ask_delete(&picked),
            _ => {}
        }
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

    /// 问过了：选了「删除」的从配置里去掉这个预设；它正好是默认预设的，默认预设回到全部功能。它的窗开着的关窗（从
    /// `/preset` 的框直接进来的回对话）。
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
        if !self.save_preset(edits, said) {
            return;
        }
        if self
            .presets
            .as_ref()
            .and_then(|open| open.form.as_ref())
            .is_some_and(|form| form.name == name)
        {
            self.close_form();
        }
        if let View::Page { page, at } = self.view {
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

    /// 填好了新预设的名字：照 `base` 写一份（没有底的、以全部功能为底的全开，以无功能为底的全关），开它的窗。
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

    /// 窗里改好了名字：把原来那一张的每一项挪到新名字下面；它是默认预设的，默认预设跟着改。
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
        let said = self.texts.presets.saved.clone();
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

    /// 开预设 `name` 的窗：读一次现在有的内置工具、MCP 服务、技能，光标停在名字那一行。
    fn open_preset_form(&mut self, name: &str) {
        let known = self.known();
        if let Some(open) = &mut self.presets {
            open.edited = Some(name.to_string());
            open.form = Some(Form {
                name: name.to_string(),
                at: None,
                two: Cell::new(true),
                top: Cell::new(0),
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

    /// 开着的窗在编辑的那一张（照这一刻的配置）；窗没开、预设没有了的是 `None`。
    pub fn preset_form(&self) -> Option<(&Form, PresetPin)> {
        let form = self.presets.as_ref()?.form.as_ref()?;
        let pin = self
            .presets_listed()
            .custom
            .into_iter()
            .find(|pin| pin.name == form.name)?;
        Some((form, pin))
    }

    /// 窗里功能那一块：内置工具、MCP 服务、技能各一组，最后是名单里写了、现在已经没有的（有才画）。
    pub fn preset_tree(&self) -> Tree {
        let Some((form, pin)) = self.preset_form() else {
            return Tree::default();
        };
        let values = self.snapshot.resolved.values();
        let global = lm_engine::skills::switches::of(&values);
        let mut tree = Tree {
            features: Vec::new(),
            groups: vec![0, 1, 2],
        };
        let mut listed = std::collections::BTreeSet::new();
        let groups = [&form.known.tools, &form.known.mcp, &form.known.skills];
        for (group, known) in groups.into_iter().enumerate() {
            for (id, shown) in known {
                listed.insert(id.as_str());
                let name = id
                    .strip_prefix("mcp-")
                    .or_else(|| id.strip_prefix("skill-"))
                    .filter(|_| group > 0)
                    .unwrap_or(id);
                tree.features.push(Feature {
                    id: id.clone(),
                    name: name.to_string(),
                    shown: shown.clone(),
                    group,
                    on: pin.on(id),
                    global_off: group > 0 && !lm_engine::skills::switches::on(&global, id),
                    missing: false,
                });
            }
        }
        let gone: Vec<&String> = pin
            .features
            .keys()
            .filter(|id| !listed.contains(id.as_str()))
            .collect();
        if !gone.is_empty() {
            tree.groups.push(MISSING);
            tree.features.extend(gone.into_iter().map(|id| Feature {
                id: id.clone(),
                name: id.clone(),
                shown: String::new(),
                group: MISSING,
                on: pin.on(id),
                global_off: false,
                missing: true,
            }));
        }
        tree
    }

    /// 预设的窗的按键（同 Miyu `preset_key`）：`j` `k` 在一列里上下走（到顶回名字那一行），`h` `l` 换列；空格、`Tab`、
    /// `Enter` 开关这一项（已经没有的从名单里去掉）；名字那一行 `Enter` 改名；`d` 先问再删；`Esc` `q` 关窗回列表（从
    /// `/preset` 的框直接进来的回对话）。`Ctrl+A` 在 [`Settings::key`] 里接（[`Settings::preset_toggle_all`]）。
    fn preset_form_key(&mut self, key: KeyEvent) {
        let tree = self.preset_tree();
        let Some((name, at, two)) = self
            .preset_form()
            .map(|(form, _)| (form.name.clone(), form.at.clone(), form.two.get()))
        else {
            // 在编辑的那一张没有了（别的窗口删了）：关窗。
            return self.close_form();
        };
        let grid = Grid::new(&tree, two);
        let here = at.as_deref().and_then(|id| tree.find(id));
        let go = |settings: &mut Settings, to: Option<usize>| {
            let id = to.map(|i| tree.features[i].id.clone());
            if let Some(form) = settings
                .presets
                .as_mut()
                .and_then(|open| open.form.as_mut())
            {
                form.at = id;
            }
        };
        match (key.code, here) {
            (KeyCode::Esc | KeyCode::Char('q'), _) => self.close_form(),
            (KeyCode::Down | KeyCode::Char('j'), Some(i)) => {
                go(self, Some(grid.down(i).unwrap_or(i)))
            }
            (KeyCode::Down | KeyCode::Char('j'), None) => {
                if let Some(first) = grid.stops().first() {
                    go(self, Some(*first));
                }
            }
            (KeyCode::Up | KeyCode::Char('k'), Some(i)) => go(self, grid.up(i)),
            (KeyCode::Left | KeyCode::Right | KeyCode::Char('h' | 'l'), Some(i)) => {
                let right = matches!(key.code, KeyCode::Right | KeyCode::Char('l'));
                go(self, Some(grid.side(i, right).unwrap_or(i)));
            }
            (KeyCode::Char('d'), _) => self.ask_delete(&name),
            (KeyCode::Enter, None) => {
                if self.warn_broken() {
                    return;
                }
                let title = self.texts.presets.name_row.clone();
                let edit = popup::Edit::new(title, name.clone(), false);
                self.popup = Some(Popup::new(Window::Edit(edit), Target::PresetRename(name)));
            }
            (KeyCode::Char(' ') | KeyCode::Tab | KeyCode::Enter, Some(i)) => {
                let feature = &tree.features[i];
                let key = presets::feature_key(&name, &feature.id);
                if feature.missing {
                    // 现在已经没有的：从名单里去掉；光标停到同一个位置上的那一项。
                    let stops = grid.stops();
                    let place = stops.iter().position(|&s| s == i).unwrap_or(0);
                    self.form_save(vec![Edit::Unset(key)]);
                    let tree = self.preset_tree();
                    let stops = Grid::new(&tree, two).stops();
                    let next = stops.get(place.min(stops.len().saturating_sub(1)));
                    let id = next.map(|&s| tree.features[s].id.clone());
                    if let Some(form) = self.presets.as_mut().and_then(|open| open.form.as_mut()) {
                        form.at = id;
                    }
                } else {
                    self.form_save(vec![Edit::Set(key, Value::Bool(!feature.on))]);
                }
            }
            _ => {}
        }
    }

    /// 窗里 `Ctrl+A`（同 Miyu `toggle_all`）：现在有的项目里有没开的就全开（`unlisted = "on"`），都开着才全关
    /// （`unlisted = "off"`）；同时去掉现有各项单独写的键，一次存。已经没有的那几项不管。窗没开的交回 `false`。
    pub(super) fn preset_toggle_all(&mut self) -> bool {
        let Some((name, pin)) = self
            .preset_form()
            .map(|(form, pin)| (form.name.clone(), pin))
        else {
            return false;
        };
        let tree = self.preset_tree();
        let present = tree.features.iter().filter(|feature| !feature.missing);
        let open = present.clone().any(|feature| !feature.on);
        let value = if open { "on" } else { "off" };
        let mut edits = vec![Edit::Set(
            presets::unlisted_key(&name),
            Value::Text(value.into()),
        )];
        edits.extend(
            present
                .filter(|feature| pin.features.contains_key(&feature.id))
                .map(|feature| Edit::Unset(presets::feature_key(&name, &feature.id))),
        );
        self.form_save(edits);
        true
    }

    /// 窗里改了一项：存。
    fn form_save(&mut self, edits: Vec<Edit>) {
        if self.warn_broken() {
            return;
        }
        let said = self.texts.presets.saved.clone();
        self.save_preset(edits, said);
    }

    /// 关窗：回列表；从 `/preset` 的框直接进来的回对话。
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
