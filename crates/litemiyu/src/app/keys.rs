//! 按键：列表开着时先归列表，别的给输入框；两下 `Esc`、翻正文、清屏、斜杠命令的执行（`tui.md`「按键」）。

use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent};

use super::App;
use crate::commands::{Line, Run, Spec};
use crate::core::Command;
use crate::input::{Action, Draft};
use crate::transcript::Starting;

impl App {
    /// 按了要按两下的 `Esc`：在回答时打断（排着队的接着发）。上一下还在时限里就做，不然只提示再按一次。
    pub(super) fn esc(&mut self) {
        let window = Duration::from_millis(self.config.layout.esc_window_ms);
        // 发出去了、核心还没开轮的也算在回答：记下，轮一开就打断（`takeback.rs`）。
        let starting = self.talk.transcript.starting != Starting::No;
        let running = self.talk.transcript.running.is_some() || starting;
        match self.typing.esc_at.take() {
            Some(at) if at.elapsed() <= window => {
                self.typing.notice = None;
                if starting {
                    self.talk.transcript.starting = Starting::Stop;
                } else if running {
                    // 她还没开始做事、又没有排着的：打断以后撤掉这一轮，那一句放回输入框（「按键」`Esc`，2026-10-07
                    // 项目主人定；和 Ctrl+C 退回排着的走同一条路，`takeback.rs`）。
                    let queued = self
                        .talk
                        .transcript
                        .entries
                        .iter()
                        .any(|e| e.queued && !e.hidden);
                    if !queued && let Some(back) = self.talk.transcript.untouched() {
                        let session = self.talk.transcript.session.clone();
                        self.talk.takeback =
                            Some(super::takeback::Takeback::Interrupting(session, back));
                    }
                    self.link.core.send(Command::Interrupt { send: true });
                }
            }
            _ => {
                self.typing.esc_at = Some(Instant::now());
                let hint = self.config.text().esc_hint.clone();
                self.hint(hint, false);
            }
        }
    }

    /// 换下一套图标，只管这一次启动（`tui.md`「图标」第 4 条）。排好的时间线照图标那一套的名字重排。
    pub(super) fn next_icons(&mut self) {
        // 出厂的几套这时才读；读不懂的（安装坏了）照旧用现在这一套，说一声为什么。
        let sets = match crate::config::builtin_icons() {
            Ok(sets) => sets,
            Err(why) => return self.hint(why, false),
        };
        let at = sets
            .iter()
            .position(|s| s.name == self.config.icons.name)
            .unwrap_or(0);
        if let Some(next) = sets.get((at + 1) % sets.len().max(1)).cloned() {
            let text = self
                .config
                .text()
                .icons_changed
                .replace("{name}", &next.name);
            self.config.icons = next;
            self.hint(text, true);
        }
    }

    /// 按键：列表开着时，上下、Tab、Enter、Esc 归列表，别的照旧给输入框。
    pub(super) fn key(&mut self, key: KeyEvent) -> Action {
        // Ctrl+V（Alt+V 同）：读剪贴板，照粘贴处理；列表、抽屉开着时也一样（`tui.md`「按键」）。
        if is_paste(&key) {
            self.paste_clipboard();
            return Action::None;
        }
        // 输入历史列表开着时，按键先归它（`tui.md`「输入历史列表」）。
        if self.typing.history.open {
            self.history_key(key);
            return Action::None;
        }
        // 确认和提问的抽屉开着时按键先归它（`tui.md`「确认和提问的抽屉」第 4 条）；后台面板、`/model`、会话列表、帮助这些
        // 框开着时也先归抽屉（同 miyu-tui `app/keys.rs`）。
        if self.drawer_key(key) {
            return Action::None;
        }
        // 后台面板开着时按键先归它（`jobs.rs`）。
        if self.boxes.panel == Some(super::Panel::Jobs) {
            self.jobs_key(key);
            return Action::None;
        }
        // `/model`、`/effort` 的框开着时按键先归它（`models.rs`、`effort.rs`）。
        if self.boxes.panel == Some(super::Panel::Models) {
            self.models_key(key);
            return Action::None;
        }
        if let Some(super::Panel::Effort { selected }) = self.boxes.panel {
            self.effort_key(selected, key);
            return Action::None;
        }
        // `/preset` 的框开着时按键先归它（`presets.rs`）。
        if self.boxes.panel == Some(super::Panel::Presets) {
            self.presets_key(key);
            return Action::None;
        }
        // 会话列表开着时按键先归它（`sessions.rs`）。
        if self.boxes.panel == Some(super::Panel::Sessions) {
            self.sessions_key(key);
            return Action::None;
        }
        // 帮助框开着时按键先归它。
        if let Some(super::Panel::Help { scroll }) = self.boxes.panel {
            self.help_key(scroll, key);
            return Action::None;
        }
        if let Some(super::Panel::Status { scroll }) = self.boxes.panel {
            self.status_key(scroll, key);
            return Action::None;
        }
        // 框下面那一行的按钮、子代理状态行先拿按键；输入框空着、没开列表时按 `↓` 焦点往下走（`agents.rs`，Miyu `jobs_key`）。
        let menu_open = self.menu_matches().is_some();
        if self.focus_key(key, menu_open) {
            return Action::None;
        }
        // 正文里有选区：Ctrl+C、Ctrl+Shift+C 复制它，Esc 取消它；都先于输入框（`tui.md`「按键」）。
        let ctrl = key
            .modifiers
            .contains(crossterm::event::KeyModifiers::CONTROL);
        // 侧边栏有选区：Ctrl+C 复制它，Esc 取消它（Miyu 终端 `app/keys.rs`，`tui.md`「后台命令、子代理和侧边栏」第 7 条）。
        if self.screen.side_select.active() {
            if ctrl && matches!(key.code, KeyCode::Char('c' | 'C')) {
                let text = self.screen.side_select.text();
                self.copy(&text);
                return Action::None;
            }
            if key.code == KeyCode::Esc {
                self.screen.side_select.clear();
                return Action::None;
            }
        }
        if self.screen.view.select.is_some() {
            if ctrl && matches!(key.code, KeyCode::Char('c' | 'C')) {
                let text = self.screen.view.selected_text();
                self.copy(&text);
                return Action::None;
            }
            if key.code == KeyCode::Esc {
                self.screen.view.select = None;
                return Action::None;
            }
        }
        // Ctrl+L：清屏，把视口顶空，往回滚内容还在；回答进行中不清（新的字马上又冒出来）。
        if ctrl && key.code == KeyCode::Char('l') {
            if self.talk.transcript.running.is_some() {
                self.hint(self.config.text().no_clear_running.clone(), false);
            } else {
                self.screen.view.clear();
            }
            return Action::None;
        }
        // PgUp、PgDn：翻正文。
        if matches!(key.code, KeyCode::PageUp | KeyCode::PageDown) {
            self.screen.view.page(key.code == KeyCode::PageDown);
            return Action::None;
        }
        // Ctrl+R：调出输入历史列表；恢复撤销只用 /restore（`tui.md`「按键」）。
        if ctrl && key.code == KeyCode::Char('r') {
            self.typing.input.recall_saved();
            if self.typing.input.sent().is_empty() {
                self.hint(self.config.text().history.none.clone(), false);
            } else {
                self.typing.history.open();
            }
            return Action::None;
        }
        // Ctrl+End：回到正文最底下，跟着最新的（`tui.md`「按键」，2026-10-02 项目主人要）。
        if ctrl && key.code == KeyCode::End {
            self.screen.view.follow();
            return Action::None;
        }
        // Ctrl+Enter：输入框有字先发（在回答时排进队），在回答的当场打断、排着的马上发（`tui.md`「按键」，
        // 2026-10-02 项目主人要；不提示）。没在回答时同 Enter。
        if ctrl && key.code == KeyCode::Enter && self.menu_matches().is_none() {
            let running = self.talk.transcript.running.is_some();
            let plain = KeyEvent::new(KeyCode::Enter, crossterm::event::KeyModifiers::NONE);
            let action = self.typing.input.key(plain);
            if running {
                if let Action::Submit(text) = action {
                    self.submit(text);
                }
                // 没有排着的（输入框也是空的）：什么都不做，不当打断用。
                if self.talk.transcript.entries.iter().any(|e| e.queued) {
                    self.link.core.send(Command::Interrupt { send: true });
                }
                return Action::None;
            }
            return action;
        }
        let Some(matches) = self.menu_matches() else {
            // `/new <打了一半的预设名>`：`Tab` 补全（`presets.rs`）。
            if key.code == KeyCode::Tab && self.complete_preset() {
                return Action::None;
            }
            return self.typing.input.key(key);
        };
        let selected = matches.get(self.typing.menu.selected).cloned();
        match (key.code, selected) {
            // `↑`（`Ctrl+K`、vim 的 `k` 换成的也是它）只在列表里往上挪，不翻输入历史（2026-10-20 项目主人：「出命令列表就应该
            // 不允许调出输入历史了」；原来 10-17 定真的 `↑` 交给输入框翻历史，`Ctrl+K` 换成的 `↑` 也跟着翻了过去）。
            (KeyCode::Up, _) => self.typing.menu.step(false, matches.len()),
            (KeyCode::Down, _) => self.typing.menu.step(true, matches.len()),
            (KeyCode::Tab, Some(spec)) => {
                self.typing.input.editor.take();
                self.typing.input.paste(&format!("/{}", spec.name));
            }
            (KeyCode::Enter, Some(spec)) => {
                self.typing.input.editor.take();
                self.run(&spec, None);
            }
            (KeyCode::Esc, _) => self.typing.menu.dismiss(self.typing.input.editor.text()),
            _ => return self.typing.input.key(key),
        }
        Action::None
    }

    /// 回车发出去的字：是命令就执行；像命令、又没有这个命令的弹一句（`tui.md`「斜杠命令列表」第 4 条）；别的发给她。
    pub(super) fn submit(&mut self, draft: Draft) {
        // 跑命令的模式：照命令发，斜杠打头的也不当命令（`shell_send.rs`）。
        if self.typing.input.shell() {
            return self.submit_shell(draft);
        }
        let text = draft.text.clone();
        // 在编辑上一句：打了命令就不编辑了；打错了命令、发不出去的，接着编辑（「输入框」第 13 条）。
        let edit = self.typing.input.take_edit();
        let line = match self.config.commands.read(&text) {
            Line::Command(spec, _) if !self.offered(spec) => Line::Unknown,
            line => line,
        };
        match line {
            Line::Command(spec, words) => {
                let (spec, words) = (spec.clone(), words.map(str::to_string));
                self.run(&spec, words.as_deref());
                return;
            }
            // 没有这个命令：记进输入历史，弹一句；那一行不放回输入框（同 miyu-tui，2026-10-30 项目主人定，要改按 `↑`）。
            // 在编辑上一句的接着编辑。
            Line::Unknown => {
                if let Some(original) = edit {
                    self.typing.input.resume_edit(original);
                }
                self.typing.input.remember(draft);
                if let Some(hint) = self
                    .config
                    .text()
                    .refusal_hints
                    .get("unknown_command")
                    .cloned()
                {
                    self.hint(hint, false);
                }
                return;
            }
            Line::Talk => {}
        }
        // 引擎起不来：发不出去，字留在输入框里（「连核心」第 8 条）。
        if !self.reachable() {
            self.typing.input.editor.set_draft(draft);
            if let Some(original) = edit {
                self.typing.input.resume_edit(original);
            }
            return;
        }
        if let Some(original) = edit {
            return self.submit_edit(original, draft);
        }
        self.screen.view.follow();
        // 输入框里的粘贴块发出去换回原文；输入历史和正文里照输入框的样子（`tui.md`「输入框」第 11 条）。
        let full = draft.expand();
        let chips = super::paste::chips(&draft);
        let images = self.images(&draft);
        self.talk.unsent = Some(super::Unsent {
            draft: Some(draft.clone()),
            original: None,
            turn: None,
        });
        self.typing.input.remember(draft);
        self.typing.input.forget_cleared();
        self.talk.transcript.user(text, chips);
        self.awaiting_turn();
        self.link.core.send(Command::Send { text: full, images });
    }

    /// 执行一条命令；`words` 是名字后面的字（能带参数的命令才有，蓝图「斜杠命令列表」第 5 条）。
    pub(super) fn run(&mut self, spec: &Spec, words: Option<&str>) {
        // 命令也记进输入历史：从列表里回车、点的，和整条打出来回车的一样，带参数的记整条（`tui.md`「按键」↑、↓）。
        let line = match words {
            Some(words) => format!("/{} {words}", spec.name),
            None => format!("/{}", spec.name),
        };
        self.typing.input.remember(Draft::plain(&line));
        self.screen.view.follow();
        match spec.run {
            Run::Revert
            | Run::Unrevert
            | Run::Redo
            | Run::Edit
            | Run::Compact
            | Run::Clear
            | Run::Rename
            | Run::Delete
            | Run::Export
                if !self.reachable() =>
            {
                self.typing.input.editor.set_draft(Draft::plain(&line));
            }
            Run::Revert
            | Run::Unrevert
            | Run::Redo
            | Run::Edit
            | Run::Compact
            | Run::Clear
            | Run::Stop
            | Run::Rename
            | Run::Delete
            | Run::Export
                if self.not_opened() =>
            {
                self.nothing_yet(spec.run)
            }
            Run::Revert => self.undo(),
            Run::Unrevert => self.link.core.send(Command::Unrevert),
            Run::Redo => self.redo(),
            Run::Edit => self.edit_last(),
            Run::Rename => self.rename(words),
            Run::Clear => self.link.core.send(Command::Clear),
            Run::Stop => self.link.core.send(Command::Stop),
            Run::Jump => self.open_jump(),
            Run::Files => self.open_files(0),
            Run::Copy => self.copy_reply(),
            Run::New => self.new_session_with(words),
            Run::Sessions => self.open_sessions(),
            Run::Delete => self.delete_current(),
            Run::Export => self.export(),
            Run::Model => self.open_models(),
            Run::Effort => self.open_effort(),
            Run::Preset => self.open_presets(),
            Run::Compact => self.compact(words),
            Run::Icons => self.next_icons(),
            Run::Quit => self.quit = true,
            Run::Help => self.open_help(),
            Run::Status => self.open_status(),
            Run::Config => self.open_settings(crate::settings::Outcome::Back),
            Run::Loop if !self.reachable() => {
                self.typing.input.editor.set_draft(Draft::plain(&line));
            }
            Run::Loop => self
                .link
                .core
                .send(Command::Loop(words.unwrap_or_default().to_string())),
        }
    }

    /// `/copy`：复制她上一轮的回答，照拖选以后 Ctrl+C 那一套；还没有回答的弹一句（蓝图「斜杠命令」`/copy`）。
    fn copy_reply(&mut self) {
        match self.talk.transcript.last_reply() {
            Some(text) => self.copy(&text),
            None => {
                let note = self.config.text().nothing_to_copy.clone();
                self.hint(note, false);
            }
        }
    }

    /// 输入历史列表开着时的按键：`↑`、`Ctrl+R` 往更早的走，`↓` 往更新的走，打字进搜索，
    /// `Enter` 挑中放进输入框，`Esc` 关掉（`tui.md`「输入历史列表」第 2–4 条；`Tab` 不做什么，第 7 条）。按键模式是 Vim 的
    /// 搜索照会话列表那一套：`/` 进搜索，平时 `j` `k` 上下（Miyu 发版前 check 第 31 条，`list_search.rs`）。
    fn history_key(&mut self, key: KeyEvent) {
        use super::list_search::{self, Step};
        use crossterm::event::KeyModifiers;
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if !ctrl && !key.modifiers.contains(KeyModifiers::ALT) {
            let history = &mut self.typing.history;
            match list_search::key(
                self.config.layout.vim,
                &mut history.typing,
                &history.query,
                key,
            ) {
                Step::Query(query) => return history.set_query(query),
                Step::Kept => return,
                Step::Pass => {}
            }
        }
        let count = self.typing.history.matches(&self.list_items().0).len();
        match key.code {
            KeyCode::Up => self.typing.history.older(count),
            KeyCode::Char('r') if ctrl => self.typing.history.older(count),
            KeyCode::Down => self.typing.history.newer(),
            KeyCode::Enter => self.pick_history(),
            KeyCode::Esc => self.typing.history.close(),
            _ => {}
        }
    }

    /// 挑中选中的那一条：放进输入框，关掉列表；`/jump` 的跳过去（「跳到说过的话」第 3 条）。
    pub(super) fn pick_history(&mut self) {
        let (text, spot) = {
            let (list, spots) = self.list_items();
            let found = self.typing.history.matches(&list);
            let picked = found.get(self.typing.history.selected).copied();
            // 列表是照先后排的一份，挑中的是其中哪一句：照位置找回它跳到哪。
            let spot = picked
                .and_then(|p| list.iter().position(|s| std::ptr::eq(s, p)))
                .and_then(|i| spots.get(i).copied());
            (picked.map(|sent| sent.draft.text.clone()), spot)
        };
        let jump = self.typing.history.jump;
        self.typing.history.close();
        match (jump, spot, text) {
            (true, Some(spot), _) => self.go(spot),
            (false, _, Some(text)) => self.typing.input.pick(&text),
            _ => {}
        }
    }

    /// 前台压缩（`/compact`）：自己发的照手动画进度条（「正文」第 9 条）。
    pub(super) fn compact(&mut self, words: Option<&str>) {
        self.talk.transcript.manual_compaction = true;
        self.link
            .core
            .send(Command::Compact(words.map(str::to_string)));
    }
}

/// 这一下是不是读剪贴板：`Ctrl+V`（认得 kitty 键盘协议的终端里按着 Shift 报成大写的 V，不算：那是终端自己的粘贴），
/// 或者 `Alt+V`（Windows Terminal 自己占着 `Ctrl+V`，剪贴板里只有图时什么都不送进来，2026-10-26 虚拟机里实测；同 Claude Code）。
pub(crate) fn is_paste(key: &KeyEvent) -> bool {
    use crossterm::event::KeyModifiers;
    key.code == KeyCode::Char('v')
        && (key.modifiers == KeyModifiers::CONTROL || key.modifiers == KeyModifiers::ALT)
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use super::is_paste;

    #[test]
    fn ctrl_v_reads_the_clipboard_but_ctrl_shift_v_is_the_terminals() {
        let key = |code, m| KeyEvent::new(code, m);
        assert!(is_paste(&key(KeyCode::Char('v'), KeyModifiers::CONTROL)));
        // 按着 Shift 的是终端自己的粘贴（走括号粘贴），不读剪贴板。
        assert!(!is_paste(&key(
            KeyCode::Char('V'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT
        )));
        assert!(!is_paste(&key(KeyCode::Char('v'), KeyModifiers::NONE)));
        assert!(is_paste(&key(KeyCode::Char('v'), KeyModifiers::ALT)));
    }
}
