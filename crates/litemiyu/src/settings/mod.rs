//! 配置界面（施工 L8，`docs/00-设计.md` 第五节第 13 条）：`litemiyu config`、对话里 `/config`、第一次运行没配置时进的同一个
//! 东西。照 Miyu 终端的配置页（蓝图 `tui.md`「配置页」）：主菜单、照清单画的几页、盖在上面的选择窗和编辑窗。这里只有状态
//! 和按键，画在 `ui/settings/`；读写配置、存密钥都经引擎的配置服务（`lm_engine::settings`）：读在界面线程，写（要同步、会等盘）
//! 排给写线程（`writer.rs`），界面先照在内存里算好的样子画（[`Service::preview`]），写完照读回的换上。
//!
//! 用到才初始化：这个模块、清单的字表、目录只在进配置界面时读，平常起来直接进对话的不碰（设计第七节）。

mod catalog_fetch;
mod extras;
mod import;
pub(crate) mod lists;
pub mod models;
mod mouse;
pub mod opening;
mod page;
mod popup;
pub mod prefs;
mod presets;
mod preview;
pub mod probe;
mod providers;
pub mod startup;
pub mod texts;
pub mod writer;

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use lm_engine::providers::miyu;
use lm_engine::settings::catalog::Catalog;
use lm_engine::settings::schema::{Schema, schema};
use lm_engine::settings::{Edit, Settings as Service, Snapshot};
use lm_engine::store::paths::{Env, Paths};
use lm_kernel::config::{Control, Kind, Value};

pub use extras::{EXTRAS, Extra};
pub use mouse::{Hit, Hits};
pub use page::Row;
pub use popup::{Choose, Popup, Target, Window};
pub use presets::{FormRow, PresetTarget, Presets};
use texts::{Texts, fill};

/// 按了一个键以后界面要做的。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// 接着开着。
    Stay,
    /// 回到对话。
    Back,
    /// 退出程序（照 `litemiyu config` 起来的）。
    Quit,
}

/// 状态行那一句的颜色。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    /// 平常的一句（黄）。
    Note,
    /// 成了。
    Good,
    /// 出错。
    Bad,
    /// 正在办（试一家通不通）。
    Busy,
}

/// 在看哪儿。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    /// 主菜单，光标在第几页。
    Menu(usize),
    /// 一页：主菜单里的第几页、光标在第几行。
    Page {
        /// 主菜单里的第几页。
        page: usize,
        /// 光标在第几行（[`Settings::rows`]）。
        at: usize,
    },
}

/// 开着的配置界面。
pub struct Settings {
    /// 读写配置的一层（写线程也拿着一份）。
    service: std::sync::Arc<Service>,
    /// 写盘的线程（`writer.rs`）：存配置、存 key、导入都排给它，界面不等盘。
    pub writer: writer::Writer,
    /// 排着的里面有一件没写成：原因，等排的都写完了照它说（不让后面写成的「已保存」盖掉）。
    write_failed: Option<String>,
    /// 最后一份照盘上读到的配置：排的写没成、又读不回的时候退回它。
    confirmed: Snapshot,
    /// 画哪几页、哪几项，字照界面语言。
    pub schema: Schema,
    /// 读到的配置：每次存完重读。
    pub snapshot: Snapshot,
    /// 界面上的字。
    pub texts: Texts,
    /// 在看哪儿。
    pub view: View,
    /// 「供应商和模型」那一页（进页时开、回主菜单放掉）；开头里借它的编辑窗、选模型的窗。
    pub models: Option<models::Models>,
    /// 盖在最上面的窗。
    pub popup: Option<Popup>,
    /// 界面语言：常用几家的名字照它挑。
    language: String,
    /// 状态行。
    pub status: Option<(String, Tone)>,
    /// 主菜单按 `Esc`：回对话，还是退出程序。
    leave: Outcome,
    /// 存过：回对话时引擎要换上新的配置。
    pub changed: bool,
    /// 提示词那一份文件（配置目录的 `prompt.md`）。
    pub prompt: std::path::PathBuf,
    /// 要用外部编辑器开的文件（「提示词」那一行按了 `Enter`）：界面拿走去开（`app/settings.rs`）。
    pub editing: Option<std::path::PathBuf>,
    /// Miyu 的数据根（在的时候供应商页多一行「从 Miyu 导入」）。
    pub miyu: Option<std::path::PathBuf>,
    /// 从 Miyu 读到的、勾选窗开着时的那一份。
    importing: Option<miyu::Found>,
    /// 正在试的一家。
    pub probing: Option<probe::Running>,
    /// 存下来的、这一趟列到的供应商的模型列表，照编号（`lists.rs`）：打开时读一次。
    pub lists: std::collections::BTreeMap<String, Vec<String>>,
    /// 在后台拉列表的那一批（`lists.rs`），和排在它后面的几批。
    fetching: Option<lists::Fetching>,
    fetching_more: Vec<lists::Fetching>,
    /// 导入、存一家以后在后台拉它的模型列表（`lists.rs`）。照真的数据起的（[`Settings::open`]）开着；测试照 [`Settings::at`] 造的
    /// 关着（存的是目录里真的地址，不能去连），要测的自己打开。
    pub auto_lists: bool,
    /// 每一家最近一次在后台拉列表的时刻（`lists.rs`，Miyu `claim_list`）：对话那一头开 `/model` 也记在同一份里（`App::list_claims`）。
    pub claims: lm_engine::providers::lists::Claims,
    /// 在后台取模型目录的那一趟（`catalog_fetch.rs`，LM-9）。
    catalog_fetch: Option<catalog_fetch::Fetching>,
    /// 上一次没取到目录的时刻：半分钟里不再取。
    catalog_failed: Option<std::time::Instant>,
    /// 取目录这件事最后说的那一句（开头里画在接模型那一步）。
    catalog_said: Option<(String, Tone)>,
    /// 要用到目录时去取（LM-9，`catalog_fetch.rs`）。照真的数据起的（[`Settings::open`]）开着；测试照 [`Settings::at`] 造的关着
    /// （不许去连真的 models.dev），要测的自己打开、把地址指到本机。
    pub auto_catalog: bool,
    /// 试一家最多等多久。
    pub wait: std::time::Duration,
    /// 技能页、MCP 页进页时读的那一份。
    pub extra_list: extras::Listed,
    /// 「预设」页（`presets.rs`，LM-29）：进页时开，回主菜单放掉。
    pub presets: Option<Presets>,
    /// 第一次运行的开头（`opening.rs`）：开着时画它、按键归它。
    pub opening: Option<opening::Opening>,
    /// 这一下存过：界面照读回的值把终端自己的几项换上（`prefs.rs`），换完放下。
    pub saved: bool,
    /// 正在预览的几项和那一档（`preview.rs`）：「明暗」的挑选窗开着时光标停的那一档、开头配色方案那一步选好还没存的，界面照它换
    /// 样子；都没有的是空的。
    pub looked: Vec<(String, Value)>,
    /// 别的页状态行没话时写的那一句（Miyu 照「供应商和模型」那一页的选中算：「已获取 N 个模型」这类）：离开那一页时记下，
    /// 还没进过的进别的页时算一次（[`Settings::note_models`]）。
    pub models_note: Option<String>,
    /// 开头里换过界面语言：写进 `ui.language` 的值（回对话时对话界面照它换字，`app/settings.rs`）。
    pub switched: Option<String>,
    /// 一页里从第几行露起：哪一页、第几行（Miyu `page_top`，画的时候照上一帧接着算）。
    pub page_top: std::cell::Cell<(usize, usize)>,
    /// 配置里有 MCP 服务（写错了的也算）：主菜单照它列不列 MCP（Miyu `entries`：有服务才列）。读配置、换了配置的样子时算
    /// （[`extras::any_server`]），`layout` 每一帧调，不读盘也不每次解析。
    mcp: bool,
    /// 这一帧点得到的地方（画的时候记，`mouse.rs`）。
    pub hits: Hits,
    /// 上一下点在哪、什么时候（认双击）。
    last_click: Option<(ratatui::layout::Position, std::time::Instant)>,
}

impl Settings {
    /// 打开：数据照 `LITEMIYU_HOME`、XDG 的环境变量，字照 `language`。主菜单按 `Esc` 照 `leave`。
    ///
    /// # Errors
    ///
    /// 找不到家目录、配置读不了、字表读不进来：一句给人看的原因。
    pub fn open(language: &str, leave: Outcome) -> Result<Settings, String> {
        let paths = Paths::locate(&Env::current()).map_err(|error| error.to_string())?;
        let mut settings = Settings::at(paths, language, leave)?;
        settings.miyu = miyu::locate(&|name| std::env::var(name).ok())
            .ok()
            .filter(|root| root.is_dir());
        settings.auto_lists = true;
        settings.auto_catalog = true;
        Ok(settings)
    }

    /// 同 [`Settings::open`]，数据在 `paths` 那一处。
    ///
    /// # Errors
    ///
    /// 同 [`Settings::open`]。
    pub fn at(paths: Paths, language: &str, leave: Outcome) -> Result<Settings, String> {
        let prompt = paths.config.join("prompt.md");
        let paths_for_lists = paths.clone();
        let service = std::sync::Arc::new(Service::new(paths, |name| std::env::var(name).ok()));
        let writer = writer::Writer::new(std::sync::Arc::clone(&service));
        let schema = schema(service.items(), language).map_err(|error| error.to_string())?;
        let snapshot = service.read().map_err(|error| error.to_string())?;
        let texts = Texts::load(language)?;
        let mcp = extras::any_server(&snapshot.text);
        let mut settings = Settings {
            mcp,
            service,
            writer,
            write_failed: None,
            confirmed: snapshot.clone(),
            schema,
            snapshot,
            texts,
            view: View::Menu(0),
            models: None,
            popup: None,
            language: language.to_string(),
            status: None,
            leave,
            changed: false,
            prompt,
            editing: None,
            miyu: None,
            importing: None,
            probing: None,
            lists: lm_engine::providers::lists::stored(&paths_for_lists),
            fetching: None,
            fetching_more: Vec::new(),
            auto_lists: false,
            claims: lm_engine::providers::lists::Claims::default(),
            catalog_fetch: None,
            catalog_failed: None,
            catalog_said: None,
            auto_catalog: false,
            wait: probe::WAIT,
            extra_list: extras::Listed::default(),
            presets: None,
            opening: None,
            saved: false,
            looked: Vec::new(),
            models_note: None,
            switched: None,
            page_top: std::cell::Cell::new((usize::MAX, 0)),
            hits: Hits::default(),
            last_click: None,
        };
        settings.warn_broken();
        Ok(settings)
    }

    /// 「提示词」：还没有这份文件的先写进出厂的那一段，交给界面用外部编辑器开（设计第五节第 15 条）；写不进的状态行说为什么。
    fn edit_prompt(&mut self) {
        let written = match std::fs::metadata(&self.prompt) {
            Ok(_) => Ok(()),
            Err(_) => self
                .prompt
                .parent()
                .map_or(Ok(()), std::fs::create_dir_all)
                .and_then(|()| std::fs::write(&self.prompt, lm_kernel::policy::shipped::PROMPT)),
        };
        match written {
            Ok(()) => self.editing = Some(self.prompt.clone()),
            Err(error) => {
                let why = self.texts.failed.replace("{why}", &error.to_string());
                self.status = Some((why, Tone::Bad));
            }
        }
    }

    /// 编辑器退出了：状态行说一句新开的会话照它。
    pub fn prompt_edited(&mut self) {
        self.status = Some((self.texts.prompt.saved.clone(), Tone::Good));
    }

    /// 提示词那一份文件现在是什么样的：没有、空的、自己写的。
    pub fn prompt_state(&self) -> &str {
        match std::fs::read_to_string(&self.prompt) {
            Err(_) => &self.texts.prompt.shipped,
            Ok(text) if text.trim().is_empty() => &self.texts.prompt.blank,
            Ok(_) => &self.texts.prompt.custom,
        }
    }

    /// 主菜单里的几页：「供应商和模型」一直在，别的有东西画的才列，照清单的先后。交回页在 `schema.pages` 里的下标。
    pub fn menu(&self) -> Vec<usize> {
        (0..self.schema.pages.len())
            .filter(|&p| {
                let id = self.schema.pages[p].id;
                id == MODELS || !page::rows(&self.schema, id).is_empty()
            })
            .collect()
    }

    /// 主菜单的先后（Miyu：通用、外观、供应商和模型、……、MCP、高级）：照清单画的几页，技能页、MCP 页插在「高级」前面
    /// （MCP 在前；技能页是 LiteMiyu 多的，挨着 MCP）。配置里没有 MCP 服务的不列 MCP（Miyu `pages/mod.rs` 的 `entries`：
    /// `servers()` 是空的不列）；加服务在配置界面外头（手改配置，同 Miyu 装包、手改；LM-3 删了
    /// 命令行的 `mcp add`），加了再打开就列。
    pub fn layout(&self) -> Vec<Entry> {
        let mut layout: Vec<Entry> = self.menu().into_iter().map(Entry::Page).collect();
        let advanced = layout
            .iter()
            .position(
                |entry| matches!(entry, Entry::Page(p) if self.schema.pages[*p].id == ADVANCED),
            )
            .unwrap_or(layout.len());
        layout.splice(
            advanced..advanced,
            EXTRAS
                .iter()
                .filter(|&&extra| extra != Extra::Mcp || self.mcp)
                .map(|&extra| Entry::Extra(extra)),
        );
        layout
    }

    /// 主菜单第 `entry` 项是照清单画的哪一页（在 `schema.pages` 里的下标）。
    pub fn page_of(&self, entry: usize) -> Option<usize> {
        match self.layout().get(entry) {
            Some(Entry::Page(p)) => Some(*p),
            _ => None,
        }
    }

    /// 主菜单第 `entry` 页是不是「供应商和模型」。
    pub fn is_models(&self, entry: usize) -> bool {
        self.page_of(entry)
            .is_some_and(|p| self.schema.pages[p].id == MODELS)
    }

    /// 打开时直接停在「供应商和模型」（测试里直接进这一页）。
    #[cfg(test)]
    pub fn show_models(&mut self) {
        if let Some(entry) = (0..self.entries()).find(|&e| self.is_models(e)) {
            self.view = View::Page { page: entry, at: 0 };
            self.enter_models();
        }
    }

    /// 主菜单第 `entry` 页的几行。
    pub fn rows(&self, entry: usize) -> Vec<Row> {
        self.page_of(entry)
            .map(|p| page::rows(&self.schema, self.schema.pages[p].id))
            .unwrap_or_default()
    }

    /// 一项现在的最终值和来自哪一层（`default`、`system`、`env`）。
    pub fn value(&self, key: &str) -> Option<(&Value, &'static str)> {
        self.snapshot
            .resolved
            .get(key)
            .map(|(value, origin)| (value, origin.layer_name()))
    }

    /// 第 `item` 项现在的值写给人看。「默认预设」写预设给人看的名字（自带的两个照界面语言；写的名字现在没有的原样）。
    pub fn shown_value(&self, item: usize) -> String {
        let shown = &self.schema.items[item];
        let value = self.value(shown.item.key).map(|(value, _)| value);
        match (shown.item.key, value) {
            (lm_engine::presets::DEFAULT, Some(Value::Text(name))) => self.preset_shown(name),
            _ => self.shown_of(item, value),
        }
    }

    /// 第 `item` 项是 `value` 时写给人看的样子。
    pub fn shown_of(&self, item: usize, value: Option<&Value>) -> String {
        page::shown_value(&self.schema.items[item], value, &self.texts)
    }

    /// 收一个终端事件。
    pub fn handle(&mut self, event: &Event) -> Outcome {
        let outcome = self.handle_event(event);
        if outcome == Outcome::Stay {
            self.opening_after();
        }
        outcome
    }

    fn handle_event(&mut self, event: &Event) -> Outcome {
        match event {
            Event::Key(key) if key.kind != KeyEventKind::Release => self.key(*key),
            Event::Mouse(mouse) => self.mouse(mouse),
            Event::Paste(text)
                if self.opening.is_some()
                    && self.popup.is_none()
                    && self.models.as_ref().is_none_or(|m| m.popup.is_none()) =>
            {
                self.opening_paste(text);
                Outcome::Stay
            }
            Event::Paste(text) if self.models.is_some() && self.popup.is_none() => {
                self.models_paste(text);
                Outcome::Stay
            }
            Event::Paste(text) => {
                if let Some(Popup {
                    window: Window::Edit(edit),
                    ..
                }) = &mut self.popup
                {
                    edit.paste(text);
                }
                Outcome::Stay
            }
            _ => Outcome::Stay,
        }
    }

    /// 按一个键：窗开着的归窗，不然照在看哪儿。
    pub fn key(&mut self, key: KeyEvent) -> Outcome {
        // 同 Miyu（`app/settings.rs`、`settings/input.rs`）：`Ctrl+C`、`Ctrl+D` 照 `litemiyu config` 起来的退出、对话里打开的回
        // 对话（开头里的退出）；不在打字的窗里 `Ctrl+J`、`Ctrl+K` 同 `↓`、`↑`，按键模式是什么都一样；别的带 Ctrl 的不管（打字的
        // 窗自己认 `Ctrl+U`）。
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('c' | 'd') if self.opening.is_some() => return Outcome::Quit,
                KeyCode::Char('c' | 'd') => return self.leave,
                _ if self.writing() => {}
                KeyCode::Char('j' | 'J') => {
                    return self.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
                }
                KeyCode::Char('k' | 'K') => {
                    return self.key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
                }
                _ => return Outcome::Stay,
            }
        }
        if key.code == KeyCode::Esc && self.stop_probe() {
            return Outcome::Stay;
        }
        if self.popup.is_some() {
            self.popup_key(key);
            // 从对话里的 `/preset` 框来新建、填名字的窗取消了：直接回对话（`presets.rs`）。
            return match self.presets_leaving() {
                true => Outcome::Back,
                false => Outcome::Stay,
            };
        }
        if self.models.as_ref().is_some_and(|m| m.popup.is_some()) {
            self.models_key(self.models_page(), key);
            return Outcome::Stay;
        }
        if self.opening.is_some() {
            return self.opening_key(key);
        }
        match self.view {
            View::Menu(at) => self.menu_key(at, key),
            View::Page { page, at } if self.extra(page) == Some(Extra::Presets) => {
                self.presets_key(page, at, key)
            }
            View::Page { page, at } if self.extra(page).is_some() => {
                if let Some(extra) = self.extra(page) {
                    self.extra_key(page, at, extra, key);
                }
                Outcome::Stay
            }
            View::Page { page, .. } if self.is_models(page) => {
                self.models_key(page, key);
                Outcome::Stay
            }
            View::Page { page, at } => {
                self.page_key(page, at, key);
                Outcome::Stay
            }
        }
    }

    /// 主菜单：上下挪、`Enter` 进去、`Esc` `q` 照打开的方式回对话或退出。
    fn menu_key(&mut self, at: usize, key: KeyEvent) -> Outcome {
        let count = self.entries();
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') => return self.leave,
            KeyCode::Down | KeyCode::Char('j') => {
                self.view = View::Menu((at + 1).min(count.saturating_sub(1)));
            }
            KeyCode::Up | KeyCode::Char('k') => self.view = View::Menu(at.saturating_sub(1)),
            KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') if at < count => {
                if self.is_models(at) {
                    self.enter_models();
                    self.view = View::Page { page: at, at: 0 };
                    self.status = None;
                    return Outcome::Stay;
                }
                self.note_models();
                if let Some(extra) = self.extra(at) {
                    match extra {
                        Extra::Presets => self.enter_presets(),
                        _ => self.enter_extra(extra),
                    }
                    self.view = View::Page { page: at, at: 0 };
                    self.status = None;
                    return Outcome::Stay;
                }
                let rows = self.rows(at);
                let first = rows
                    .iter()
                    .position(|row| page::selectable(*row))
                    .unwrap_or(0);
                self.view = View::Page {
                    page: at,
                    at: first,
                };
                self.status = None;
            }
            _ => {}
        }
        Outcome::Stay
    }

    /// 一页：上下挪（跳过组名）、`Enter` 照控件改、`d` 恢复默认、`Esc` `q` `←` 回主菜单。
    fn page_key(&mut self, page: usize, at: usize, key: KeyEvent) {
        // 按一个键先清掉上一句，正在办的留着（Miyu `pages/keys.rs` 的 `section_key`）。
        self.status = self.status.take().filter(|(_, tone)| *tone == Tone::Busy);
        let rows = self.rows(page);
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Left | KeyCode::Char('h') => {
                self.view = View::Menu(page);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let next = (at + 1..rows.len()).find(|&i| page::selectable(rows[i]));
                self.view = View::Page {
                    page,
                    at: next.unwrap_or(at),
                };
            }
            KeyCode::Up | KeyCode::Char('k') => {
                let before = (0..at).rev().find(|&i| page::selectable(rows[i]));
                self.view = View::Page {
                    page,
                    at: before.unwrap_or(at),
                };
            }
            KeyCode::Enter => match rows.get(at) {
                Some(Row::Item(item)) => self.change(*item),
                Some(Row::Prompt) => self.edit_prompt(),
                _ => {}
            },
            // 恢复默认先问一句（Miyu「恢复默认：名字？ / 移除此项的设置，恢复为默认的值」）。
            KeyCode::Char('d') => {
                if let Some(Row::Item(item)) = rows.get(at) {
                    self.ask_unset(*item);
                }
            }
            _ => {}
        }
    }

    /// `d` 恢复默认：配置里没写这一项的说一声；写了的先问（Miyu「恢复默认：名字？ / 移除此项的设置，恢复为默认的值」，
    /// 按钮「恢复」「取消」，停在「取消」）。
    fn ask_unset(&mut self, item: usize) {
        let key = self.schema.items[item].item.key;
        if !self.snapshot.written.contains_key(key) {
            self.status = Some((self.texts.not_set.clone(), Tone::Note));
            return;
        }
        let [ask, said, yes, no] = &self.texts.unset_ask;
        let name = &self.schema.items[item].words.name;
        let from = self
            .texts
            .origins
            .get("default")
            .cloned()
            .unwrap_or_default();
        let confirm = popup::Confirm {
            title: fill(ask, &[("name", name)]),
            lines: vec![fill(said, &[("from", &from)])],
            buttons: vec![(yes.clone(), true), (no.clone(), false)],
            at: 1,
        };
        self.popup = Some(Popup::new(Window::Confirm(confirm), Target::Unset(item)));
    }

    /// 改第 `item` 项：开关当场换、存；下拉开选择窗；别的开编辑窗。
    fn change(&mut self, item: usize) {
        if self.warn_broken() {
            return;
        }
        let shown = &self.schema.items[item];
        let key = shown.item.key.to_string();
        let value = self.value(&key).map(|(value, _)| value.clone());
        match (shown.item.ui.control, shown.item.kind) {
            // 「默认预设」：照现在有的预设列（`presets.rs`）。
            _ if key == lm_engine::presets::DEFAULT => {
                let title = shown.words.name.clone();
                self.choose_default_preset(title);
            }
            (Control::Toggle, _) => {
                let on = matches!(value, Some(Value::Bool(true)));
                self.save(&[Edit::Set(key, Value::Bool(!on))], item);
            }
            (_, Kind::Option(options)) => {
                let names = options
                    .iter()
                    .map(|option| {
                        shown
                            .words
                            .options
                            .get(*option)
                            .cloned()
                            .unwrap_or_else(|| (*option).to_string())
                    })
                    .collect();
                let current = options
                    .iter()
                    .position(|option| matches!(&value, Some(Value::Text(text)) if text == option));
                let choose = Choose::new(shown.words.name.clone(), names, current);
                self.popup = Some(Popup::new(Window::Choose(choose), Target::Item(key)));
            }
            _ => {
                let text = page::editable(value.as_ref());
                let edit = popup::Edit::new(shown.words.name.clone(), text, false);
                self.popup = Some(Popup::new(Window::Edit(edit), Target::Item(key)));
            }
        }
    }

    /// 窗里按一个键：选好、填好了照写到哪交出去。
    fn popup_key(&mut self, key: KeyEvent) {
        let Some(popup) = &mut self.popup else {
            return;
        };
        match popup.window.key(key) {
            popup::Done::Open => {}
            popup::Done::Cancel => {
                let target = popup.target.clone();
                self.popup = None;
                self.importing = None;
                self.preset_cancelled(&target);
            }
            popup::Done::Accept => {
                let target = popup.target.clone();
                self.accept(target);
            }
        }
    }

    /// 窗里选好、填好了：照写到哪办。
    fn accept(&mut self, target: Target) {
        let chosen = match self.popup.as_ref().map(|popup| &popup.window) {
            Some(Window::Choose(choose)) => Some(choose.at),
            Some(Window::Confirm(confirm)) => Some(confirm.at),
            _ => None,
        };
        match target {
            Target::Item(key) => self.accept_item(&key),
            Target::Pick(key) => self.theme_picked(&key),
            Target::Import => self.import_ticked(),
            Target::ImportClash(ids) => self.import_clash(&ids, chosen),
            Target::PresetNew(base) => self.preset_new(base.as_deref()),
            Target::PresetRename(old) => self.preset_rename(&old),
            Target::PresetDelete(name) => self.preset_delete(&name, chosen),
            Target::Unset(item) => {
                self.popup = None;
                let key = self.schema.items[item].item.key.to_string();
                if chosen == Some(0) {
                    self.save(&[Edit::Unset(key)], item);
                }
            }
        }
    }

    /// 照清单画的一页里的一项选好、填好了：照这一项的类型读，存。填的不收的窗留着、状态行说一声。
    fn accept_item(&mut self, key: &str) {
        let Some(item) = self
            .schema
            .items
            .iter()
            .position(|shown| shown.item.key == key)
        else {
            self.popup = None;
            return;
        };
        let shown = &self.schema.items[item];
        let edit = match (
            self.popup.as_ref().map(|popup| &popup.window),
            shown.item.kind,
        ) {
            // 「默认预设」的选择窗（`presets.rs`）：选的是第几个预设。
            (Some(Window::Choose(choose)), _) if key == lm_engine::presets::DEFAULT => {
                match self.default_preset_at(choose.at) {
                    Some(name) => Edit::Set(key.to_string(), Value::Text(name.into())),
                    None => return,
                }
            }
            (Some(Window::Choose(choose)), Kind::Option(options)) => {
                Edit::Set(key.to_string(), Value::Text(options[choose.at].into()))
            }
            // 开头「铺底色」的「开、关」（`opening.rs`，同 Miyu 引导）。
            (Some(Window::Choose(choose)), Kind::Bool) => {
                Edit::Set(key.to_string(), Value::Bool(choose.at == 0))
            }
            (Some(Window::Edit(edit)), _) => match page::read_input(shown, &edit.text) {
                Ok(Some(value)) => Edit::Set(key.to_string(), value),
                Ok(None) => Edit::Unset(key.to_string()),
                // 读得成、不合这一项（超范围、写法不对）的：同 Miyu 交给核心、核心不收——窗关上，状态行说没存成（Miyu
                // `popup_keys.rs` → `write_setting`）。
                Err(())
                    if lm_kernel::config::edit::input(shown.item.kind, edit.text.trim())
                        .is_some() =>
                {
                    let why = fill(&self.texts.bad_value, &[("name", &shown.words.name)]);
                    self.popup = None;
                    let said = fill(&self.texts.failed, &[("why", &why)]);
                    self.status = Some((said, Tone::Bad));
                    return;
                }
                // 读不成数的：红字写在窗里（Miyu「请输入数字」），窗留着。
                Err(()) => {
                    let said = match shown.item.kind {
                        Kind::Int { .. } | Kind::Float { .. } => {
                            self.texts.models.error("bad_number")
                        }
                        _ => fill(&self.texts.bad_value, &[("name", &shown.words.name)]),
                    };
                    if let Some(Popup {
                        window: Window::Edit(edit),
                        ..
                    }) = &mut self.popup
                    {
                        edit.error = Some(said);
                    }
                    return;
                }
            },
            _ => return,
        };
        self.popup = None;
        self.save(&[edit], item);
    }

    /// 存几项：状态行先说「正在保存…」，写完照第 `item` 项什么时候生效说（没存成的说原因、值退回）。交回收没收下（不收的
    /// ——键不认得、值不对、文件读不懂——当场说原因）。
    pub(crate) fn save(&mut self, edits: &[Edit], item: usize) -> bool {
        let applies = self.schema.items[item].item.applies.as_str();
        let said = self.texts.applies.get(applies).cloned().unwrap_or_default();
        match self.save_edits(edits.to_vec(), None, writer::After::Say(said)) {
            Ok(()) => true,
            Err(why) => {
                self.status = Some((fill(&self.texts.failed, &[("why", &why)]), Tone::Bad));
                false
            }
        }
    }

    /// 改几项（毛病二，配置 四）：先在内存里算出改完的样子换上（界面当场照它画，下一项照它接着改），写盘排给写线程（先排的先写），
    /// 写完照读回的换上（[`Settings::poll_writes`]）。`secret` 是这一下要新存的 key：编号（密钥的名字照它起）、预计存成的名字、
    /// 明文；写线程先存它，存成的名字和预计的不一样（这一瞬间别人改了密钥文件）时配置里照存成的写。不收的交回原因（原话）。
    pub(crate) fn save_edits(
        &mut self,
        mut edits: Vec<Edit>,
        secret: Option<(String, String, lm_kernel::config::secret::Secret)>,
        after: writer::After,
    ) -> Result<(), String> {
        let names: Vec<String> = secret.iter().map(|(_, name, _)| name.clone()).collect();
        let preview = self
            .service
            .preview(&self.snapshot, &edits, &names)
            .map_err(|error| error.to_string())?;
        if !self.writer.busy() {
            self.confirmed = self.snapshot.clone();
        }
        if preview.written == self.snapshot.written && secret.is_none() {
            // 和现在的一样：不用写。
            if let writer::After::Say(said) | writer::After::Lists(said, _) = &after {
                self.status = Some((said.clone(), Tone::Good));
            }
            return Ok(());
        }
        self.snapshot = preview;
        self.mcp = extras::any_server(&self.snapshot.text);
        self.saved = true;
        self.changed = true;
        self.status = Some((self.texts.models.status("saving"), Tone::Busy));
        self.writer.push(
            Box::new(move |service: &Service| {
                if let Some((wanted, predicted, secret)) = secret {
                    let name = service.store_secret(&wanted, &secret)?;
                    if name != predicted {
                        for edit in &mut edits {
                            if let Edit::Set(
                                _,
                                Value::Secret(lm_kernel::config::secret::Reference::Secret(n)),
                            ) = edit
                                && *n == predicted
                            {
                                n.clone_from(&name);
                            }
                        }
                    }
                }
                service.set(&edits)
            }),
            after,
        );
        Ok(())
    }

    /// 排一件不先算样子的写（从 Miyu 导入：引擎成套写）：状态行说「正在保存…」，写完照读回的换上、照 `after` 说。
    pub(crate) fn save_job(&mut self, job: writer::Job, after: writer::After) {
        self.status = Some((self.texts.models.status("saving"), Tone::Busy));
        self.writer.push(job, after);
    }

    /// 主循环醒来时收一下写完了的：写成的照 `after` 说；排的都写完了照读回的配置换上（没存成的那几项就退回去了）。交回收到了没有。
    pub fn poll_writes(&mut self) -> bool {
        let mut any = false;
        while let Some(done) = self.writer.try_done() {
            any = true;
            let idle = !self.writer.busy();
            if let Err(why) = &done.result {
                let said = fill(&self.texts.failed, &[("why", why)]);
                self.status = Some((said.clone(), Tone::Bad));
                self.write_failed = Some(said);
            }
            let read = done.snapshot.clone();
            if idle {
                // 排的都写完了：照读回的换上；读不回的，有没写成的退回最后一份读到的，都写成了的留着算好的那一份。
                let back = match (done.snapshot, self.write_failed.is_some()) {
                    (Some(snapshot), _) => Some(snapshot),
                    (None, true) => Some(self.confirmed.clone()),
                    (None, false) => None,
                };
                if let Some(snapshot) = back {
                    self.confirmed = snapshot.clone();
                    self.snapshot = snapshot;
                    self.mcp = extras::any_server(&self.snapshot.text);
                    self.saved = true;
                    self.refresh_models();
                }
            }
            if let Ok(changed) = done.result {
                self.changed |= !changed.is_empty();
                match done.after {
                    writer::After::Say(said) => self.status = Some((said, Tone::Good)),
                    writer::After::Quiet => {
                        if matches!(self.status, Some((_, Tone::Busy))) {
                            self.status = None;
                        }
                    }
                    writer::After::Lists(said, ids) => {
                        // 写完读回的那一份里才有导进来的几家（导入不先算样子）。
                        let snapshot = read.unwrap_or_else(|| self.snapshot.clone());
                        self.fetch_lists_with(&snapshot, &ids);
                        // 开头里存了一家（`opening/model.rs`）没话要说：只留「正在获取…」那一句。
                        let said = match &self.status {
                            Some((listing, Tone::Busy))
                                if self.fetching_lists() && said.is_empty() =>
                            {
                                Some((listing.clone(), Tone::Busy))
                            }
                            Some((listing, Tone::Busy)) if self.fetching_lists() => {
                                Some((format!("{said} · {listing}"), Tone::Good))
                            }
                            _ if said.is_empty() => None,
                            _ => Some((said, Tone::Good)),
                        };
                        if let Some(said) = said {
                            self.status = Some(said);
                        }
                    }
                }
            }
            if idle && let Some(failed) = self.write_failed.take() {
                self.status = Some((failed, Tone::Bad));
            }
        }
        if any {
            self.opening_after();
        }
        any
    }

    /// 测试里等排着的都写完（最多二十秒），收完。
    #[cfg(test)]
    pub fn flush_writes(&mut self) {
        let until = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while self.writer.busy() {
            assert!(std::time::Instant::now() < until, "二十秒没写完");
            self.writer.wait(until);
            self.poll_writes();
        }
    }

    /// 关掉配置界面，交出写线程（还没写完的接着写）。
    pub fn into_writer(self) -> writer::Writer {
        let Settings { writer, .. } = *Box::new(self);
        writer
    }

    /// 「供应商和模型」在主菜单里是第几页（开头里借它的窗时，按键照这一页认）。
    fn models_page(&self) -> usize {
        match self.view {
            View::Page { page, .. } => page,
            View::Menu(at) => at,
        }
    }

    /// 进别的页时还没记过「供应商和模型」那一句的，照刚打开那一页的样子（停在供应商分页的第一家）算一次：读一次目录，算完
    /// 就放掉（同 Miyu：它进配置页就读 `model.list`）。读不了目录的照配置里写了的算，不说原因（原因进「供应商和模型」时说）。
    fn note_models(&mut self) {
        if self.models_note.is_some() {
            return;
        }
        let catalog = Catalog::load(self.service.paths()).ok();
        let models = models::Models::open(&self.snapshot, catalog, self.lists.clone());
        self.models_note = Some(models.quiet(&self.texts.models));
    }

    /// 正在打字：编辑窗开着，或者「供应商和模型」的编辑窗里正在改一格（Miyu `settings/input.rs` 的 `writing`）。
    fn writing(&self) -> bool {
        let editing = matches!(
            self.popup,
            Some(Popup {
                window: Window::Edit(_),
                ..
            })
        );
        editing
            || self.opening_writing()
            || matches!(
                self.models.as_ref().and_then(|m| m.popup.as_ref()),
                Some(models::Popup::Form(form)) if form.editing.is_some()
            )
    }

    /// 配置文件读不懂：状态行说清是哪一行，交回真（这时只能看、不能存）。
    pub(crate) fn warn_broken(&mut self) -> bool {
        let Some((line, column)) = self.snapshot.broken else {
            return false;
        };
        let said = fill(
            &self.texts.broken,
            &[("line", &line.to_string()), ("column", &column.to_string())],
        );
        self.status = Some((said, Tone::Bad));
        true
    }
}

/// 「供应商和模型」那一页的编号。
const MODELS: &str = "models";

/// 「高级」那一页的编号：技能页、MCP 页插在它前面。
const ADVANCED: &str = "advanced";

/// 主菜单的一项。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Entry {
    /// 照清单画的一页：在 `schema.pages` 里的下标。
    Page(usize),
    /// 技能页、MCP 页。
    Extra(Extra),
}

#[cfg(test)]
mod tests;

/// 测试用：缓存目录里放一份取到过的模型目录（真目录裁出来的那一份，`lm-kernel/testdata`）。LM-9 起程序不带目录，要照目录列、
/// 认供应商的测试先放它。
#[cfg(test)]
pub(crate) fn put_catalog(paths: &Paths) {
    let cache = paths.model_cache();
    std::fs::create_dir_all(&cache).unwrap();
    std::fs::write(
        cache.join(lm_engine::models::catalog::FILE),
        include_str!("../../../lm-kernel/testdata/models/models-dev-trimmed.json"),
    )
    .unwrap();
}
