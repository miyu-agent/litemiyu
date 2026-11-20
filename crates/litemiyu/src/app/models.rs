//! `/model`：换这个会话的模型（Miyu 终端 `app/models.rs`，蓝图 `tui.md`「配置与模型」第 1 条）。开框时向引擎要一次一行行
//! （`Command::Choices`，池、配好的几家的模型）；打字筛（Vim 按键模式下先按 `/`，`list_search.rs`），`↑` `↓` 选，`Enter`
//! 换（下一个回合开始生效），没有 key 的不换、提示为什么；`Esc` 关。鼠标悬停选中、点一下等于 `Enter`，滚轮滚（照会话列表）。
//!
//! 换的是这个会话的（内核的 `Configure`），也记成新会话的默认（`models.chat`，同 Miyu：手动换的也记成新会话的默认，
//! 2026-10-02 项目主人定）。还没开会话的只记默认，开会话时照它。底栏当场写成选的那个，不弹提示（2026-10-01 项目主人）。
//!
//! 应急模式（设计第五节第 16 条）照样开这个框，只在这一次运行里换：「记成新会话的默认」引擎只记在内存里、不写配置
//! （`core/engine/models.rs` 的 `chat_default`），不在后台拉供应商的模型列表（拉到了要存）；另多一行照打的字换
//! （`rescue.rs` 的 `rescue_models`）。

use crossterm::event::{KeyCode, KeyEvent, MouseEvent, MouseEventKind};
use ratatui::layout::Position;

use super::list_search::{self, Step};
use super::{App, Panel};
use crate::app::model_list::ModelList;
use crate::core::{Choice, Command};

impl App {
    /// 开框，向引擎要一行行。
    pub(super) fn open_models(&mut self) {
        if !self.reachable() {
            return;
        }
        self.boxes.model_list = Some(ModelList::default());
        self.boxes.panel = Some(Panel::Models);
        self.link.core.send(Command::Choices);
        if self.rescue.is_none() {
            self.lists.start();
        }
    }

    /// 引擎交回了一行行：光标停在正在用的那一个；后台拉到了新列表又交一次的，停在原来选着的那一个上。
    pub(super) fn models_listed(&mut self, all: Vec<Choice>) {
        let current = self.talk.transcript.current_ref();
        let (all, manual) = self.rescue_models(all, current.as_deref());
        if let Some(list) = &mut self.boxes.model_list {
            list.manual = manual;
            let kept = list
                .loaded
                .then(|| list.picked().map(|choice| choice.reference.clone()))
                .flatten();
            list.replace(all, kept.as_deref().or(current.as_deref()));
        }
    }

    /// 每一拍收一下 `/model` 开框时在后台拉的列表（[`Lists`]）：拉到了、存下了的，叫引擎重读存下的列表，框还开着的再要一次一行行。
    pub(super) fn poll_lists(&mut self) {
        if !self.lists.done() {
            return;
        }
        self.link.core.send(Command::ReloadLists);
        if self.boxes.panel == Some(Panel::Models) {
            self.link.core.send(Command::Choices);
        }
    }

    /// 框开着时的按键。
    pub(super) fn models_key(&mut self, key: KeyEvent) {
        let vim = self.config.layout.vim;
        let Some(list) = self.boxes.model_list.as_mut() else {
            self.boxes.panel = None;
            return;
        };
        match list_search::key(vim, &mut list.typing, &list.query, key) {
            Step::Query(query) => return list.typed(query),
            Step::Kept => return,
            Step::Pass => {}
        }
        match key.code {
            KeyCode::Esc => self.close_models(),
            KeyCode::Up => list.step(false),
            KeyCode::Down => list.step(true),
            KeyCode::Enter => {
                if let Some(choice) = list.picked().cloned() {
                    self.choose_model(&choice);
                }
            }
            _ => {}
        }
    }

    /// 鼠标在框上：悬停选中，点一下换，滚轮挪露出的那一段；交回归不归它。
    pub(super) fn models_mouse(&mut self, mouse: MouseEvent, at: Position) -> bool {
        if self.boxes.panel != Some(Panel::Models) || !self.screen.areas.menu.contains(at) {
            return false;
        }
        let rows = self.config.layout.session_rows.max(1);
        let by = match mouse.kind {
            MouseEventKind::ScrollUp => -1,
            MouseEventKind::ScrollDown => 1,
            _ => 0,
        };
        let row =
            at.y.checked_sub(self.screen.areas.menu_text.y)
                .map(usize::from);
        let index = row.and_then(|row| self.screen.panel_rows.get(row).copied().flatten());
        let inside = self.screen.areas.menu_text.contains(at);
        let Some(list) = self.boxes.model_list.as_mut() else {
            return true;
        };
        let count = list.matches().len();
        if by != 0 {
            // 从现在露出的那一段起挪（`menu::edge` 记着的）。
            list.pinned = list.pinned.or(Some(list.top.get()));
            crate::menu::pin(list.selected, &mut list.pinned, count, rows, by);
            return true;
        }
        let Some(index) = index.filter(|_| inside) else {
            return true;
        };
        crate::menu::pin(list.selected, &mut list.pinned, count, rows, 0);
        list.selected = index;
        if matches!(mouse.kind, MouseEventKind::Down(_))
            && let Some(choice) = list.picked().cloned()
        {
            self.choose_model(&choice);
        }
        true
    }

    fn close_models(&mut self) {
        self.boxes.panel = None;
        self.boxes.model_list = None;
    }

    /// 换成选中的那个：没有 key 的不换，提示为什么；别的（[`crate::core::ChoiceState::pickable`]）交给引擎，
    /// 关框。开着会话的换这个会话的，都记成新会话的默认。
    fn choose_model(&mut self, choice: &Choice) {
        if !choice.state.pickable() {
            let note = self.config.text().model_panel.refuse_no_key.clone();
            self.hint(note, false);
            return;
        }
        self.close_models();
        if self.talk.transcript.current_ref().as_deref() == Some(choice.reference.as_str()) {
            return;
        }
        if self.talk.transcript.session.is_some() {
            self.link
                .core
                .send(Command::Configure(choice.reference.clone()));
        }
        self.link
            .core
            .send(Command::ChatDefault(choice.reference.clone()));
        self.link.default_chat = Some(choice.reference.clone());
        self.talk.transcript.chose_model(&choice.reference);
    }
}

/// `/model` 开框时在后台拉模型列表（配置 五，照 Miyu：开选模型的框时 `model.list` 列整份的，每家最多一分钟在后台拉一次）：配好的几家里
/// 用得上的、到了时候的（`lm_engine::providers::lists::due`）在一条做完就退的线程里列、存下；读配置、读目录、列、存都在那条线程里，
/// 界面不等。照 Miyu 不说话：拉不到的不打扰（Miyu 记一行运行日志；LiteMiyu 的界面这一头没有运行日志），拉到的框里直接多出来。记的「拉过」和配置界面共用一份。
#[derive(Debug, Default)]
pub struct Lists {
    /// 每一家最近一次在后台拉的时刻（配置界面打开时拿同一份）。
    pub claims: lm_engine::providers::lists::Claims,
    /// 正在拉的那一条线程：拉完交回存下了没有。
    running: Option<std::sync::mpsc::Receiver<bool>>,
}

impl Lists {
    /// 起一条线程拉；上一条还没完的不再起。
    fn start(&mut self) {
        if self.running.is_some() {
            return;
        }
        let Ok(paths) =
            lm_engine::store::paths::Paths::locate(&lm_engine::store::paths::Env::current())
        else {
            return;
        };
        let claims = self.claims.clone();
        let (sender, running) = std::sync::mpsc::channel();
        let spawned = std::thread::Builder::new()
            .name("lists".to_string())
            .spawn(move || {
                let stored = refresh(&paths, &claims);
                if sender.send(stored).is_err() {}
            });
        if spawned.is_ok() {
            self.running = Some(running);
        }
    }

    /// 在拉：主循环照节拍醒来收（`deadline.rs`）。
    pub fn running(&self) -> bool {
        self.running.is_some()
    }

    /// 拉完了、存下了新的列表：交回真（交一次）。
    fn done(&mut self) -> bool {
        let Some(running) = &self.running else {
            return false;
        };
        match running.try_recv() {
            Ok(stored) => {
                self.running = None;
                stored
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => false,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.running = None;
                false
            }
        }
    }
}

/// 在后台线程里：照盘上的配置、目录，配好的几家里用得上的、到了时候的列一次、存下；交回存下了没有。
fn refresh(
    paths: &lm_engine::store::paths::Paths,
    claims: &lm_engine::providers::lists::Claims,
) -> bool {
    use lm_engine::providers::{lists, probe};
    let env = |name: &str| std::env::var(name).ok();
    let service = lm_engine::settings::Settings::new(paths.clone(), env);
    let Ok(snapshot) = service.read() else {
        return false;
    };
    if snapshot.broken.is_some() {
        return false;
    }
    let Ok(catalog) = lm_engine::settings::catalog::Catalog::load(paths) else {
        return false;
    };
    let mut stored = false;
    for id in lm_kernel::models::provider::configured(&snapshot.resolved.values()) {
        let Ok(target) = probe::target(&service, &snapshot, &catalog, &id, None, &env) else {
            continue;
        };
        if !lists::due(paths, claims, &id) {
            continue;
        }
        let (_keep, never) = tokio::sync::oneshot::channel();
        // 拉不到、存不下的照 Miyu 不打扰：框里照旧列存下的、配置里写的、目录里的。
        if let Some(Ok(names)) = probe::list(&target, crate::settings::lists::WAIT, never)
            && !names.is_empty()
            && lists::store(paths, &id, &names).is_ok()
        {
            stored = true;
        }
    }
    stored
}
