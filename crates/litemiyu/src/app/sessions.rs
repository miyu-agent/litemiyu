//! 会话的进出（Miyu 终端 `app/switch.rs`，蓝图 `tui.md`「会话列表 `/sessions`」、「后台命令、子代理和侧边栏」第 7 条）：
//! `/sessions` 开会话列表的框，打字筛，`↑` `↓` 选，`Enter` 切过去，空格勾、`Ctrl+A` 全勾、`Ctrl+D` 按两下删，`Esc` 关；
//! 鼠标悬停选中、点一行切过去，滚轮挪露出的那一段。侧边栏「最近会话」点一行也切过去。`/new bare`、`/new full` 定新会话开哪一档（设计第五节第 14 条）。
//!
//! 列表照引擎扫的会话目录（`Command::Sessions`，连这个窗口开着的一起列，最近有动静的在前）：开框时要一份，侧边栏看得到
//! 时、会话开了换了、一轮做完、起了标题以后再要一份；看不到侧边栏、没开框时不扫（设计第五节第 12 条）。
//!
//! 和 Miyu 不一样的：切走时原来那个不停放着接着跑（LiteMiyu 一个窗口只开一个主会话，切走就停下它，同 `/new`）；别的
//! 窗口开着的不硬开，照实说「这个会话在别的窗口开着」；没有置顶（LiteMiyu 的会话没有这一样）。

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseEvent, MouseEventKind};
use lm_engine::session::Listed;
use ratatui::layout::Position;

use super::list_search::{self, Step};
use super::{App, Panel};
use crate::core::{Command, Push, Update};
use crate::session_list::SessionList;

/// 最近列出来的会话（侧边栏「最近会话」、会话列表照它画）。
#[derive(Debug, Default)]
pub struct Recent {
    /// 引擎最近交回的那一份；还没要过的是 `None`。
    pub listed: Option<Vec<Listed>>,
    /// 要过、还没交回来。
    asked: bool,
    /// 手上的那一份旧了（会话开了、换了，一轮做完，起了标题）：看得到侧边栏时再要一份。
    pub(super) stale: bool,
}

/// 要开的旧会话补到哪了（第十九趟，同 Miyu `app/switch.rs` 的 `opening`、`start_entered.rs` 的 `entering`）：`-c`、`-s`
/// 起来时、会话列表和侧边栏里点了的，交给引擎去开到补完为止。带着在开的会话编号。
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub enum Opening {
    /// 没在开。
    #[default]
    None,
    /// 要了，引擎还没开好：屏幕上照旧是原来那个；起来时还没有原来那个的，照会话的样子画、正文先空着，不画首页。
    Asked(String),
    /// 开好了，正文换了一份空的、在收补发：这之间不画（屏幕上照旧是上一帧），补完了当场画（[`App::opening_done`]）。
    Filling(String),
}

impl Opening {
    /// 正在开的就是 `id`（要了还没开好，或者在收补发）。
    fn of(&self, id: &str) -> bool {
        matches!(self, Opening::Asked(open) | Opening::Filling(open) if open == id)
    }
}

impl App {
    /// 要引擎开旧会话 `id`：补完以前不画首页、不画补了一半的（[`Opening`]）。
    pub(crate) fn open_session(&mut self, id: String) {
        self.talk.opening = Opening::Asked(id.clone());
        self.link.core.send(Command::Open {
            id,
            view_broken: true,
        });
    }

    /// 补完了（钟回到现在）、开不了：不等了，当场画。
    pub(super) fn opening_done(&mut self) {
        if self.talk.opening != Opening::None {
            self.talk.opening = Opening::None;
            self.screen.draw_now = true;
        }
    }

    /// 这一帧画不画：开好的旧会话还在收补发的不画，屏幕上照旧是上一帧（原来那个会话，或者起来时会话的样子）。
    pub fn holding(&self) -> bool {
        matches!(self.talk.opening, Opening::Filling(_))
    }

    /// 会话的进出那几种消息：交回 `true` 的不再往下走。
    pub(super) fn sessions_update(&mut self, update: &Update) -> bool {
        match update {
            Update::Sessions { listed } => {
                self.talk.recent.asked = false;
                match listed {
                    Ok(listed) => {
                        if let Some(list) = &mut self.boxes.sessions {
                            list.replace(listed.clone());
                        }
                        self.talk.recent.listed = Some(listed.clone());
                    }
                    Err(reason) => {
                        if self.boxes.sessions.is_some() {
                            let texts = &self.config.text().sessions;
                            let note = texts.list_failed.replace("{reason}", reason);
                            self.hint(note, false);
                        }
                    }
                }
                return true;
            }
            Update::Deleted(done) => {
                self.talk.recent.stale = true;
                if let Err(reason) = done {
                    let texts = &self.config.text().sessions;
                    let note = texts.delete_failed.replace("{reason}", reason);
                    self.hint(note, false);
                }
                return true;
            }
            Update::Opened(session) => {
                self.opened(session.clone());
                if let Opening::Asked(id) = &self.talk.opening {
                    self.talk.opening = Opening::Filling(id.clone());
                }
                return true;
            }
            Update::OpenFailed {
                taken,
                unavailable,
                message,
            } => {
                // 一时读不了（`LoadError::passing`）：不说成打不开、坏了，过几秒自动再开（Miyu 核心 YC-2、终端 `app/unread.rs`）。
                if *unavailable && let Opening::Asked(id) = &self.talk.opening {
                    let id = id.clone();
                    self.opening_done();
                    self.session_unavailable(id);
                    return true;
                }
                let text = self.config.text();
                let note = if *taken {
                    text.sessions.taken.clone()
                } else if *unavailable {
                    // 不知道是在开哪个的（没在开）：不再试，照试满了说。
                    text.sessions.unavailable_gave_up.clone()
                } else {
                    text.sessions.open_failed.replace("{reason}", message)
                };
                self.opening_done();
                self.hint(note, false);
                return true;
            }
            Update::ReadOnly { line } => {
                // 日志中间坏了一行（Miyu 核心 YC-6、终端 `app/unread.rs`）：坏行以前的补发完了，正文末尾写坏在哪，记着只能看。
                let text = self.config.text().sessions.read_only.clone();
                let text = text.replace("{line}", &line.to_string());
                self.talk.transcript.read_only(text);
                return true;
            }
            Update::Ready(_) | Update::Push(Push::Title { .. } | Push::TurnEnded(_)) => {
                self.talk.recent.stale = true;
            }
            _ => {}
        }
        false
    }

    /// 侧边栏看得到：手上的列表旧了（还没有也算）、没在要的，向引擎要一份（画的时候调，`ui/mod.rs`）。
    pub fn want_recent(&mut self) {
        // 应急模式不扫：会话都在磁盘上，开不了（`rescue.rs`）。
        if self.rescue.is_some() {
            return;
        }
        let recent = &mut self.talk.recent;
        if recent.asked || !(recent.stale || recent.listed.is_none()) {
            return;
        }
        if let crate::transcript::Link::Down(_) = self.talk.transcript.link {
            return;
        }
        recent.asked = true;
        recent.stale = false;
        self.link.core.send(Command::Sessions);
    }

    /// `/sessions`：开框，向引擎要列表；这次启动里列过的先照那一份画，交回新的再换。
    pub(super) fn open_sessions(&mut self) {
        if !self.reachable() {
            return;
        }
        let mut list = SessionList {
            current: self.talk.main_session(),
            ..SessionList::default()
        };
        if let Some(listed) = self.talk.recent.listed.clone() {
            list.replace(listed);
        }
        self.boxes.sessions = Some(list);
        self.boxes.panel = Some(Panel::Sessions);
        self.talk.recent.asked = true;
        self.talk.recent.stale = false;
        self.link.core.send(Command::Sessions);
    }

    /// `/delete`：删掉正在说话的会话（Miyu 终端 `app/session.rs` 的 `delete_current`）。回答进行中的先说要打断；别的像 `/new`
    /// 一样回到新会话，再交引擎删掉它的目录，当场弹「已删除「标题」」（同 Miyu，普通通知的颜色）；删不掉的再弹一句为什么。
    pub(super) fn delete_current(&mut self) {
        let Some(session) = self.talk.main_session() else {
            return;
        };
        if self.talk.transcript.running.is_some() {
            let note = self.config.text().sessions.delete_running.clone();
            self.hint(note, false);
            return;
        }
        self.leave_child();
        let texts = &self.config.text().sessions;
        let title = self
            .talk
            .transcript
            .title
            .clone()
            .unwrap_or_else(|| texts.untitled.clone());
        let done = texts.deleted.replace("{title}", &title);
        self.new_session(None);
        self.link.core.send(Command::Delete(session));
        self.hint(done, false);
    }

    /// 关掉会话列表的框。
    fn close_sessions(&mut self) {
        self.boxes.sessions = None;
        if self.boxes.panel == Some(Panel::Sessions) {
            self.boxes.panel = None;
        }
    }

    /// 框开着时的按键：打字筛（按键模式是 Vim 的先按 `/`，`list_search.rs`），`↑` `↓` 选，`Enter` 切过去，空格勾、`Ctrl+A`
    /// 全勾、`Ctrl+D` 删，`Esc` 关（Miyu `app/switch.rs` 的 `sessions_key`）。
    pub(super) fn sessions_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let plain = !key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT);
        let vim = self.config.layout.vim;
        let Some(list) = self.boxes.sessions.as_mut() else {
            self.boxes.panel = None;
            return;
        };
        // 空格勾选；按键模式是 Vim 时按了 `/` 打字时空格是字（拆段搜）。
        let ticking = key.code == KeyCode::Char(' ') && !list.typing;
        if plain && !ticking {
            match list_search::key(vim, &mut list.typing, &list.query, key) {
                Step::Query(query) => return list.typed(query),
                Step::Kept => return,
                Step::Pass => {}
            }
        }
        match key.code {
            KeyCode::Esc => self.close_sessions(),
            KeyCode::Up => list.step(false),
            KeyCode::Down => list.step(true),
            KeyCode::Enter => {
                if let Some(picked) = list.picked().cloned() {
                    self.switch_session(&picked);
                }
            }
            KeyCode::Char(' ') if !ctrl => list.tick(),
            KeyCode::Char('a') if ctrl => list.tick_all(),
            KeyCode::Char('d') if ctrl => self.delete_picked(),
            _ => {}
        }
    }

    /// `Ctrl+D`：删勾了的全部、没勾的删选中的那一个；整批按第一次提示再按一次，第二次才删（Miyu「会话列表」第 2 条，
    /// `app/switch.rs` 的 `delete_picked`）。正在用的也能删（Miyu 2026-11-02 项目主人）：先切到剩下的里最近有动静的那个，
    /// 一个都不剩的开新会话，再删；它有一轮在跑的先说要打断。删好的提示照普通通知的颜色（Miyu `595e509f`）。
    fn delete_picked(&mut self) {
        let main = self.talk.main_session();
        let running = self.talk.transcript.running.is_some();
        let texts = self.config.text().sessions.clone();
        let Some(list) = self.boxes.sessions.as_mut() else {
            return;
        };
        let doomed = list.doomed();
        let mine = main.as_ref().is_some_and(|m| doomed.contains(m));
        if mine && running {
            self.hint(texts.delete_running, false);
            return;
        }
        let title = |list: &SessionList, id: &str| {
            let found = list.all.iter().find(|s| s.id.as_str() == id);
            found
                .filter(|s| !s.title.trim().is_empty())
                .map_or(texts.untitled.clone(), |s| s.title.clone())
        };
        let note = match doomed.as_slice() {
            [] => return,
            [one] => title(list, one),
            many => many.len().to_string(),
        };
        let one = doomed.len() == 1;
        if list.armed.as_ref() != Some(&doomed) {
            list.armed = Some(doomed);
            let ask = if one {
                texts.delete_again.replace("{title}", &note)
            } else {
                texts.delete_many.replace("{count}", &note)
            };
            self.hint(ask, false);
            return;
        }
        let next = list.next_after(&doomed).cloned();
        list.remove(&doomed);
        // 删的里有正在用的：先切走（切到最近的那个，都删光了开新会话），引擎换下它以后再删（命令照先后做）。
        if mine {
            match next {
                Some(next) => self.switch_session(&next),
                None => {
                    self.leave_child();
                    self.new_session(None);
                }
            }
        }
        // 手上那一份会话列表也当场拿掉（侧边栏「最近会话」、下次开框先照它画）。
        if let Some(listed) = &mut self.talk.recent.listed {
            listed.retain(|s| !doomed.iter().any(|d| d == s.id.as_str()));
        }
        for session in doomed {
            self.link.core.send(Command::Delete(session));
        }
        let done = if one {
            texts.deleted.replace("{title}", &note)
        } else {
            texts.deleted_many.replace("{count}", &note)
        };
        // 照普通通知的颜色，不用绿色（Miyu 发版前 check 12、32）。
        self.hint(done, false);
    }

    /// 鼠标在框上：悬停选中，点一下切过去；滚轮挪露出的那一段，选中的不动；落在框里别处（边框、段头）也算它的。交回归不归它
    /// （Miyu `app/switch.rs` 的 `sessions_mouse`）。
    pub(super) fn sessions_mouse(&mut self, mouse: MouseEvent, at: Position) -> bool {
        if self.boxes.panel != Some(Panel::Sessions) {
            return false;
        }
        let areas = self.screen.areas;
        if !areas.menu.contains(at) {
            return false;
        }
        // 滚轮挪露出的那一段，选中的不动（照输入历史列表）。
        let by = match mouse.kind {
            MouseEventKind::ScrollUp => -1,
            MouseEventKind::ScrollDown => 1,
            _ => 0,
        };
        let rows = self.config.layout.session_rows.max(1);
        if by != 0 {
            if let Some(list) = self.boxes.sessions.as_mut() {
                let count = list.matches().len();
                crate::menu::pin(list.selected, &mut list.pinned, count, rows, by);
            }
            return true;
        }
        let row = at.y.checked_sub(areas.menu_text.y).map(usize::from);
        let index = row.and_then(|row| self.screen.panel_rows.get(row).copied().flatten());
        let Some(index) = index.filter(|_| areas.menu_text.contains(at)) else {
            return true;
        };
        let Some(list) = self.boxes.sessions.as_mut() else {
            return true;
        };
        let count = list.matches().len();
        crate::menu::pin(list.selected, &mut list.pinned, count, rows, 0);
        list.selected = index;
        list.armed = None;
        if matches!(mouse.kind, MouseEventKind::Down(_))
            && let Some(picked) = list.picked().cloned()
        {
            self.switch_session(&picked);
        }
        true
    }

    /// 点了侧边栏「最近会话」的第 `row` 行（不算标题）：切过去；当前这个不动（Miyu「后台命令、子代理和侧边栏」第 7 条）。
    pub(super) fn recent_clicked(&mut self, row: u16) {
        let Some(id) = self.screen.recent.get(usize::from(row)).cloned() else {
            return;
        };
        let picked = self
            .talk
            .recent
            .listed
            .as_ref()
            .and_then(|all| all.iter().find(|s| s.id.as_str() == id).cloned());
        if let Some(picked) = picked {
            self.switch_session(&picked);
        }
    }

    /// 切到会话 `picked`：正在用的不动；别的交给引擎去开，开好了换（[`App::opened`]）。别的窗口开着的引擎开不了，照实说
    /// （`Update::OpenFailed` 的 `taken`）；列表里的「开着」不拿来先挡：这个窗口刚切走、还在关的也标着开着，引擎等它关完再开。
    fn switch_session(&mut self, picked: &Listed) {
        self.close_sessions();
        // 正在开的就是它（点了还没换上来又点一下）：等补完换上来，不再开一次（Miyu `d820a762`，2026-10-27 项目主人报侧边栏
        // 切会话闪一下空会话）。再开一次引擎答「在别的窗口开着」，当场把补了一半的画出来。
        if self.talk.opening.of(picked.id.as_str()) {
            return;
        }
        if self.talk.main_session().as_deref() == Some(picked.id.as_str()) {
            // 看着它派的子会话时点了它自己：回到它（Miyu 排期第 64 条）。
            self.leave_child();
            return;
        }
        if !self.reachable() {
            return;
        }
        self.open_session(picked.id.as_str().to_string());
    }

    /// 引擎开好了旧会话 `session`：换一份空的正文（同 `/new`），接着来的推送是它从头补发的。从别的会话切过来的：输入框里
    /// 没发的那句留给原来那个会话，换上来的放回它自己的（没有的空着；Miyu `636b2b8d`）。原来还没开会话的（`-c`、`-s` 起来时
    /// 等着开、新会话页上开的）框里的字不动：Miyu 起来时接着开的那条路不换框里的字，这里同一个函数，先打的字不能被开好那一下
    /// 冲掉（第二十一趟量尺 `order.py` 在模拟器里慢，开好以前就打了字，这样冲掉过）。
    fn opened(&mut self, session: String) {
        if let Some(was) = self.talk.main_session() {
            let typed = self.typing.input.draft();
            if typed == crate::input::Draft::default() {
                self.typing.drafts.remove(&was);
            } else {
                self.typing.drafts.insert(was, typed);
            }
            let own = self.typing.drafts.remove(&session).unwrap_or_default();
            self.typing.input.editor.set_draft(own);
        }
        self.reset_talk();
        // 它在哪个目录干活照列出来时记的（引擎照它开；不在了的照终端所在的目录），`@` 列表在那里找。
        self.talk.workdir = self
            .talk
            .recent
            .listed
            .iter()
            .flatten()
            .find(|listed| listed.id.as_str() == session)
            .map(|listed| std::path::PathBuf::from(&listed.cwd))
            .filter(|dir| dir.is_dir());
        self.talk.recent.stale = true;
        self.link.human = crate::human::Human::load(self.config.language.code());
        self.talk
            .transcript
            .update(Update::Ready(session), self.config.text());
    }

    /// `/new`，后面的字定开哪一档：不写的照配置的默认，`bare` 无功能，`full` 全部功能；写别的弹一句、不开。
    pub(super) fn new_session_with(&mut self, words: Option<&str>) {
        let bare = match words {
            None => None,
            Some("bare") => Some(true),
            Some("full") => Some(false),
            Some(_) => {
                let note = self.config.text().sessions.new_tier.clone();
                self.hint(note, false);
                return;
            }
        };
        self.new_session(bare);
    }
}

#[cfg(test)]
mod tests;
