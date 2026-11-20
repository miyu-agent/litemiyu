//! 焦点往下走和子代理状态行的按键、鼠标（Miyu 终端 `focus.rs`、`app/jobs.rs` 的 `jobs_key` 后半段，蓝图 `tui.md`「后台命令、
//! 子代理和侧边栏」第 2、6 条、「切进子会话」第 1、4 条）：输入框空着时按 `↓`，焦点先停在底栏的后台按钮上（有后台命令时），
//! `Enter` 才开后台面板，再 `↓` 到子代理状态行；`↑` 往回走，`←` `→` 在按钮之间换；状态行里 `Enter` 切进那个子代理的会话
//! （主会话那一行回主会话），焦点留在这一行；`x` 停掉选中的子代理；`Esc` 回输入框；别的键焦点回输入框，字照打进去。点一行同
//! `Enter`。
//!
//! 窄窗口没有侧边栏，从这里进子会话；宽窗口两处都能进（同 Miyu）。

use crossterm::event::{KeyCode, KeyEvent, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Position;

use super::App;
use crate::core::Command;
use crate::focus::{Button, Focus, Stops};

impl App {
    /// 按键先归框下面那一行的按钮、子代理状态行（Miyu `jobs_key`，面板那一半在 `keys.rs` 分给各个框）；归了它们返回 `true`。
    /// 输入框空着、没开列表时按 `↓`，焦点往下走。
    pub(super) fn focus_key(&mut self, key: KeyEvent, menu_open: bool) -> bool {
        let buttons = self.seen().footer_buttons();
        let stops = Stops {
            buttons: &buttons,
            agents: self.seen().agent_stops(),
        };
        let focus = self.typing.focus.settle(stops);
        self.typing.focus = match (focus, key.code) {
            (Focus::Input, KeyCode::Down) if self.typing.input.editor.is_empty() && !menu_open => {
                focus.down(stops)
            }
            (Focus::Input, _) => return false,
            (_, KeyCode::Down) => focus.down(stops),
            (_, KeyCode::Up) => focus.up(stops),
            (_, KeyCode::Left) => focus.side(stops, false),
            (_, KeyCode::Right) => focus.side(stops, true),
            (_, KeyCode::Esc) => Focus::Input,
            (Focus::Footer(Button::Background), KeyCode::Enter) => {
                self.open_jobs();
                focus
            }
            // 第 0 行回主会话，别的切进那个子代理的会话（「切进子会话」第 1、4 条）；焦点留在这一行
            // （2026-09-30 项目主人：原来切完回输入框）。
            (Focus::Agent(at), KeyCode::Enter) => {
                self.agent_row_picked(at);
                focus
            }
            // 选中的子代理按 x：停掉它（主会话那一行、报完了的不管）。
            (Focus::Agent(at), KeyCode::Char('x')) if at > 0 => {
                let listed = self.talk.jobs.listed(self.talk.viewing());
                let job = listed
                    .get(at - 1)
                    .filter(|j| j.ended.is_none())
                    .map(|j| j.id.clone());
                if let Some(job) = job {
                    self.link.core.send(Command::StopJob(job));
                }
                focus
            }
            // 别的键：焦点回输入框，字照打进去。
            _ => {
                self.typing.focus = Focus::Input;
                return false;
            }
        };
        focus != Focus::Input || self.typing.focus != Focus::Input
    }

    /// 第 `at` 行（0 是主会话）：回主会话，或者切进那个子代理的会话。
    fn agent_row_picked(&mut self, at: usize) {
        if at == 0 {
            return self.leave_child();
        }
        let session = self
            .talk
            .jobs
            .listed(self.talk.viewing())
            .get(at - 1)
            .and_then(|j| j.session.clone());
        if let Some(session) = session {
            self.enter_child(session);
        }
    }

    /// 状态行上的鼠标：点一行同在那一行按 `Enter`，焦点不挪过来。交回归不归它。
    pub(super) fn agents_mouse(&mut self, mouse: MouseEvent, at: Position) -> bool {
        if !self.screen.areas.agent_rows.contains(at) {
            return false;
        }
        if mouse.kind == MouseEventKind::Down(MouseButton::Left) {
            let row = usize::from(at.y - self.screen.areas.agent_rows.y);
            match self.screen.agent_rows.get(row).cloned() {
                Some(None) => self.leave_child(),
                Some(Some(session)) if !session.is_empty() => self.enter_child(session),
                _ => {}
            }
        }
        true
    }

    /// 状态行有在跑的子代理：每秒醒一次，用时跟着走。别的时候不醒。
    pub(super) fn agents_deadline(&self) -> Option<std::time::Instant> {
        let running = self.talk.jobs.agents().any(|j| j.ended.is_none());
        (running && !self.talk.blank())
            .then(|| std::time::Instant::now() + std::time::Duration::from_secs(1))
    }
}

#[cfg(test)]
mod tests;
