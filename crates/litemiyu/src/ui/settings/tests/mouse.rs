//! 配置界面的鼠标（照 Miyu 终端 `settings/mouse.rs`）：先画一帧记下点得到哪，再照记下的地方点。点主菜单进页、点分页换页、
//! 点栏里的行选中、再点开窗、点窗里的格子和按钮、问一句的窗点按钮、滚轮挪；窗盖着的地方点不到底下；开头那几屏不收点。

use crossterm::event::{Event, KeyCode, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};

use super::{Scratch, press, screen, settings};
use crate::settings::models::Popup as ModelPopup;
use crate::settings::models::nav::{Col, Page};
use crate::settings::{Hit, Outcome, Popup, Settings, View, Window};

/// 记下的第一处 `hit` 在哪：先画一帧。
fn place(settings: &Settings, hit: Hit) -> Rect {
    screen(settings);
    settings
        .hits
        .find(hit)
        .unwrap_or_else(|| panic!("没画出 {hit:?}"))
}

/// 照终端的样子交一个鼠标事件。
fn mouse(settings: &mut Settings, kind: MouseEventKind, at: Position) -> Outcome {
    let outcome = settings.handle(&Event::Mouse(MouseEvent {
        kind,
        column: at.x,
        row: at.y,
        modifiers: KeyModifiers::NONE,
    }));
    // 写盘在写线程里（毛病二）：点完等排着的写完，下面照盘上的、写完以后的状态行查。
    settings.flush_writes();
    outcome
}

/// 左键点 `hit` 那一处（往里挪一格，不压在边上）。
fn click(settings: &mut Settings, hit: Hit) -> Outcome {
    let rect = place(settings, hit);
    let at = Position::new(rect.x + 1.min(rect.width.saturating_sub(1)), rect.y);
    mouse(settings, MouseEventKind::Down(MouseButton::Left), at)
}

const TWO: &str = "[providers.alpha]\nbase_url = \"http://127.0.0.1:9/v1\"\n\n[providers.alpha.models.\"m1\"]\n\n\
                   [providers.beta]\nbase_url = \"http://127.0.0.1:8/v1\"\n\n[providers.beta.models.\"m2\"]\n";

fn models_popup(settings: &Settings) -> Option<&ModelPopup> {
    settings.models.as_ref().and_then(|m| m.popup.as_ref())
}

#[test]
fn clicking_a_menu_entry_opens_its_page_and_the_wheel_moves() {
    let (mut settings, _scratch) = settings("mouse-menu");
    // 滚轮往下一格：主菜单挪到第二项。
    mouse(
        &mut settings,
        MouseEventKind::ScrollDown,
        Position::new(5, 5),
    );
    assert_eq!(settings.view, View::Menu(1));
    mouse(&mut settings, MouseEventKind::ScrollUp, Position::new(5, 5));
    assert_eq!(settings.view, View::Menu(0));
    // 点第二项（外观）的第二行：进外观页。
    let rect = place(&settings, Hit::Menu(1));
    let second = Position::new(rect.x + 2, rect.y + 1);
    mouse(
        &mut settings,
        MouseEventKind::Down(MouseButton::Left),
        second,
    );
    let (text, _) = screen(&settings);
    assert!(text.contains("◉ 外观"), "{text}");
    // 页里的行点不动（同 Miyu），滚轮挪光标。
    let before = settings.view;
    mouse(
        &mut settings,
        MouseEventKind::ScrollDown,
        Position::new(5, 5),
    );
    assert_ne!(settings.view, before, "滚轮挪到下一项");
}

#[test]
fn tabs_rows_form_rows_and_buttons_take_clicks() {
    let scratch = Scratch::new("mouse-models");
    let paths = scratch.paths();
    std::fs::write(paths.config_file(), TWO).unwrap();
    let mut settings = Settings::at(paths.clone(), "zh", Outcome::Back).unwrap();
    settings.show_models();
    let provider = |s: &Settings| {
        let m = s.models.as_ref().unwrap();
        m.nav.provider(&m.data).map(|p| p.id.clone())
    };
    assert_eq!(provider(&settings).as_deref(), Some("alpha"));
    // 点第二家：选中，不开窗。
    click(&mut settings, Hit::Row(Col::Provider, 1));
    assert_eq!(provider(&settings).as_deref(), Some("beta"));
    assert!(models_popup(&settings).is_none());
    // 再点选中的那一行：开编辑窗。
    click(&mut settings, Hit::Row(Col::Provider, 1));
    assert!(matches!(models_popup(&settings), Some(ModelPopup::Form(_))));
    // 窗盖着的地方点不到底下的栏：点窗里第一格只挪窗里的焦点。
    use crate::settings::models::forms::Focus;
    let focus = |s: &Settings| match models_popup(s) {
        Some(ModelPopup::Form(form)) => (form.focus, form.editing.is_some()),
        other => panic!("窗关了：{other:?}"),
    };
    screen(&settings);
    let other = (0..20)
        .find(|&i| {
            Focus::Row(i) != focus(&settings).0 && settings.hits.find(Hit::FormRow(i)).is_some()
        })
        .expect("窗里还有别的格子");
    click(&mut settings, Hit::FormRow(other));
    assert_eq!(
        focus(&settings),
        (Focus::Row(other), false),
        "点一下只挪焦点"
    );
    assert_eq!(provider(&settings).as_deref(), Some("beta"));
    // 点「取消」按钮：关窗，配置一个字节没动。
    click(&mut settings, Hit::Button(1));
    assert!(models_popup(&settings).is_none());
    assert_eq!(std::fs::read_to_string(paths.config_file()).unwrap(), TWO);
    // 点分页「默认模型」（LM-8 砍了模型池，原来点的是第三页「模型池」）。
    click(&mut settings, Hit::Tab(1));
    assert_eq!(settings.models.as_ref().unwrap().nav.page, Page::Defaults);
    // 点分页回「供应商」，栏照旧点得动。
    click(&mut settings, Hit::Tab(0));
    assert_eq!(settings.models.as_ref().unwrap().nav.page, Page::Providers);
    click(&mut settings, Hit::Row(Col::Provider, 0));
    assert_eq!(provider(&settings).as_deref(), Some("alpha"));
}

#[test]
fn the_reset_question_takes_a_click_on_its_buttons() {
    let scratch = Scratch::new("mouse-unset");
    let paths = scratch.paths();
    std::fs::write(paths.config_file(), "[jobs]\nmax_running = \"10m\"\n").unwrap();
    let mut settings = Settings::at(paths.clone(), "zh", Outcome::Back).unwrap();
    let page = settings
        .layout()
        .iter()
        .position(|e| matches!(e, crate::settings::Entry::Page(p) if settings.schema.pages[*p].id == "general"))
        .unwrap();
    settings.view = View::Menu(page);
    press(&mut settings, KeyCode::Enter);
    let at = settings
        .rows(page)
        .iter()
        .position(|row| {
            matches!(row, crate::settings::Row::Item(i) if settings.schema.items[*i].item.key == "jobs.max_running")
        })
        .unwrap();
    settings.view = View::Page { page, at };
    press(&mut settings, KeyCode::Char('d'));
    assert!(matches!(
        settings.popup,
        Some(Popup {
            window: Window::Confirm(_),
            ..
        })
    ));
    // 点「恢复」。
    click(&mut settings, Hit::Button(0));
    assert!(settings.popup.is_none());
    let config = std::fs::read_to_string(paths.config_file()).unwrap();
    assert!(!config.contains("max_running"), "{config}");
}

#[test]
fn the_opening_does_not_take_clicks() {
    let scratch = Scratch::new("mouse-opening");
    let mut settings = Settings::at(scratch.paths(), "zh", Outcome::Back).unwrap();
    settings.start_opening();
    let (before, _) = screen(&settings);
    for y in 0..30 {
        for x in (0..120).step_by(7) {
            mouse(
                &mut settings,
                MouseEventKind::Down(MouseButton::Left),
                Position::new(x, y),
            );
        }
    }
    mouse(
        &mut settings,
        MouseEventKind::ScrollDown,
        Position::new(5, 5),
    );
    assert!(settings.opening.is_some());
    assert_eq!(screen(&settings).0, before, "开头那几屏点了、滚了都不动");
}
