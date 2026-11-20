//! `/effort`：换这个会话的思考程度（Miyu 终端 `app/effort.rs`，蓝图 `tui.md`「配置与模型」思考程度，核心 SM-1；2026-10-30
//! 项目主人定：`/effort` 只管当前会话，配置页管新会话的默认）。开框时向引擎要正在用的模型（没有的照会话的引用、默认的聊天
//! 模型）有哪几级，第一行「跟随设置」；`Enter` 交给引擎（内核的 `SetEffort`），关框，不弹提示，成了底栏当场变。还没开会话的
//! 引擎记着，开会话时带上。用的是模型池的提示一句，不开框（2026-10-02、10-30 项目主人定：池不能设）。
//!
//! 和 Miyu 不一样的：Miyu 每次连上、换了会话都要一次 `model.list` 给底栏写这一档；LiteMiyu 的底栏照回合开始时推来的
//! `model.changed` 写，只在开框时问引擎（平常不多跑）。

use crossterm::event::{KeyCode, KeyEvent, MouseEvent, MouseEventKind};
use ratatui::layout::Position;

use super::{App, Panel};
use crate::core::{Command, Efforts};

impl App {
    /// 开框，向引擎要几级。
    pub(super) fn open_effort(&mut self) {
        if !self.reachable() {
            return;
        }
        let reference = self.effort_reference();
        self.boxes.efforts = None;
        self.boxes.panel = Some(Panel::Effort { selected: 0 });
        self.link.core.send(Command::Efforts(reference));
    }

    /// 改哪个模型的：这一轮真发的那个（底栏写着的），没有的照会话的引用；都没有的是 `None`，引擎照默认的聊天模型补。
    fn effort_reference(&self) -> Option<String> {
        let t = &self.talk.transcript;
        t.model
            .as_ref()
            .filter(|(_, endpoint)| !endpoint.is_empty())
            .map(|(model, endpoint)| format!("{endpoint}/{model}"))
            .or_else(|| t.model_ref().map(str::to_string))
    }

    /// 引擎交回了这个模型有哪几级：框开着的照它排，选着会话自己定的那一级。
    pub(super) fn efforts_listed(&mut self, reference: String, levels: Vec<String>) {
        if !matches!(self.boxes.panel, Some(Panel::Effort { .. })) {
            return;
        }
        let name = reference
            .split_once('/')
            .map_or(reference.as_str(), |(_, model)| model)
            .to_string();
        let target = Efforts {
            reference,
            name,
            levels,
            current: self.talk.transcript.own_effort().map(str::to_string),
        };
        let selected = crate::ui::effort_list::initial(&target);
        self.boxes.panel = Some(Panel::Effort { selected });
        self.boxes.efforts = Some(target);
    }

    /// 框开着时的按键。
    pub(super) fn effort_key(&mut self, selected: usize, key: KeyEvent) {
        let last = self.boxes.efforts.as_ref().map_or(0, |t| t.levels.len());
        let at = |selected| Some(Panel::Effort { selected });
        // `j`、`k` 在前头换成了 `↓`、`↑`（开着不能打字的框，`vim.rs`，同 Miyu）。
        match key.code {
            KeyCode::Up => self.boxes.panel = at(selected.saturating_sub(1)),
            KeyCode::Down => self.boxes.panel = at((selected + 1).min(last)),
            KeyCode::Enter => self.choose_effort(selected),
            KeyCode::Esc => self.boxes.panel = None,
            _ => {}
        }
    }

    /// 鼠标在框上：悬停选中，点一下换；交回归不归它。
    pub(super) fn effort_mouse(&mut self, mouse: MouseEvent, at: Position) -> bool {
        let Some(Panel::Effort { .. }) = self.boxes.panel else {
            return false;
        };
        let areas = self.screen.areas;
        if !areas.menu.contains(at) {
            return false;
        }
        let row = at.y.checked_sub(areas.menu_text.y).map(usize::from);
        let index = row.and_then(|row| self.screen.panel_rows.get(row).copied().flatten());
        if let Some(index) = index.filter(|_| areas.menu_text.contains(at)) {
            self.boxes.panel = Some(Panel::Effort { selected: index });
            if matches!(mouse.kind, MouseEventKind::Down(_)) {
                self.choose_effort(index);
            }
        }
        true
    }

    /// 换成第 `index` 行（第 0 行跟随设置）：交给引擎，关框，不弹提示（2026-10-01 项目主人：改模型、思考程度不要通知）。
    /// 和现在一样的只关框。成了底栏照 [`App::effort_set`] 变。
    fn choose_effort(&mut self, index: usize) {
        self.boxes.panel = None;
        let Some(target) = self.boxes.efforts.take() else {
            return;
        };
        let level = index
            .checked_sub(1)
            .and_then(|i| target.levels.get(i).cloned());
        if index > 0 && level.is_none() || level == target.current {
            return;
        }
        self.link.core.send(Command::Effort(level));
    }

    /// 这个会话的思考程度换成了（引擎答应了，或者还没开会话、引擎记下了）：定了一档的底栏当场写它。
    pub(super) fn effort_set(&mut self, level: Option<String>) {
        self.talk.transcript.own_effort_set(level);
    }

    /// 读到默认的聊天模型（Miyu 终端 `chat_configured`）：记下，还没开会话的底栏照它写。Miyu 是连上核心时向核心要
    /// `models.chat`；LiteMiyu 照启动时已经读进来的 `config.toml`、配置界面存了的、`/model` 选了的，引擎线程不为这一格起来。
    pub fn chat_configured(&mut self, chat: Option<String>) {
        self.link.default_chat = chat;
        self.footer_default();
    }

    /// 底栏：还没开会话的照默认的聊天模型写（Miyu 终端 `footer_effort` 还没开会话那一支，`Transcript::show_default`）；
    /// 开了会话的照推来的，不动。思考程度写会话自己定的那一档（`/effort` 开会话前选了的）；跟着配置的不写——那一档要照模型
    /// 目录算，LiteMiyu 不为底栏读目录，开了会话照推来的写（和 Miyu 不一样的一处）。没配默认模型的照旧空着（同 Miyu）。
    pub(super) fn footer_default(&mut self) {
        let Some(chat) = self.link.default_chat.clone() else {
            return;
        };
        let own = self.talk.transcript.own_effort().map(str::to_string);
        self.talk.transcript.show_default(&chat, own);
    }
}
