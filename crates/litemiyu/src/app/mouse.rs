//! 鼠标：按下时落在哪一块，拖动、松开都归它；点开收起时间线；命令列表的悬停和点击（`tui.md`「鼠标」）。

use crossterm::event::{MouseEvent, MouseEventKind};
use ratatui::layout::Position;

use super::{App, Grab};
use crate::body_view::BodyAction;
use crate::commands::Spec;
use crate::input::Action;
use crate::ui::rows::Target;

impl App {
    /// 终端收回来了（挂起回来、编辑器退出了）：走的时候 `terminal.rs` 关了终端的像素坐标，这边记的也记成关着（不然抓着滚动条
    /// 按 `Ctrl+Z`、`Ctrl+G` 回来，这边当开着、终端照格子报，审计 #067）；一格多少像素重新问过的是 `cell`（走的时候可能缩放了字体）。
    pub fn terminal_back(&mut self, cell: Option<(u32, u32)>) {
        // 终端那边已经关了，不用再写。
        self.screen
            .pixel
            .reset(&mut std::io::sink())
            .unwrap_or_default();
        self.screen.pixel.cell = cell;
    }

    /// 鼠标：按下时看落在哪一块，之后的拖动、松开都归那一块；悬停、滚轮照位置分（Miyu 终端 `app/mouse.rs::mouse`）。
    pub(super) fn mouse(&mut self, mouse: MouseEvent) -> Action {
        // 拖滚动条时终端按像素报（`pixel_mouse.rs`）：先换回格子，带小数的第几行交给正文。
        let (mouse, pixel_y) = self.screen.pixel.as_cells(mouse);
        self.screen.view.pixel_y = pixel_y;
        let action = self.mouse_cells(mouse);
        self.screen.view.pixel_y = None;
        // 抓着滚动条就开像素坐标，松开就关（`main.rs` 每帧照它开关）。
        self.screen.pixel.want = self.screen.view.bar_held();
        action
    }

    fn mouse_cells(&mut self, mouse: MouseEvent) -> Action {
        let at = Position::new(mouse.column, mouse.row);
        self.screen.pointer = Some(at);
        // 已抓住的拖选和松开始终归起点区域，经过侧栏、按钮或抽屉也不被截走。
        let captured = self.screen.grab.is_some()
            && matches!(mouse.kind, MouseEventKind::Drag(_) | MouseEventKind::Up(_));
        // 滚轮在帮助框上：滚帮助框。
        if !captured && self.help_mouse(mouse, at) {
            return Action::None;
        }
        // `/model`、`/effort` 框里的一行（`models.rs`、`effort.rs`）。
        if !captured && (self.models_mouse(mouse, at) || self.effort_mouse(mouse, at)) {
            return Action::None;
        }
        // 会话列表里的一行（`sessions.rs`）。
        if !captured && self.sessions_mouse(mouse, at) {
            return Action::None;
        }
        // 侧边栏里按下、拖、松开：拖了是选字，只点不拖的照点的来（Miyu 在 `jobs_mouse` 里、会话列表那几个框后面）。
        if !captured && self.sidebar_mouse(mouse, at) {
            return Action::None;
        }
        // 底栏的后台按钮、后台面板里的一条（`jobs.rs`）。
        if !captured && self.jobs_mouse(mouse, at) {
            return Action::None;
        }
        // 子代理状态行的一行：点了切进那个子会话，点「主会话」回去（`agents.rs`）。
        if !captured && self.agents_mouse(mouse, at) {
            return Action::None;
        }
        let press = matches!(mouse.kind, MouseEventKind::Down(_));
        // 滚轮在侧边栏上：滚侧边栏，一格三行同正文（Miyu 排期第 14 条；滚过头的画的时候限住）。
        if !captured && self.screen.areas.sidebar.contains(at) {
            let top = &mut self.screen.side_top;
            match mouse.kind {
                MouseEventKind::ScrollDown => *top = top.saturating_add(crate::ui::sidebar::WHEEL),
                MouseEventKind::ScrollUp => *top = top.saturating_sub(crate::ui::sidebar::WHEEL),
                _ => {}
            }
        }
        // 侧边栏的短编号：点了复制完整的会话编号，在松开时（`sidebar_mouse`）；这里只是悬停。
        if !captured && self.screen.areas.session_id.contains(at) {
            return Action::None;
        }
        // 用量那一格卡住时的「压缩」：同 `/compact`（「框下面那一行」用量那一格）。
        if !captured && self.screen.areas.compact.contains(at) {
            if press {
                self.compact(None);
            }
            return Action::None;
        }
        // 用量那一格别处：开 `/status`（Miyu 开 `/context`；2026-11-14 项目主人：明细不做，做一个 `/status`）。
        if !captured && self.screen.areas.usage_cell.contains(at) {
            if press {
                self.open_status();
            }
            return Action::None;
        }
        // 排着队的话：点哪条退回哪条，字放回输入框，她这一轮照跑（2026-10-22 项目主人：「不如直接点击那条消息撤回」）。
        if !captured && self.screen.areas.queued.contains(at) {
            if matches!(mouse.kind, MouseEventKind::Down(_)) {
                let row = usize::from(at.y - self.screen.areas.queued.y);
                self.withdraw(row);
            }
            return Action::None;
        }
        // 正文顶上钉着的你说的话：点一下跳回那句话（`ui/pinned.rs`）。
        if !captured && self.screen.areas.pinned.contains(at) {
            if matches!(mouse.kind, MouseEventKind::Down(_))
                && let Some(id) = self.screen.pinned
            {
                self.jump_to(id);
            }
            return Action::None;
        }
        // 回到底部的按钮（`ui/bottom_button.rs`）。
        if !captured && self.screen.areas.bottom.contains(at) {
            if matches!(mouse.kind, MouseEventKind::Down(_)) {
                self.screen.view.follow();
            }
            return Action::None;
        }
        // 抽屉开着：悬停选中那一项，点一下等于 `Enter`（`tui.md`「确认和提问的抽屉」第 4 条）。
        if !captured && self.talk.drawers.open() && self.screen.areas.text.contains(at) {
            // 滚轮滚要跑的命令那一块：抽屉里只有它会滚（`ui/drawer/command.rs`）。
            if let MouseEventKind::ScrollUp | MouseEventKind::ScrollDown = mouse.kind {
                if let Some(d) = self.talk.drawers.current.as_mut() {
                    d.scroll_command(mouse.kind == MouseEventKind::ScrollDown);
                }
                return Action::None;
            }
            let row = usize::from(at.y - self.screen.areas.text.y);
            if let Some(index) = self.screen.drawer_rows.get(row).copied().flatten() {
                match mouse.kind {
                    MouseEventKind::Moved => {
                        if let Some(d) = self.talk.drawers.current.as_mut() {
                            d.cursor[d.tab] = index;
                        }
                    }
                    MouseEventKind::Down(_) => self.drawer_click(index),
                    _ => {}
                }
            }
            return Action::None;
        }
        let under = if self.screen.areas.menu.contains(at) {
            Some(Grab::Menu)
        } else if self.screen.areas.frame.contains(at) {
            Some(Grab::Input)
        } else if self.screen.areas.body.contains(at) || self.screen.view.on_bar_column(at.x, at.y)
        {
            Some(Grab::Body)
        } else {
            None
        };
        let owner = match mouse.kind {
            MouseEventKind::Down(_) => {
                self.screen.grab = under;
                under
            }
            MouseEventKind::Drag(_) => self.screen.grab,
            MouseEventKind::Up(_) => self.screen.grab.take(),
            _ => under,
        };
        if owner != Some(Grab::Body) {
            self.screen.view.hover = None;
            self.screen.view.bar_hover = false;
            // 链接的悬停也放掉：挪到正文左边的空白、输入框这些地方时，下划线和手形指针不留着（2026-10-13 验收 T-225）。
            self.screen.view.hover_link = None;
        }
        if owner != Some(Grab::Input) {
            self.typing.input.unhover();
        }
        match owner {
            Some(Grab::Menu) if self.typing.history.open => self.history_mouse(mouse),
            Some(Grab::Menu) => match self.menu_matches() {
                Some(matches) => self.menu_mouse(mouse, &matches),
                None => Action::None,
            },
            Some(Grab::Input) => self.typing.input.mouse(mouse, under == Some(Grab::Input)),
            Some(Grab::Body) => {
                match self.screen.view.mouse(mouse) {
                    BodyAction::None => {}
                    BodyAction::Copy(text) => return Action::Copy(text),
                    BodyAction::Toggle(target) => self.toggle(target),
                    BodyAction::Open(url) => self.open_link(&url),
                }
                Action::None
            }
            None => Action::None,
        }
    }

    /// 侧边栏里按下、拖、松开（Miyu 终端 `app/jobs.rs::sidebar_mouse`）：拖了是选字，松开复制、留着选区；只点不拖的照点的来
    /// （点短编号复制完整编号、点「最近会话」的一行切过去、点「跳转消息」的一行跳过去）；在别处按下，侧边栏的选区作废
    /// （`tui.md`「后台命令、子代理和侧边栏」第 7 条）。归了它的交回 `true`。
    fn sidebar_mouse(&mut self, mouse: MouseEvent, at: Position) -> bool {
        let areas = self.screen.areas;
        let inside = areas.sidebar.contains(at);
        match mouse.kind {
            MouseEventKind::Down(_) if inside => {
                self.screen.side_select.press(at.x, at.y);
                true
            }
            MouseEventKind::Down(_) => {
                self.screen.side_select.clear();
                false
            }
            MouseEventKind::Drag(_) if self.screen.side_select.pressing() => {
                self.screen.side_select.drag(at.x, at.y);
                true
            }
            MouseEventKind::Up(_) if self.screen.side_select.pressing() => {
                if self.screen.side_select.release() {
                    if areas.session_id.contains(at)
                        && let Some(session) = self.talk.transcript.session.clone()
                    {
                        self.copy(&session);
                    } else if areas.recent.contains(at) {
                        self.recent_clicked(at.y - areas.recent.y);
                    } else if areas.files.contains(at) {
                        self.files_clicked(at.y - areas.files.y);
                    } else if areas.jumps.contains(at) {
                        self.jump_clicked(at.y - areas.jumps.y);
                    }
                } else {
                    let text = self.screen.side_select.text();
                    if !text.is_empty() {
                        self.copy(&text);
                    }
                }
                true
            }
            _ => false,
        }
    }

    /// 点了链接：网址、本机的文件、目录用系统的打开方式开；开不了的提示一句（「她的回答：Markdown」第 10 条）。
    pub(super) fn open_link(&mut self, url: &str) {
        use crate::open::{Env, Launched, OpenError, launch};
        // 本机的文件、目录：先交系统给这种文件定的默认程序，没有的交浏览器，都没有的说一声（同 miyu-tui，2026-10-24 项目
        // 主人：点 `.html` 进了编辑器，「和 show 用类似点击打开策略」）。种类照文件本身问，问不到的当不认得的文件。
        if let Some(file) = crate::open::local(url) {
            if launch(&file, "application/octet-stream", &Env::current()) == Launched::Nothing {
                let note = self
                    .config
                    .text()
                    .open
                    .no_program
                    .replace("{path}", &file.display().to_string());
                self.hint(note, false);
            }
            return;
        }
        let words = &self.config.text().open;
        let note = match crate::open::open(url) {
            Ok(()) => return,
            Err(OpenError::Scheme) => words.refused.replace("{url}", url),
            Err(OpenError::Missing) => words.missing.replace("{path}", url),
            Err(OpenError::Spawn(e)) => words.failed.replace("{reason}", &e.to_string()),
        };
        self.hint(note, false);
    }

    /// 指针停在能点的东西上：指针变手（`pointer.rs`；照 Miyu `app/jobs.rs` 的 `pointing` 一项项对过，界面线第二十一趟）。
    pub fn pointing(&self) -> bool {
        let over = |r: ratatui::layout::Rect| self.screen.pointer.is_some_and(|p| r.contains(p));
        // 子代理状态行：子代理那几行变手，「主会话」那一行（第 0 行）不变（同 Miyu `agents_hover >= 2`）。
        let rows = self.screen.areas.agent_rows;
        let agent = self
            .screen
            .pointer
            .is_some_and(|p| rows.contains(p) && p.y > rows.y);
        // 正文里能点的（收起那一行、一步、能点开的你说的话）也算（`tui.md`「鼠标」）。
        self.screen.view.hover_link.is_some()
            || self.screen.view.hover.is_some()
            // 输入框里的文件块（Miyu `hovered_attachment`）。
            || self.typing.input.hovered_attachment().is_some()
            || over(self.screen.areas.compact)
            || over(self.screen.areas.usage_cell)
            || over(self.screen.areas.bottom)
            // 底栏「N 个后台命令」按钮（Miyu `over(areas.button)`）。
            || over(self.screen.areas.jobs_button)
            || over(self.screen.areas.session_id)
            || over(self.screen.areas.jumps)
            || over(self.screen.areas.pinned)
            || over(self.screen.areas.files)
            || over(self.screen.areas.recent)
            || agent
            // 正文的滚动条（`body_view.rs`）。
            || self.screen.view.bar_hover
            // 排着队的话点了能撤回（`takeback.rs`）。
            || over(self.screen.areas.queued)
    }

    /// 点开、收起时间线的一段或一步。
    pub(super) fn toggle(&mut self, target: Target) {
        // 人点过的那一段不放（红线 5）：读回来的照日志补发，记不得点过什么。
        let at = match target {
            Target::Segment(i) | Target::Entry(i) | Target::Step(i, _) | Target::Details(i, _) => i,
        };
        if let Some(piece) = self.talk.transcript.entries.get(at).map(|e| e.piece) {
            self.talk.transcript.pin(piece as usize);
        }
        let entries = &mut self.talk.transcript.entries;
        match target {
            Target::Segment(i) => {
                if let Some(segment) = entries.get_mut(i).and_then(|e| e.segment.as_mut()) {
                    segment.open = Some(!segment.expanded(self.config.timeline.fold));
                }
            }
            Target::Entry(i) => {
                if let Some(entry) = entries.get_mut(i) {
                    entry.open = !entry.open;
                }
            }
            Target::Step(i, j) => {
                let segment = entries.get_mut(i).and_then(|e| e.segment.as_mut());
                if let Some(step) = segment.and_then(|s| s.steps.get_mut(j)) {
                    step.open = Some(!step.opened(&self.config.timeline));
                }
            }
            // 回答里的 `<details>`：点过的记在这一条上，再点一下去掉（和写的 `open` 反过来）。
            Target::Details(i, k) => {
                if let Some(entry) = entries.get_mut(i) {
                    match entry.details.iter().position(|&d| d == k) {
                        Some(at) => {
                            entry.details.remove(at);
                        }
                        None => entry.details.push(k),
                    }
                }
            }
        }
    }

    /// 输入历史列表上的鼠标：悬停选中那一条，点一下等于 `Enter`（`tui.md`「输入历史列表」第 5 条）。
    fn history_mouse(&mut self, mouse: MouseEvent) -> Action {
        use crossterm::event::MouseButton;
        // 下面要改列表的状态：先拷一份列的东西。
        let list = self.list_items().0.into_owned();
        let found = self.typing.history.matches(&list);
        let width = self.screen.areas.menu_text.width;
        let now = std::time::SystemTime::now();
        // 和画出来的同一份：放进列表那块的高度（「窗口小的时候」第 1 条）。
        let max = usize::from(self.screen.areas.menu_text.height);
        // 悬停钉住、滚轮滚（最新的贴底：滚轮往上是更早的），照钉住以后画出来的行认指针下是哪一条。
        let Some(by) = list_step(mouse.kind, true) else {
            return Action::None;
        };
        // 列表那一段几条照画的同一份算（全部历史、窗口，Miyu「输入历史列表」第 1 条「高度」）。
        let (columns, screen) = self.screen.pixel.size;
        let room = crate::ui::HistoryRoom {
            width,
            max,
            screen,
            columns,
            total: list.len(),
        };
        let side = crate::ui::history_side(columns, width);
        let shown = crate::ui::history_list_rows(&self.config.layout, list.len(), side, screen);
        let history = &mut self.typing.history;
        crate::menu::pin(
            history.selected,
            &mut history.pinned,
            found.len(),
            shown,
            by,
        );
        let laid = crate::ui::history_lines(&self.typing.history, &found, room, &self.config, now);
        let at = crate::ui::history_index_at(
            self.screen.areas.menu_text,
            &laid,
            mouse.column,
            mouse.row,
        );
        if let Some(index) = at {
            self.typing.history.selected = index;
            if mouse.kind == MouseEventKind::Down(MouseButton::Left) {
                self.pick_history();
            }
        }
        Action::None
    }

    /// 列表上的鼠标（「斜杠命令列表」第 3 条）：悬停高亮指针下那一条、列表不动，滚轮滚，点一下就执行。
    pub(super) fn menu_mouse(&mut self, mouse: MouseEvent, matches: &[Spec]) -> Action {
        use crossterm::event::{MouseButton, MouseEventKind};
        // 露了几条照画出来的：框里放字的那一块有几行（「窗口小的时候」第 1 条）。
        let rows = crate::ui::menu_rows(
            self.config.layout.menu_rows,
            self.screen.areas.menu_text.height,
        );
        let count = matches.len();
        let Some(by) = list_step(mouse.kind, false) else {
            return Action::None;
        };
        crate::menu::pin(
            self.typing.menu.selected,
            &mut self.typing.menu.pinned,
            count,
            rows,
            by,
        );
        let top = crate::menu::top(
            self.typing.menu.selected,
            self.typing.menu.pinned,
            count,
            rows,
        );
        let at = crate::ui::menu_index_at(self.screen.areas.menu_text, count, top, rows, mouse.row);
        let Some((index, spec)) = at.and_then(|i| Some((i, matches.get(i)?))) else {
            return Action::None;
        };
        self.typing.menu.selected = index;
        if mouse.kind == MouseEventKind::Down(MouseButton::Left) {
            let spec = spec.clone();
            self.typing.input.editor.take();
            self.run(&spec, None);
        }
        Action::None
    }
}

/// 列表上的鼠标这一下要把露的那一段挪几条：悬停、按下左键钉住不挪（0），滚轮一格一条；别的不管。`upward`：列表
/// 从下往上排（输入历史列表，最新的贴底），滚轮往上是往后翻。
pub(super) fn list_step(kind: MouseEventKind, upward: bool) -> Option<isize> {
    use crossterm::event::MouseButton;
    let sign = if upward { -1 } else { 1 };
    match kind {
        MouseEventKind::Moved | MouseEventKind::Down(MouseButton::Left) => Some(0),
        MouseEventKind::ScrollDown => Some(sign),
        MouseEventKind::ScrollUp => Some(-sign),
        _ => None,
    }
}
