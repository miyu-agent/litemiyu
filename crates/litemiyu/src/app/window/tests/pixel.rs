//! 拖滚动条按像素（对照表 A40，Miyu 终端 `pixel_mouse.rs`、`app/mouse.rs::mouse`）：抓住滑块开终端的像素坐标、松开关；开着时
//! 终端报的像素换回格子，带小数的第几行交给滚动条，拖半格也挪得动。

use crossterm::event::{KeyModifiers, MouseButton, MouseEvent, MouseEventKind};

use super::{Ui, session};

fn mouse(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
    MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    }
}

/// 照主循环画完一帧那样开关像素坐标，交回写给终端的。
fn synced(ui: &mut Ui) -> String {
    let mut out = Vec::new();
    ui.app.screen.pixel.sync(&mut out).expect("写得进");
    String::from_utf8(out).expect("是字")
}

#[test]
fn holding_the_scrollbar_turns_pixels_on_and_half_a_cell_moves_the_text() {
    let mut ui = Ui::open(&session(90), usize::MAX, 72, 20);
    // 一格 10×20 像素（起来时问终端的窗口大小知道的）。
    ui.app.screen.pixel.cell = Some((10, 20));
    ui.draw();
    let x = ui.app.screen.view.bar_column();
    let bar = ui.app.screen.view.bar().expect("放不下，有滚动条");
    let area = ui.app.screen.areas.body;
    let thumb = area.y + u16::try_from(bar.start.floor() as usize).expect("在屏幕里");
    ui.app.mouse(mouse(MouseEventKind::Moved, x, thumb));
    ui.app
        .mouse(mouse(MouseEventKind::Down(MouseButton::Left), x, thumb));
    assert!(ui.app.screen.pixel.want, "抓住滑块要像素坐标");
    assert_eq!(synced(&mut ui), "\x1b[?1016h");
    ui.draw();
    let before = ui.app.screen.view.first;
    // 终端照像素报：往上拖半格（10 像素）。认出是像素（坐标超出屏幕）以后换回格子，滚动条照带小数的行挪。
    let px = |cell: u16, size: u16, part: u16| cell * size + part;
    ui.app.mouse(mouse(
        MouseEventKind::Drag(MouseButton::Left),
        px(x, 10, 5),
        px(thumb, 20, 0).saturating_sub(10),
    ));
    ui.draw();
    let after = ui.app.screen.view.first;
    assert!(after < before, "拖半格也往上挪了：{before} → {after}");
    // 再往上拖到第一格：一样照像素认。
    ui.app.mouse(mouse(
        MouseEventKind::Drag(MouseButton::Left),
        px(x, 10, 5),
        px(area.y, 20, 0),
    ));
    ui.draw();
    assert!(ui.app.screen.view.first < after, "接着照像素挪");
    ui.app.mouse(mouse(
        MouseEventKind::Up(MouseButton::Left),
        px(x, 10, 5),
        px(area.y, 20, 0),
    ));
    assert!(!ui.app.screen.pixel.want, "松开不要了");
    assert_eq!(synced(&mut ui), "\x1b[?1016l");
}

#[test]
fn without_a_known_cell_size_pixels_stay_off() {
    let mut ui = Ui::open(&session(90), usize::MAX, 72, 20);
    ui.app.screen.pixel.cell = None;
    ui.draw();
    let x = ui.app.screen.view.bar_column();
    let bar = ui.app.screen.view.bar().expect("有滚动条");
    let thumb =
        ui.app.screen.areas.body.y + u16::try_from(bar.start.floor() as usize).expect("在屏幕里");
    ui.app.mouse(mouse(MouseEventKind::Moved, x, thumb));
    ui.app
        .mouse(mouse(MouseEventKind::Down(MouseButton::Left), x, thumb));
    assert_eq!(synced(&mut ui), "", "终端没报一格多大：不开");
}

/// 审计 2026-11-18 #067：抓着滚动条按 `Ctrl+Z`、`Ctrl+G`，走的时候终端的像素坐标关了（`terminal.rs`），这边原来还记着开着：
/// 回来以后不再写打开，终端照格子报、这边照像素换，拖起来乱跳。回来以后记成关着，再拖照常重新打开；一格多大照回来时问到的。
#[test]
fn coming_back_to_the_terminal_turns_pixels_off_and_takes_the_new_cell_size() {
    let mut ui = Ui::open(&session(90), usize::MAX, 72, 20);
    ui.app.screen.pixel.cell = Some((10, 20));
    ui.draw();
    let x = ui.app.screen.view.bar_column();
    let bar = ui.app.screen.view.bar().expect("放不下，有滚动条");
    let thumb =
        ui.app.screen.areas.body.y + u16::try_from(bar.start.floor() as usize).expect("在屏幕里");
    ui.app.mouse(mouse(MouseEventKind::Moved, x, thumb));
    ui.app
        .mouse(mouse(MouseEventKind::Down(MouseButton::Left), x, thumb));
    assert_eq!(synced(&mut ui), "\x1b[?1016h");
    // 挂起回来，这期间缩放了字体：一格 12×24。
    ui.app.terminal_back(Some((12, 24)));
    assert_eq!(ui.app.screen.pixel.cell, Some((12, 24)));
    assert_eq!(synced(&mut ui), "", "终端那边已经关了，不再写");
    // 接着拖（这一下终端照格子报）：照常重新打开。
    ui.app
        .mouse(mouse(MouseEventKind::Drag(MouseButton::Left), x, thumb));
    assert_eq!(synced(&mut ui), "\x1b[?1016h", "再拖照常打开");
}
