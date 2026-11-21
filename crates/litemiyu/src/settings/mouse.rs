//! 配置界面的鼠标（照 Miyu 终端 `settings/mouse.rs`，蓝图「配置页」第 6、15 条）：点主菜单的一项进去；「供应商和模型」点分页
//! 换页，点栏里的一行选中、点选中的那行或双击开窗；点悬浮窗里的行（编辑窗的格子、选模型的窗、添加供应商的目录）选中、再点或
//! 双击等于 `Enter`；点按钮就是按下去。滚轮等于 `↑` `↓`（窗开着的滚窗里的）。
//!
//! 和 Miyu 一样不做的：照清单画的几页、MCP 页、技能页的行点不动（只滚轮挪）；选择窗、勾选窗的行点不动（Miyu 记了点得到的
//! 地方，`click` 里只认选模型的窗和目录窗）；点窗外不关窗；第一次运行的开头那几屏不收点（Miyu 的引导只记指针）。
//!
//! 点得到哪画的时候记（[`Hits`]，每帧先清掉）；一处叠着几样的照后画的（窗盖在页上面，点窗里的不落到底下那一行）。

use std::cell::RefCell;
use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};

use super::models::Popup as ModelPopup;
use super::models::forms::Focus;
use super::models::nav::{Col, PAGES};
use super::{Outcome, Popup, Settings, View, Window};

/// 两下点在同一格、隔得不超过这么久算双击（同 Miyu `layout.json` 的 `double_click_ms` 出厂值）。
pub const DOUBLE_CLICK: Duration = Duration::from_millis(400);

/// 点得到的一处。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    /// 主菜单的第几项。
    Menu(usize),
    /// 「供应商和模型」的第几个分页。
    Tab(usize),
    /// 一栏的第几行。
    Row(Col, usize),
    /// 编辑窗的第几格（`Form::rows` 的下标）。
    FormRow(usize),
    /// 选模型的窗、添加供应商的目录窗的第几行。
    PickRow(usize),
    /// 悬浮窗的第几个按钮。
    Button(usize),
}

/// 这一帧点得到的地方：画的时候记（画只拿得到 `&Settings`），点的时候照它找。另记着悬浮窗里从第几行露起（Miyu
/// `popup_top`）：画窗时照上一帧的接着算、记下这一帧的，没有窗开着时归零（`ui/settings/mod.rs`）。
#[derive(Debug, Default)]
pub struct Hits(RefCell<Vec<(Rect, Hit)>>, std::cell::Cell<usize>);

impl Hits {
    /// 悬浮窗从第几行露起记在哪（`ui/settings/floating.rs` 照它接着露）。
    pub fn top_cell(&self) -> &std::cell::Cell<usize> {
        &self.1
    }

    /// 记下这一帧悬浮窗从第几行露起；没有窗开着时记 0（下一个窗从头露起）。
    pub fn set_top(&self, top: usize) {
        self.1.set(top);
    }

    /// 新的一帧：清掉上一帧的。
    pub fn clear(&self) {
        self.0.borrow_mut().clear();
    }

    /// 记下 `rect` 点得到 `hit`。
    pub fn push(&self, rect: Rect, hit: Hit) {
        self.0.borrow_mut().push((rect, hit));
    }

    /// 落在 `at` 上的那一处：后画的盖在先画的上面，照后画的。
    pub fn at(&self, at: Position) -> Option<Hit> {
        self.0
            .borrow()
            .iter()
            .rev()
            .find(|(rect, _)| rect.contains(at))
            .map(|(_, hit)| *hit)
    }

    /// 记下的第一处 `hit` 在哪（测试照它点）。
    #[cfg(test)]
    pub fn find(&self, hit: Hit) -> Option<Rect> {
        self.0
            .borrow()
            .iter()
            .find(|(_, h)| *h == hit)
            .map(|(rect, _)| *rect)
    }
}

impl Settings {
    /// 一个鼠标事件：左键按下照点的那一处办，滚轮等于方向键，别的不管。
    pub(super) fn mouse(&mut self, mouse: &MouseEvent) -> Outcome {
        if self.opening.is_some() {
            return Outcome::Stay;
        }
        let arrow = |code| KeyEvent::new(code, KeyModifiers::NONE);
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                self.click(Position::new(mouse.column, mouse.row), Instant::now())
            }
            MouseEventKind::ScrollDown => self.key(arrow(KeyCode::Down)),
            MouseEventKind::ScrollUp => self.key(arrow(KeyCode::Up)),
            _ => Outcome::Stay,
        }
    }

    /// 左键点了 `at`（`now` 是什么时候点的）：上一下点在同一格、隔得不到 [`DOUBLE_CLICK`] 的算双击。
    pub fn click(&mut self, at: Position, now: Instant) -> Outcome {
        let double = self
            .last_click
            .replace((at, now))
            .is_some_and(|(was, then)| was == at && now.duration_since(then) <= DOUBLE_CLICK);
        let Some(hit) = self.hits.at(at) else {
            return Outcome::Stay;
        };
        let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        let open = self.popup.is_some() || self.models.as_ref().is_some_and(|m| m.popup.is_some());
        match hit {
            Hit::Menu(i) if !open && matches!(self.view, View::Menu(_)) => {
                self.view = View::Menu(i);
                return self.key(enter);
            }
            Hit::Tab(i) if !open => {
                if let Some(models) = &mut self.models {
                    models.nav.turn(PAGES[i.min(PAGES.len() - 1)]);
                    models.nav.col = 0;
                }
            }
            Hit::Row(col, row) if !open => {
                let Some(models) = &mut self.models else {
                    return Outcome::Stay;
                };
                let cols = models.nav.cols(&models.data);
                let again =
                    models.nav.focus(&models.data) == col && models.nav.selected(col) == row;
                if let Some(at) = cols.iter().position(|c| *c == col) {
                    models.nav.col = at;
                }
                models.nav.pick(col, row);
                if again || double {
                    return self.key(enter);
                }
            }
            Hit::FormRow(i) => {
                if let Some(ModelPopup::Form(form)) =
                    self.models.as_mut().and_then(|m| m.popup.as_mut())
                {
                    let again = form.focus == Focus::Row(i);
                    form.focus = Focus::Row(i);
                    if again || double {
                        return self.key(enter);
                    }
                }
            }
            Hit::PickRow(i) => {
                let sel = match self.models.as_mut().and_then(|m| m.popup.as_mut()) {
                    Some(ModelPopup::Pick(pick)) => &mut pick.sel,
                    Some(ModelPopup::Catalog(pick)) => &mut pick.sel,
                    _ => return Outcome::Stay,
                };
                let again = *sel == i;
                *sel = i;
                if again || double {
                    return self.key(enter);
                }
            }
            Hit::Button(b) => {
                if let Some(Popup {
                    window: Window::Confirm(confirm),
                    ..
                }) = &mut self.popup
                {
                    confirm.at = b;
                    return self.key(enter);
                }
                match self.models.as_mut().and_then(|m| m.popup.as_mut()) {
                    Some(ModelPopup::Form(form)) => form.focus = Focus::Button(b),
                    Some(ModelPopup::Confirm(confirm)) => confirm.sel = b,
                    _ => return Outcome::Stay,
                }
                return self.key(enter);
            }
            _ => {}
        }
        Outcome::Stay
    }
}
