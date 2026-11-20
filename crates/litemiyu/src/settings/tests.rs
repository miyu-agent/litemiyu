//! 配置界面的状态和按键：主菜单、照清单画的一页，开关、下拉、一行字，恢复默认，配置读不懂时不让存。

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use lm_engine::store::paths::Paths;

use super::{Outcome, Popup, Row, Settings, Tone, View, Window};

/// 临时的数据目录，用完删掉。
struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir =
            std::env::temp_dir().join(format!("litemiyu-settings-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        Scratch(dir)
    }

    fn paths(&self) -> Paths {
        let paths = Paths::under(&self.0);
        std::fs::create_dir_all(&paths.config).unwrap();
        paths
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if std::fs::remove_dir_all(&self.0).is_err() {}
    }
}

fn press(settings: &mut Settings, code: KeyCode) -> Outcome {
    let outcome = settings.key(KeyEvent::new(code, KeyModifiers::NONE));
    // 写盘在写线程里（毛病二）：按完等排着的写完，下面照盘上的、写完以后的状态行查。
    settings.flush_writes();
    outcome
}

fn typed(settings: &mut Settings, text: &str) {
    for c in text.chars() {
        press(settings, KeyCode::Char(c));
    }
}

/// 进主菜单里名字是 `page` 的那一页，光标挪到名字是 `item` 的那一项。
fn go_to(settings: &mut Settings, page: &str, item: &str) {
    let entry = (0..settings.entries())
        .find(|&e| {
            settings
                .page_of(e)
                .is_some_and(|p| settings.schema.pages[p].id == page)
        })
        .expect("主菜单里有这一页");
    settings.view = View::Menu(entry);
    press(settings, KeyCode::Enter);
    let rows = settings.rows(entry);
    let at = rows
        .iter()
        .position(|row| matches!(row, Row::Item(i) if settings.schema.items[*i].item.key == item))
        .expect("这一页有这一项");
    settings.view = View::Page { page: entry, at };
}

fn config(paths: &Paths) -> String {
    std::fs::read_to_string(paths.config_file()).unwrap_or_default()
}

#[test]
fn the_menu_lists_pages_with_something_to_draw_and_esc_leaves_as_opened() {
    let scratch = Scratch::new("menu");
    let mut settings = Settings::at(scratch.paths(), "zh", Outcome::Quit).unwrap();
    let names: Vec<&str> = settings
        .menu()
        .iter()
        .map(|&p| settings.schema.pages[p].id)
        .collect();
    assert_eq!(
        names,
        ["general", "appearance", "models", "privacy", "advanced"]
    );
    assert_eq!(press(&mut settings, KeyCode::Char('j')), Outcome::Stay);
    press(&mut settings, KeyCode::Char('j'));
    assert_eq!(settings.view, View::Menu(2));
    press(&mut settings, KeyCode::Enter);
    assert!(matches!(settings.view, View::Page { page: 2, .. }));
    press(&mut settings, KeyCode::Esc);
    assert_eq!(settings.view, View::Menu(2), "一页里 Esc 回主菜单");
    assert_eq!(press(&mut settings, KeyCode::Esc), Outcome::Quit);
}

#[test]
fn moving_in_a_page_skips_group_names() {
    let scratch = Scratch::new("move");
    let mut settings = Settings::at(scratch.paths(), "zh", Outcome::Back).unwrap();
    press(&mut settings, KeyCode::Enter);
    let rows = settings.rows(0);
    assert!(matches!(rows[0], Row::Group(_)));
    assert_eq!(
        settings.view,
        View::Page { page: 0, at: 1 },
        "进来停在第一项"
    );
    for _ in 0..rows.len() {
        press(&mut settings, KeyCode::Char('j'));
        let View::Page { at, .. } = settings.view else {
            panic!()
        };
        // 通用页最后一行是「提示词」（`Row::Prompt`）：停得上去，组名照旧跳过。
        assert!(matches!(rows[at], Row::Item(_) | Row::Prompt));
    }
}

#[test]
fn a_toggle_flips_and_saves_at_once() {
    let scratch = Scratch::new("toggle");
    let paths = scratch.paths();
    let mut settings = Settings::at(paths.clone(), "zh", Outcome::Back).unwrap();
    go_to(&mut settings, "advanced", "compaction.prepare");
    press(&mut settings, KeyCode::Enter);
    assert_eq!(config(&paths), "[compaction]\nprepare = false\n");
    assert!(settings.changed);
    let (said, tone) = settings.status.clone().unwrap();
    assert_eq!(tone, Tone::Good);
    assert_eq!(said, "已保存");
}

#[test]
fn a_select_opens_its_options_and_saves_the_chosen_one() {
    let scratch = Scratch::new("select");
    let paths = scratch.paths();
    let mut settings = Settings::at(paths.clone(), "zh", Outcome::Back).unwrap();
    go_to(&mut settings, "general", "ui.language");
    press(&mut settings, KeyCode::Enter);
    let Some(Popup {
        window: Window::Choose(choose),
        ..
    }) = &settings.popup
    else {
        panic!("开了选择窗")
    };
    assert_eq!(choose.names, ["跟随系统", "中文", "English"]);
    assert_eq!(choose.current, Some(0));
    press(&mut settings, KeyCode::Down);
    press(&mut settings, KeyCode::Down);
    press(&mut settings, KeyCode::Enter);
    assert!(settings.popup.is_none());
    assert_eq!(config(&paths), "[ui]\nlanguage = \"en\"\n");
    let item = settings
        .schema
        .items
        .iter()
        .position(|s| s.item.key == "ui.language")
        .unwrap();
    assert_eq!(settings.shown_value(item), "English");
    assert_eq!(
        settings.value("ui.language").map(|(_, origin)| origin),
        Some("system")
    );
}

#[test]
fn text_is_read_by_the_item_kind_and_bad_values_are_not_saved() {
    let scratch = Scratch::new("text");
    let paths = scratch.paths();
    let mut settings = Settings::at(paths.clone(), "zh", Outcome::Back).unwrap();
    go_to(&mut settings, "general", "jobs.max_running");
    press(&mut settings, KeyCode::Enter);
    settings.key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
    typed(&mut settings, "soon");
    press(&mut settings, KeyCode::Enter);
    // 同 Miyu：读得成字、这一项不收的，窗关上、状态行说没存成（Miyu 交给核心，核心不收）。
    assert!(settings.popup.is_none(), "窗关上");
    assert!(
        matches!(&settings.status, Some((said, Tone::Bad)) if said.contains("的值无效")),
        "{:?}",
        settings.status
    );
    assert_eq!(config(&paths), "");
    press(&mut settings, KeyCode::Enter);
    settings.key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
    typed(&mut settings, "10m");
    press(&mut settings, KeyCode::Enter);
    assert_eq!(config(&paths), "[jobs]\nmax_running = \"10m\"\n");
}

/// 数字项读不成数的：窗留着，窗里红字「请输入数字」（Miyu `popup_keys.rs`）。
#[test]
fn a_number_that_is_not_a_number_keeps_the_window_open() {
    let scratch = Scratch::new("number");
    let paths = scratch.paths();
    let mut settings = Settings::at(paths.clone(), "zh", Outcome::Back).unwrap();
    go_to(&mut settings, "advanced", "compaction.line");
    press(&mut settings, KeyCode::Enter);
    settings.key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
    typed(&mut settings, "abc");
    press(&mut settings, KeyCode::Enter);
    assert!(
        matches!(
            &settings.popup,
            Some(Popup { window: Window::Edit(edit), .. })
                if edit.error.as_deref() == Some("请输入数字")
        ),
        "{:?}",
        settings.popup.as_ref().map(|p| &p.window)
    );
    settings.key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
    typed(&mut settings, "99");
    press(&mut settings, KeyCode::Enter);
    assert!(settings.popup.is_none(), "超范围的窗关上");
    assert!(matches!(&settings.status, Some((_, Tone::Bad))));
    assert_eq!(config(&paths), "");
}

#[test]
fn d_goes_back_to_the_default() {
    let scratch = Scratch::new("unset");
    let paths = scratch.paths();
    std::fs::write(
        paths.config_file(),
        "# 留着\n[sessions]\ndefault = \"bare\"\n",
    )
    .unwrap();
    let mut settings = Settings::at(paths.clone(), "zh", Outcome::Back).unwrap();
    go_to(&mut settings, "general", "sessions.default");
    press(&mut settings, KeyCode::Char('d'));
    // 同 Miyu：先问一句，选「恢复」才删。
    assert!(config(&paths).contains("default"), "问完以前不动");
    assert!(matches!(
        &settings.popup,
        Some(Popup { window: Window::Confirm(confirm), .. })
            if confirm.title == "恢复默认：新会话的功能？"
                && confirm.lines == ["移除此项的设置，恢复为默认的值"]
                && confirm.at == 1
    ));
    // 停在「取消」：往左挪到「恢复」再确定。
    press(&mut settings, KeyCode::Left);
    press(&mut settings, KeyCode::Enter);
    assert!(!config(&paths).contains("default"), "{}", config(&paths));
    assert!(config(&paths).starts_with("# 留着\n"));
    assert_eq!(
        settings.status.as_ref().map(|s| s.0.as_str()),
        Some("已保存")
    );
    // 没写的项按 `d` 只说一声。
    press(&mut settings, KeyCode::Char('d'));
    assert!(settings.popup.is_none());
    assert_eq!(
        settings.status.clone(),
        Some(("未设置此项".to_string(), Tone::Note))
    );
}

#[test]
fn a_broken_file_is_shown_but_not_saved() {
    let scratch = Scratch::new("broken");
    let paths = scratch.paths();
    std::fs::write(paths.config_file(), "[ui\n").unwrap();
    let mut settings = Settings::at(paths.clone(), "en", Outcome::Back).unwrap();
    assert_eq!(settings.status.as_ref().map(|s| s.1), Some(Tone::Bad));
    go_to(&mut settings, "advanced", "compaction.prepare");
    press(&mut settings, KeyCode::Enter);
    assert_eq!(config(&paths), "[ui\n", "一个字节没动");
    assert!(!settings.changed);
}

/// 同 Miyu：`Ctrl+J`、`Ctrl+K` 在不打字时同 `↓`、`↑`（按键模式是什么都一样），打字的窗里不挪；`Ctrl+C` 照打开的方式回对话或退出；
/// 别的带 Ctrl 的不管。
#[test]
fn ctrl_keys_move_or_leave_like_miyu() {
    let scratch = Scratch::new("ctrl");
    let ctrl = |c| KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL);
    let mut settings = Settings::at(scratch.paths(), "zh", Outcome::Back).unwrap();
    settings.key(ctrl('j'));
    assert_eq!(settings.view, View::Menu(1));
    settings.key(ctrl('k'));
    assert_eq!(settings.view, View::Menu(0));
    assert_eq!(settings.key(ctrl('x')), Outcome::Stay);
    assert_eq!(settings.view, View::Menu(0), "别的带 Ctrl 的不管");
    assert_eq!(settings.key(ctrl('c')), Outcome::Back);
    go_to(&mut settings, "general", "jobs.max_running");
    let before = settings.view;
    press(&mut settings, KeyCode::Enter);
    settings.key(ctrl('j'));
    assert_eq!(settings.view, before, "打字的窗里不挪");
    assert!(settings.popup.is_some());
    let mut standalone = Settings::at(scratch.paths(), "zh", Outcome::Quit).unwrap();
    assert_eq!(standalone.key(ctrl('d')), Outcome::Quit);
}
