//! 第一次运行的开头（照 Miyu 终端第一次打开的引导 `oobe/` 搬）：没有配置文件、或者上次没走完时起来就进这里。照 Miyu 的先后：
//! 语言 → 操作方式 → 图标 → 模型 → 配色方案 → 完成，「按 Enter 开始对话」进对话。第一步按 `Esc` 退出程序，下次起来还从
//! 这里开始（状态目录里的记号，`startup::opening_mark`）。
//!
//! 不搬的步（Miyu 有、LiteMiyu 没有这些功能）：欢迎页（吉祥物、星点动画）、货币、人格、关于你、预设、语义模型；完成那一屏的
//! 星点。配色方案那一步 Miyu 没建人格时跳过，LiteMiyu 没有人格、这一步照旧走（项目主人 2026-11-14 要开头的明暗即时预览）。
//! 接模型那一步在 `opening/model.rs`；语言、操作方式、图标照 Miyu `oobe/mod.rs` 的 `pick_key`。

mod field;
pub mod model;
pub mod more;
mod nav;

use crossterm::event::{KeyCode, KeyEvent};
use lm_kernel::config::{Kind, Value};

pub use field::{DOT, Field, unlink};
pub use model::{Form, ModelStep, Phase, Row, Slot};
use nav::Nav;

use super::{Choose, Outcome, Popup, Settings, Target, Tone, Window};
use crate::config::texts;

/// 走到哪一步（Miyu `oobe/step.rs`，只留 LiteMiyu 有的）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// 界面语言。
    Language,
    /// 你喜欢的操作方式。
    Keys,
    /// 图标。
    Icons,
    /// 接模型。
    Model,
    /// 配色方案。
    Theme,
    /// 完成。
    Done,
}

impl Step {
    /// 进度那一行里第几步；完成那一屏不画进度。
    pub fn index(self) -> Option<usize> {
        match self {
            Step::Language => Some(0),
            Step::Keys => Some(1),
            Step::Icons => Some(2),
            Step::Model => Some(3),
            Step::Theme => Some(4),
            Step::Done => None,
        }
    }

    fn next(self) -> Step {
        match self {
            Step::Language => Step::Keys,
            Step::Keys => Step::Icons,
            Step::Icons => Step::Model,
            Step::Model => Step::Theme,
            Step::Theme | Step::Done => Step::Done,
        }
    }

    fn previous(self) -> Step {
        match self {
            Step::Language | Step::Keys => Step::Language,
            Step::Icons => Step::Keys,
            Step::Model => Step::Icons,
            Step::Theme => Step::Model,
            Step::Done => Step::Theme,
        }
    }
}

/// 一步交回的（Miyu `Move`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Move {
    /// 留在这一步。
    Stay,
    /// 下一步。
    Next,
    /// 上一步。
    Back,
}

/// 选一行的一步（Miyu `oobe/pick.rs`）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Pick {
    /// 一行行。
    pub rows: Vec<String>,
    /// 选着第几行。
    pub selected: usize,
}

impl Pick {
    /// 这几行，选着第 `selected` 行（超出的停在最后一行）。
    pub fn new(rows: Vec<String>, selected: usize) -> Self {
        let selected = selected.min(rows.len().saturating_sub(1));
        Self { rows, selected }
    }

    /// 上下挪（vim 的 `j` `k` 也认）：挪了交回真，别的键交回假。
    pub fn key(&mut self, key: KeyEvent) -> bool {
        let last = self.rows.len().saturating_sub(1);
        match nav::plain(key) {
            Some(Nav::Up) => self.selected = self.selected.saturating_sub(1),
            Some(Nav::Down) => self.selected = (self.selected + 1).min(last),
            _ => return false,
        }
        true
    }
}

/// 配色方案那一步的几格（Miyu `oobe/theme.rs` 的 `Slot`，只留 LiteMiyu 有的）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThemeSlot {
    /// 明暗。
    Mode,
    /// 铺底色。
    Ground,
    /// 下一步。
    Next,
}

/// 配色方案那一步从上往下的几格。
pub const THEME_SLOTS: [ThemeSlot; 3] = [ThemeSlot::Mode, ThemeSlot::Ground, ThemeSlot::Next];

/// 明暗的三档，照这个先后（`tui.theme_mode`）。
pub const MODES: [&str; 3] = ["auto", "dark", "light"];

/// 操作方式那一步的两行（`tui.keys`，Miyu `KEY_MODES`）。
pub const KEY_MODES: [&str; 2] = ["arrows", "vim"];

/// 图标那一步的两行（`tui.icons`，Miyu `ICON_SETS`）。
pub const ICON_SETS: [&str; 2] = ["nerd", "plain"];

/// 开头走到哪。
#[derive(Debug)]
pub struct Opening {
    /// 第几步。
    pub step: Step,
    /// 语言那一步：第一行跟随系统，下面一种一行。
    pub language: Pick,
    /// 语言那一步每一行写进 `ui.language` 的值。
    codes: Vec<String>,
    /// 操作方式那一步。
    pub keys: Pick,
    /// 图标那一步。
    pub icons: Pick,
    /// 接模型那一步。
    pub model: ModelStep,
    /// 配色方案那一步光标在哪一格。
    pub theme: ThemeSlot,
    /// 配色方案那一步选好、还没存的几项（键、值）：停在这一步时界面照它换样子，「下一步」一起存（Miyu `ThemeStep` 的 `mode`、
    /// `ground`）。
    pub picks: Vec<(String, Value)>,
    /// 配色方案那一步「下一步」排给写线程了、还没写完（Miyu `ThemeStep::busy`）：写完才往下走。
    pub theme_saving: bool,
    /// 开头一起来时排了一次从 Miyu 的自动导入、导进来的几家还没照进接模型那一步（`import.rs`）。
    pub importing: bool,
}

texts! {
    /// 语言那一步的字。
    #[derive(Debug, Clone)]
    pub struct LanguageTexts {
        /// 标题。
        pub title: String,
        /// 说明。
        pub sub: String,
        /// 第一行：「跟随系统（{name}）」。
        pub auto: String,
    }
}

texts! {
    /// 操作方式那一步的字。
    #[derive(Debug, Clone)]
    pub struct ControlsTexts {
        /// 标题。
        pub title: String,
        /// 说明。
        pub sub: String,
        /// 方向键。
        pub arrows: String,
        /// 方向键后面暗色的一句。
        pub arrows_note: String,
        /// Vim。
        pub vim: String,
        /// Vim 后面暗色的一句。
        pub vim_note: String,
    }
}

texts! {
    /// 图标那一步的字。
    #[derive(Debug, Clone)]
    pub struct IconsTexts {
        /// 标题。
        pub title: String,
        /// 说明。
        pub sub: String,
        /// 看得到图标。
        pub yes: String,
        /// 是方块或者乱码。
        pub no: String,
    }
}

texts! {
    /// 接模型那一步的字（Miyu `oobe.model`）。
    #[derive(Debug, Clone)]
    pub struct ModelTexts {
        /// 选一家：标题。
        pub title: String,
        /// 选一家：说明。
        pub sub: String,
        /// 找到了环境变量里的 key。
        pub found: String,
        /// 配置里已经有了。
        pub configured: String,
        /// 段名「其他」。
        pub other: String,
        /// 更多供应商…
        pub more: String,
        /// 「更多供应商…」浮窗的标题。
        pub more_title: String,
        /// 浮窗里搜索那一格空着、在搜时暗色写的。
        pub more_search: String,
        /// 浮窗里一家都对不上。
        pub no_match: String,
        /// 自定义…
        pub custom: String,
        /// 自定义后面暗色的一句。
        pub custom_note: String,
        /// 已经有主对话模型：标题。
        pub ready_title: String,
        /// 「已配置：{name}」。
        pub ready: String,
        /// 使用。
        pub keep: String,
        /// 更换。
        pub change: String,
        /// 密钥。
        pub key: String,
        /// 「来自环境变量 {env}」。
        pub key_env: String,
        /// 无需密钥。
        pub key_none: String,
        /// 请填写密钥。
        pub key_needed: String,
        /// ID。
        pub id: String,
        /// ID 写法不对。
        pub id_bad: String,
        /// ID 已有。
        pub id_taken: String,
        /// 仍然保存。
        pub save_anyway: String,
        /// 仍然保存前要填模型名。
        pub model_for_save: String,
        /// 模型名下面：会真发一句。
        pub cost: String,
        /// 选了一家：说明。
        pub key_sub: String,
        /// 显示名称。
        pub name: String,
        /// 显示名称空着时暗色写的。
        pub name_hint: String,
        /// 地址。
        pub url: String,
        /// 地址写法不对。
        pub url_bad: String,
        /// 接口。
        pub driver: String,
        /// 三种接口的名字。
        pub drivers: [String; 3],
        /// 正在连接…
        pub testing: String,
        /// 「连接成功，找到 {count} 个模型」。
        pub connected: String,
        /// 「连接成功：{model} 可以用」。
        pub connected_request: String,
        /// 选模型：说明。
        pub pick_sub: String,
        /// 列不出模型：请填模型名。
        pub no_list: String,
        /// 模型名。
        pub model_name: String,
        /// 模型名空着时暗色写的。
        pub model_example: String,
        /// 「连接失败（{stage}）：{message}」。
        pub failed: String,
        /// 搜索模型。
        pub search: String,
        /// 「/ 搜索」。
        pub search_cue: String,
        /// 「↑ 还有 {n} 个」。
        pub more_above: String,
        /// 「↓ 还有 {n} 个」。
        pub more_below: String,
        /// 正在保存…
        pub saving: String,
        /// 测试连接。
        pub test: String,
        /// 自定义：标题。
        pub custom_title: String,
        /// 自定义：说明。
        pub custom_sub: String,
        /// 没通在哪一步：获取模型列表、发送请求、配置。
        pub stages: [String; 3],
    }
}

texts! {
    /// 配色方案那一步的字。
    #[derive(Debug, Clone)]
    pub struct ThemeTexts {
        /// 标题。
        pub title: String,
        /// 说明。
        pub sub: String,
        /// 明暗、铺底色。
        pub rows: [String; 2],
        /// 明暗的三档。
        pub modes: [String; 3],
        /// 铺底色的开、关。
        pub ground: [String; 2],
        /// 铺底色下面那一句。
        pub ground_note: String,
        /// 下一步。
        pub next: String,
    }
}

texts! {
    /// 完成那一屏的字。
    #[derive(Debug, Clone)]
    pub struct DoneTexts {
        /// 标题。
        pub title: String,
        /// 「模型 {model} · 人格 {persona} · 预设 {preset}」：值是空的那一段整段不写（Miyu `summary_line`）。
        pub summary: String,
        /// 按 Enter 开始对话。
        pub start: String,
    }
}

texts! {
    /// 最下面一行的按键提示（Miyu `oobe.keys`）。
    #[derive(Debug, Clone)]
    pub struct KeyTexts {
        /// 搜模型时。
        pub searching: String,
        /// 编辑一格时。
        pub editing: String,
        /// 停在「测试连接」。
        pub test: String,
        /// 停在「仍然保存」。
        pub anyway: String,
        /// 选一行的几屏。
        pub pick: String,
        /// 填的那一屏。
        pub form: String,
        /// 停在接口那一格。
        pub driver: String,
        /// 选模型。
        pub models: String,
        /// 「更多供应商…」窗（方向键）。
        pub more: String,
        /// 「更多供应商…」窗（Vim）。
        pub more_vim: String,
        /// 配色方案那一步。
        pub theme: String,
        /// 停着的一格右端：Enter 编辑。
        pub enter_edit: String,
        /// 停着的一格右端：Enter 选择。
        pub enter_pick: String,
        /// 第一步（LiteMiyu 没有欢迎页：Esc 退出）。
        pub first: String,
    }
}

texts! {
    /// 开头的字（照 Miyu 终端 `text/<语言>.json` 的 `oobe`）。
    #[derive(Debug, Clone)]
    pub struct OpeningTexts {
        /// 进度那一行的五步。
        pub steps: [String; 5],
        /// 语言。
        pub language: LanguageTexts,
        /// 操作方式。
        pub controls: ControlsTexts,
        /// 图标。
        pub icons: IconsTexts,
        /// 接模型。
        pub model: ModelTexts,
        /// 从 Miyu 导入（LiteMiyu 多的）：名字、暗色的一句。
        pub import: [String; 2],
        /// 配色方案。
        pub theme: ThemeTexts,
        /// 完成。
        pub done: DoneTexts,
        /// 按键提示。
        pub keys: KeyTexts,
    }
}

impl Settings {
    /// 进开头：写下没走完的记号（写不了的不管，下次照没有配置文件认），读常用的几家。
    pub fn start_opening(&mut self) {
        let mark = super::startup::opening_mark(self.service.paths());
        // 写不进（目录只读）的不碍事：没有配置文件的下次照样进开头。
        if let Some(dir) = mark.parent()
            && std::fs::create_dir_all(dir).is_ok()
        {
            std::fs::write(&mark, b"").unwrap_or_default();
        }
        // 目录读一次（缓存里取来的，LM-9：没有的是空的，走到接模型那一步时再去取）：常用的几家列在接模型那一步，「更多
        // 供应商…」也照它。
        let catalog = self.catalog();
        self.models = Some(super::models::Models::open(
            &self.snapshot,
            catalog,
            self.lists.clone(),
        ));
        let (language, codes) = self.language_pick();
        let keys = KEY_MODES
            .iter()
            .position(|mode| self.text_value("tui.keys") == *mode)
            .unwrap_or(1);
        let icons = ICON_SETS
            .iter()
            .position(|set| self.text_value("tui.icons") == *set)
            .unwrap_or(0);
        let texts = &self.texts.opening;
        let opening = Opening {
            step: Step::Language,
            language,
            codes,
            keys: Pick::new(
                vec![texts.controls.arrows.clone(), texts.controls.vim.clone()],
                keys,
            ),
            icons: Pick::new(vec![texts.icons.yes.clone(), texts.icons.no.clone()], icons),
            model: ModelStep::new(self),
            theme: ThemeSlot::Mode,
            picks: Vec::new(),
            theme_saving: false,
            importing: false,
        };
        self.opening = Some(opening);
        self.status = None;
        // 一家都没配、电脑上有 Miyu 的配置：不问，直接导进来（`import.rs`）；写完了接模型那一步照导进来的走。
        let importing = self.import_all_from_miyu();
        if let Some(opening) = &mut self.opening {
            opening.importing = importing;
        }
        // 已经配好一家的（重走开头）：照 Miyu 引导模型那一步，在后台把列表拉一遍（配置 五）。
        self.refresh_lists();
    }

    /// 语言那一步的几行和选着的（Miyu `app/oobe.rs` 的 `language_pick`）：第一行「跟随系统（这时的系统语言）」，下面照
    /// `ui.language` 的选项一种一行；名字照配置清单的字。
    fn language_pick(&self) -> (Pick, Vec<String>) {
        let Some(item) = self.schema.item("ui.language") else {
            return (Pick::default(), Vec::new());
        };
        let codes: Vec<String> = match item.item.kind {
            Kind::Option(options) => options.iter().map(|o| (*o).to_string()).collect(),
            _ => Vec::new(),
        };
        let name = |code: &str| {
            item.words
                .options
                .get(code)
                .cloned()
                .unwrap_or_else(|| code.to_string())
        };
        let system = crate::language::LanguageTable::builtin()
            .map(|table| table.detect(|var| std::env::var(var).ok()))
            .map(|language| language.code().to_string())
            .unwrap_or_default();
        let rows = codes
            .iter()
            .map(|code| match code.as_str() {
                lm_engine::settings::declared::AUTO => self
                    .texts
                    .opening
                    .language
                    .auto
                    .replace("{name}", &name(&system)),
                other => name(other),
            })
            .collect();
        let written = self.text_value("ui.language");
        let selected = codes.iter().position(|c| *c == written).unwrap_or(0);
        (Pick::new(rows, selected), codes)
    }

    /// 换界面语言（Miyu `switch_language`）：写 `ui.language`，开头、配置界面的字当场换成那一种；对话界面的字回对话时换
    /// （`app/settings.rs`）。
    fn use_language(&mut self, opening: &mut Opening, at: usize) {
        let Some(code) = opening.codes.get(at).cloned() else {
            return;
        };
        self.set_quietly("ui.language", Value::Text(code.clone().into()));
        let language = if code == lm_engine::settings::declared::AUTO {
            crate::language::LanguageTable::builtin()
                .map(|table| table.detect(|var| std::env::var(var).ok()))
                .map(|language| language.code().to_string())
                .unwrap_or_default()
        } else {
            code.clone()
        };
        if language == self.language {
            return;
        }
        if let (Ok(texts), Ok(schema)) = (
            super::texts::Texts::load(&language),
            lm_engine::settings::schema::schema(self.service.items(), &language),
        ) {
            self.texts = texts;
            self.schema = schema;
            self.language.clone_from(&language);
            self.switched = Some(code);
            let (pick, codes) = self.language_pick();
            opening.language = Pick {
                selected: at,
                ..pick
            };
            opening.codes = codes;
            let words = &self.texts.opening;
            opening.keys.rows = vec![words.controls.arrows.clone(), words.controls.vim.clone()];
            opening.icons.rows = vec![words.icons.yes.clone(), words.icons.no.clone()];
            opening.model.arrange(self);
        }
    }

    /// 开头里按一个键（窗都关着时）。
    pub(super) fn opening_key(&mut self, key: KeyEvent) -> Outcome {
        let Some(mut opening) = self.opening.take() else {
            return Outcome::Stay;
        };
        let moved = match opening.step {
            Step::Language | Step::Keys | Step::Icons => self.opening_pick_key(&mut opening, key),
            Step::Model => self.model_key(&mut opening, key),
            Step::Theme => self.theme_key(&mut opening, key),
            Step::Done => {
                if key.code == KeyCode::Enter {
                    return self.opening_done();
                }
                Move::Stay
            }
        };
        if moved == Move::Back && opening.step == Step::Language {
            return Outcome::Quit;
        }
        match moved {
            Move::Stay => {}
            Move::Next => opening.step = opening.step.next(),
            Move::Back => opening.step = opening.step.previous(),
        }
        if moved != Move::Stay {
            self.status = None;
        }
        // 走到接模型那一步（LM-9）：要用到目录了，缓存里没有、旧了的在后台取（在取、取完的那一句画在几家上面）。
        let model = opening.step == Step::Model;
        if self.opening.is_none() {
            self.opening = Some(opening);
        }
        if model {
            self.want_catalog();
        }
        Outcome::Stay
    }

    /// 语言、操作方式、图标那几步（Miyu `pick_key`）：回车写这一项、往下走。
    fn opening_pick_key(&mut self, opening: &mut Opening, key: KeyEvent) -> Move {
        let step = opening.step;
        let pick = match step {
            Step::Language => &mut opening.language,
            Step::Keys => &mut opening.keys,
            _ => &mut opening.icons,
        };
        if pick.key(key) {
            return Move::Stay;
        }
        let at = pick.selected;
        match key.code {
            KeyCode::Esc => Move::Back,
            KeyCode::Enter if step == Step::Language => {
                self.use_language(opening, at);
                Move::Next
            }
            KeyCode::Enter if step == Step::Keys => {
                let mode = KEY_MODES[at.min(KEY_MODES.len() - 1)];
                self.set_quietly("tui.keys", Value::Text(mode.into()));
                Move::Next
            }
            KeyCode::Enter => {
                let set = ICON_SETS[at.min(ICON_SETS.len() - 1)];
                self.set_quietly("tui.icons", Value::Text(set.into()));
                Move::Next
            }
            _ => Move::Stay,
        }
    }

    /// 配色方案那一步（Miyu `oobe/theme.rs` 的 `key`）：上下挪，明暗、铺底色回车开选择窗（挪着就换样子，`Enter` 选好先记下，
    /// `Esc` 换回开窗以前的），「下一步」把选好的一起排给写线程，写完才往下走（`opening_after`）；在写的时候只认 `Esc`。
    fn theme_key(&mut self, opening: &mut Opening, key: KeyEvent) -> Move {
        if key.code == KeyCode::Esc {
            // 在写的照旧写完；回来时不再替人往下走。
            opening.theme_saving = false;
            return Move::Back;
        }
        if opening.theme_saving {
            return Move::Stay;
        }
        let at = THEME_SLOTS
            .iter()
            .position(|s| *s == opening.theme)
            .unwrap_or(0);
        let shown = |settings: &Settings, key: &str| {
            opening
                .picks
                .iter()
                .find(|(picked, _)| picked == key)
                .map(|(_, value)| value.clone())
                .or_else(|| settings.value(key).map(|(value, _)| value.clone()))
        };
        match nav::plain(key) {
            Some(Nav::Up) => opening.theme = THEME_SLOTS[at.saturating_sub(1)],
            Some(Nav::Down) => opening.theme = THEME_SLOTS[(at + 1).min(THEME_SLOTS.len() - 1)],
            None if key.code == KeyCode::Enter => match opening.theme {
                ThemeSlot::Next => return self.theme_save(opening),
                ThemeSlot::Mode => {
                    let words = &self.texts.opening.theme;
                    let now = match shown(self, "tui.theme_mode") {
                        Some(Value::Text(text)) => text.to_string(),
                        _ => String::new(),
                    };
                    let current = MODES.iter().position(|mode| now == *mode);
                    let choose = Choose::new(words.rows[0].clone(), words.modes.to_vec(), current);
                    self.popup = Some(Popup::new(
                        Window::Choose(choose),
                        Target::Pick("tui.theme_mode".into()),
                    ));
                }
                ThemeSlot::Ground => {
                    let words = &self.texts.opening.theme;
                    let on = !matches!(
                        shown(self, "tui.theme_background"),
                        Some(Value::Bool(false))
                    );
                    let choose = Choose::new(
                        words.rows[1].clone(),
                        words.ground.to_vec(),
                        Some(usize::from(!on)),
                    );
                    self.popup = Some(Popup::new(
                        Window::Choose(choose),
                        Target::Pick("tui.theme_background".into()),
                    ));
                }
            },
            _ => {}
        }
        Move::Stay
    }

    /// 配色方案那一步的选择窗里 `Enter`（`Target::Pick`）：记下选的、关窗，不写盘（Miyu `Pressed::Picked` 只改 `ThemeStep`）。
    pub(super) fn theme_picked(&mut self, key: &str) {
        let Some(Popup {
            window: Window::Choose(choose),
            ..
        }) = self.popup.take()
        else {
            return;
        };
        let value = match key {
            "tui.theme_mode" => MODES.get(choose.at).map(|mode| Value::Text((*mode).into())),
            _ => Some(Value::Bool(choose.at == 0)),
        };
        if let (Some(opening), Some(value)) = (&mut self.opening, value) {
            opening.picks.retain(|(picked, _)| picked != key);
            opening.picks.push((key.to_string(), value));
        }
    }

    /// 「下一步」（Miyu `ThemeStep::save`）：选好的几项一起排给写线程，界面不等盘；没选过的不写（配置文件照旧只写人选了的）。
    /// 什么都没选的直接往下走。
    fn theme_save(&mut self, opening: &mut Opening) -> Move {
        if opening.picks.is_empty() {
            return Move::Next;
        }
        let edits = opening
            .picks
            .iter()
            .filter(|(key, _)| self.schema.items.iter().any(|shown| shown.item.key == key))
            .map(|(key, value)| lm_engine::settings::Edit::Set(key.clone(), value.clone()))
            .collect();
        match self.save_edits(edits, None, super::writer::After::Quiet) {
            Ok(()) if self.writer.busy() => {
                opening.theme_saving = true;
                Move::Stay
            }
            Ok(()) => Move::Next,
            Err(why) => {
                let said = super::texts::fill(&self.texts.failed, &[("why", &why)]);
                self.status = Some((said, Tone::Bad));
                Move::Stay
            }
        }
    }

    /// 完成那一屏回车：删记号、进对话。
    fn opening_done(&mut self) -> Outcome {
        // 删不掉的（本来就没写成、目录只读）：有配置文件了，下次照样直接进对话。
        let mark = super::startup::opening_mark(self.service.paths());
        std::fs::remove_file(&mark).unwrap_or_default();
        self.status = None;
        self.opening = None;
        Outcome::Back
    }

    /// 窗、编辑窗关上以后、写完了以后看一眼：接模型那一步导好了一家的照它走（`model.rs` 的 `model_imported`），存完了的往下走。
    pub(super) fn opening_after(&mut self) {
        let Some(mut opening) = self.opening.take() else {
            return;
        };
        let closed = self.popup.is_none() && self.models.as_ref().is_none_or(|m| m.popup.is_none());
        // 开头一起来自动导的那一次：人还停在前面几步时就写完了，这时就照导进来的把接模型那一步备好。
        if (opening.step == Step::Model || opening.importing) && closed {
            let saving = matches!(opening.model.phase, Phase::Saving(_));
            if matches!(opening.model.phase, Phase::List | Phase::Ready(_)) {
                self.model_imported(&mut opening);
                opening.importing &= self.providers().is_empty();
            } else if saving && self.model_saved(&mut opening) {
                opening.step = Step::Theme;
            }
        }
        // 配色方案那一步「下一步」写完了：写成了的往下走，没写成的留在这一步、状态行照实说（同 Miyu 写不成留在这一步）。
        if opening.step == Step::Theme && opening.theme_saving && !self.writer.busy() {
            opening.theme_saving = false;
            if !matches!(self.status, Some((_, Tone::Bad))) {
                self.status = None;
                opening.step = Step::Done;
            }
        }
        // 配色那一步改了当场看得见，不用状态行再说「已保存」。
        if opening.step == Step::Theme && matches!(self.status, Some((_, Tone::Good))) {
            self.status = None;
        }
        self.opening = Some(opening);
    }

    /// 粘贴：给接模型那一步光标那一格。
    pub(super) fn opening_paste(&mut self, text: &str) {
        if let Some(opening) = &mut self.opening
            && opening.step == Step::Model
        {
            Settings::model_paste(opening, text);
        }
    }

    /// 开头里正在打字（接模型那一步的格子、搜模型）：`Ctrl` 那几个键归格子。
    pub(super) fn opening_writing(&self) -> bool {
        self.opening
            .as_ref()
            .is_some_and(|o| o.step == Step::Model && o.model.writing())
    }

    /// 完成那一屏的那一行（Miyu `summary_line`）：照 ` · ` 分的几段，值是空的那一段整段不写（LiteMiyu 没有人格、预设）。
    pub fn opening_summary(&self, opening: &Opening) -> String {
        let model = opening.model.chosen.clone().unwrap_or_default();
        let values = [
            ("{model}", model.as_str()),
            ("{persona}", ""),
            ("{preset}", ""),
        ];
        self.texts
            .opening
            .done
            .summary
            .split(" · ")
            .filter(|part| {
                !values
                    .iter()
                    .any(|(key, value)| part.contains(key) && value.is_empty())
            })
            .map(|part| {
                values.iter().fold(part.to_string(), |text, (key, value)| {
                    text.replace(key, value)
                })
            })
            .collect::<Vec<_>>()
            .join(" · ")
    }

    /// 开头的字。
    pub fn opening_texts(&self) -> &OpeningTexts {
        &self.texts.opening
    }

    /// 配置里现在的一项（选项）；不是字的是空的。
    pub fn text_value(&self, key: &str) -> String {
        match self.value(key) {
            Some((Value::Text(text), _)) => text.to_string(),
            _ => String::new(),
        }
    }

    /// 存一项，状态行不说（开头里改了当场看得见）。
    fn set_quietly(&mut self, key: &str, value: Value) {
        if !self.schema.items.iter().any(|shown| shown.item.key == key) {
            return;
        }
        let edit = lm_engine::settings::Edit::Set(key.to_string(), value);
        if let Err(why) = self.save_edits(vec![edit], None, super::writer::After::Quiet) {
            let said = super::texts::fill(&self.texts.failed, &[("why", &why)]);
            self.status = Some((said, Tone::Bad));
        }
    }
}

#[cfg(test)]
mod tests;
