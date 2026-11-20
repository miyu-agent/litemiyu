//! 程序的状态，和把终端事件、引擎的消息分给各块。按键在 `keys.rs`，鼠标在 `mouse.rs`。

mod agents;
mod asking;
mod boxes;
mod build;
mod checkup;
mod child;
mod colors;
mod compose;
mod deadline;
mod drawer;
mod effort;
mod export;
mod files_page;
mod frame;
mod help;
mod jobs;
mod jump;
mod keys;
mod link;
pub(crate) mod list_search;
mod looping;
mod media;
mod meter;
pub(crate) mod model_list;
mod models;
mod mouse;
mod paste;
mod peers;
mod redo;
mod rename;
mod rescue;
mod screen;
mod seen;
mod session;
mod sessions;
mod settings;
mod shell_send;
mod status;
mod takeback;
mod talk;
mod typing;
mod undo;
mod unread;
mod updates;
pub(crate) mod vim;
mod window;

pub use boxes::Panel;
pub use files_page::{FilesPage, Shown};
pub use frame::Lists;
pub use jobs::JobsView;
pub use screen::Screen;
pub use seen::Seen;
pub use sessions::{Opening, Recent};

use std::time::{Duration, Instant};

use crossterm::event::{Event, KeyCode, KeyEventKind};

use crate::commands::{self, Spec};
use crate::config::Config;
use crate::core::Core;
use crate::history::History;
use crate::human::Human;
use crate::input::{Action, Draft, InputBox, PasteRule};
use crate::menu::Menu;
use crate::tips::Tips;

/// 按下鼠标时落在哪一块：拖动、松开都归它，拖出了那一块也一样。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Grab {
    Menu,
    Input,
    Body,
}

/// 一条提示：到点自己消失，或者留到人下一次按键（被忽略的几样那一条，`checkup.rs`）。
#[derive(Debug)]
pub struct Notice {
    /// 显示的字。
    pub text: String,
    /// 接在最后一行后面、放不下时也整个留着的一截（`/rename` 重起没起成接那一次的错，同 miyu-tui `hint_keeping`）。
    pub tail: String,
    /// 什么时候消失；`None` 是不到点、留到人下一次按键（按下就收掉，再照常办那个键）。
    pub until: Option<Instant>,
    /// 是好消息（复制成了）：绿；别的提示暗。
    pub good: bool,
}

/// 最近发出去的一句，没发出去时撤回来用（「输入框」第 12、13 条）。
struct Unsent {
    /// 放回输入框的字：说的话、编辑上一句改过的；`/redo` 原样重来的没有。
    draft: Option<Draft>,
    /// 编辑上一句的，改之前的那句：放回去以后接着编辑。
    original: Option<Draft>,
    /// 重做先藏掉的那一轮：显示回来。
    turn: Option<u64>,
}

/// 整个程序的状态。
pub struct App {
    /// 界面上的字和布局的数值。
    pub config: Config,
    /// 开着的框（`boxes.rs`，蓝图 `app.md`）。
    pub boxes: boxes::Boxes,
    /// 输入框和它上面的列表、焦点、提示（`typing.rs`，蓝图 `app.md`）。
    pub typing: typing::Typing,
    /// 交给引擎的一头和给人看的字（`link.rs`，蓝图 `app.md`）。
    pub link: link::Link,
    /// 会话：正文、确认和提问（`talk.rs`，蓝图 `app.md`）。
    pub talk: talk::Talk,
    /// 要画的东西的账：排好的行（`media.rs`，蓝图 `app.md`）。
    pub media: media::Media,
    /// 上一帧画出来的：各块位置、视口、光标、行表、鼠标（`screen.rs`，蓝图 `app.md`）。
    pub screen: screen::Screen,
    /// 终端所在的目录，家目录写成 `~`：首页照它写。
    pub cwd: String,
    /// 该退出了。
    pub quit: bool,
    /// 开着的配置界面（`app/settings.rs`）：开着时终端的事件都归它，画的也是它。
    pub settings: Option<Box<crate::settings::Settings>>,
    /// 关了配置界面时还没写完的（`settings/writer.rs`）、写完要不要叫引擎换上新配置：写完了才叫（引擎照盘上的读）。
    pub writes: Option<(crate::settings::writer::Writer, bool)>,
    /// `/model` 开框时在后台拉模型列表（`models.rs`），记的「拉过」配置界面也用。
    pub lists: models::Lists,
    /// 按了 `Ctrl+Z`：主循环画完这一批，还原终端、挂起（`terminal::suspend`）。
    pub suspend: bool,
    /// 要让出终端开的编辑器（`compose.rs`、配置界面的提示词）：主循环画完这一批去开，退出了交回 [`App::edited`]。
    pub edit: Option<crate::editor::Edit>,
    /// 应急模式（`rescue.rs`，设计第五节第 16 条）：不是的没有。
    pub rescue: Option<Box<rescue::Rescue>>,
}

impl App {
    /// 这一下 Esc 归输入框（打断、清空）：列表（命令列表、输入历史列表）、面板、抽屉开着的，焦点在别处的，
    /// 有选区的，都先归它们（`13-终端界面.md` 第十节「由近及远」，`tui.md`「按键」Esc）。
    fn esc_for_input(&self, menu_open: bool) -> bool {
        !menu_open
            && !self.typing.history.open
            && self.boxes.panel.is_none()
            && !self.talk.drawers.open()
            && self.typing.input.editor.selection().is_none()
            && self.screen.view.select.is_none()
            && self.typing.focus == crate::focus::Focus::Input
    }

    /// 处理一个终端事件。`/jump` 回车以后在等（只留几屏时）：按键、粘贴先攒着，等完了照先后补办（`jump.rs` 的 `hold`）。
    pub fn handle(&mut self, event: Event) {
        if let Some(event) = self.hold(event) {
            self.handle_now(event);
        }
    }

    /// 办一个终端事件：[`App::handle`] 不攒的，攒着补办的。
    fn handle_now(&mut self, event: Event) {
        // 留到下一次按键的提示：按下（不算松开）就收掉，键照常办（办的时候弹了新提示的照弹）。
        if let Event::Key(key) = &event
            && key.kind != KeyEventKind::Release
            && self
                .typing
                .notice
                .as_ref()
                .is_some_and(|n| n.until.is_none())
        {
            self.typing.notice = None;
        }
        if self.settings.is_some() {
            self.settings_event(&event);
            return;
        }
        // 「文件修改」那一页开着：按键、鼠标都归它（`files_page.rs`）。
        if self.files_open() {
            self.files_event(&event);
            return;
        }
        let menu_open = self.menu_matches().is_some();
        let event = self.vim_keys(event, menu_open);
        let action = match event {
            // 松开键的不认，只认按下和按住。
            Event::Key(key) if key.kind == KeyEventKind::Release => Action::None,
            // 看着子会话、输入框空着：`Esc` 回主会话或者停掉它（「切进子会话」第 4 条，`child.rs`）。
            Event::Key(key)
                if key.code == KeyCode::Esc
                    && self.esc_for_input(menu_open)
                    && self.typing.input.editor.is_empty()
                    && self.child_esc() =>
            {
                Action::None
            }
            // 编辑上一句时一下就取消（「输入框」第 13 条）。
            Event::Key(key)
                if key.code == KeyCode::Esc
                    && self.typing.input.editing()
                    && self.esc_for_input(menu_open)
                    && self.talk.transcript.running.is_none() =>
            {
                self.typing.input.cancel_edit();
                Action::None
            }
            // Esc 由近及远（`13-终端界面.md` 第十节）：列表、选区归 `key`；在回答时（发出去了、还没开轮的也算）第一下
            // 只提示，一小会儿以内再按一下才打断。不清空输入框（2026-10-20 项目主人定）。
            Event::Key(key)
                if key.code == KeyCode::Esc
                    && self.esc_for_input(menu_open)
                    && (self.talk.transcript.running.is_some()
                        || self.talk.transcript.starting != crate::transcript::Starting::No) =>
            {
                self.esc();
                Action::None
            }
            Event::Key(key) => {
                // 人敲了字：引擎线程先起，发第一句话时不用再等（`docs/00-设计.md` 第七节「守红线要做的」第 1 条）。
                self.link.core.warm();
                self.key(key)
            }
            Event::Paste(text) => {
                self.paste(&text);
                Action::None
            }
            Event::Mouse(mouse) => self.mouse(mouse),
            _ => Action::None,
        };
        match action {
            Action::None => {}
            Action::Submit(text) => self.submit(text),
            Action::Copy(text) => self.copy(&text),
            Action::Open(path) => self.open_link(&path),
            Action::Quit => self.quit = true,
            Action::Suspend => self.suspend = true,
            Action::Compose if self.rescue_refuses() => {}
            Action::Compose => self.compose(),
            // Ctrl+C 分级：输入框空着时，在回答就打断（排着队的退回输入框），不在回答才提示用 Ctrl+D 退出。
            // 看着在跑的子会话：一下停掉它（「切进子会话」第 4 条）。
            Action::ExitHint if self.child_ctrl_c() => {}
            Action::ExitHint if self.talk.transcript.running.is_some() => self.interrupt(),
            Action::ExitHint => self.hint(self.config.text().exit_hint.clone(), false),
            Action::Cleared => self.hint(self.config.text().input_cleared.clone(), false),
        }
    }

    /// 在输入框左上方写一条提示，新的顶掉旧的。
    fn hint(&mut self, text: String, good: bool) {
        self.hint_keeping(text, String::new(), good);
    }

    /// 同 [`App::hint`]，不到点消失，留到人下一次按键（被忽略的几样，`checkup.rs`：只停两秒人会错过）。
    fn hint_until_key(&mut self, text: String) {
        self.typing.notice = Some(Notice {
            text,
            tail: String::new(),
            until: None,
            good: false,
        });
    }

    /// 同 [`App::hint`]，后面接着 `tail`：放不下时截前面的字，它整个留着（同 miyu-tui）。
    fn hint_keeping(&mut self, text: String, tail: String, good: bool) {
        // 好几行的多停一会儿：一行一份，最多五份。
        let lines = u32::try_from(text.lines().count().clamp(1, 5)).unwrap_or(1);
        let stay = Duration::from_millis(self.config.layout.notice_ms) * lines;
        self.typing.notice = Some(Notice {
            text,
            tail,
            until: Some(Instant::now() + stay),
            good,
        });
    }

    /// 到点了：压缩那一行的进度条追一下，抽屉的时限，收掉过期的提示。
    pub fn tick(&mut self) {
        // 压缩那一行的进度条追一下（`tui.md`「正文」第 9 条）。
        let layout = &self.config.layout;
        self.talk.transcript.climb(
            Instant::now(),
            layout.bar.width,
            &layout.compaction,
            &mut self.screen.rng,
        );
        self.drawer_tick();
        // 会话一时读不了：到点再试（`unread.rs`）。
        self.reread_due(Instant::now());
        // 等的时候攒着的按键扣满了：不等了，补办（`jump.rs`）。
        self.held_overdue(Instant::now());
        // 后台面板展开着在跑的命令：输出跟着读（`jobs.rs`）。
        self.advance_jobs();
        if let Some(open) = &mut self.settings {
            open.poll_probe();
            open.poll_lists();
            open.poll_catalog();
            open.poll_writes();
        }
        self.apply_saved();
        self.poll_closed_writes();
        self.poll_lists();
        if self
            .typing
            .notice
            .as_ref()
            .and_then(|n| n.until)
            .is_some_and(|until| until <= Instant::now())
        {
            self.typing.notice = None;
        }
    }

    /// 列表开着时筛出来的命令；没开是 `None`。每次按输入框里现在的字重新筛，顺手定开不开。
    pub fn menu_matches(&mut self) -> Option<Vec<Spec>> {
        // 翻输入历史翻出来的命令还没改过：不弹，`↑` `↓` 接着翻（蓝图「按键」`↑`、`↓`）。
        if self.typing.input.recalled() {
            return None;
        }
        let text = self.typing.input.editor.text();
        let typed = commands::menu_typed(text);
        let matches: Vec<Spec> = typed
            .map(|t| {
                self.config
                    .commands
                    .filter(t)
                    .into_iter()
                    .filter(|spec| self.offered(spec))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        self.typing
            .menu
            .sync(text, typed, matches.len())
            .then_some(matches)
    }
}
