//! `/preset` 的框和 `/new <预设名>`：列表到了选中当前会话用的那一个；按键各办各的事（设默认、只切换、删除要按两次、
//! 自带的不能删）；会话开了以后预设固定，`Enter` 只改默认、`s` 是新建会话；`Tab` 补全预设名；侧边栏那一句。

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use lm_engine::presets::Listed;
use lm_kernel::policy::PresetPin;

use super::{ahead, completed, new_argument, side};
use crate::app::{App, Panel};
use crate::config::Config;
use crate::core::{Core, PinState, Pinned, Update};

fn fresh() -> App {
    App::new(Config::builtin().expect("出厂的配置读得懂"), Core::idle())
}

fn press(app: &mut App, code: KeyCode) {
    app.handle(crossterm::event::Event::Key(KeyEvent::new(
        code,
        KeyModifiers::NONE,
    )));
}

fn pin(name: &str) -> PresetPin {
    PresetPin {
        name: name.to_string(),
        unlisted: false,
        features: std::collections::BTreeMap::new(),
    }
}

fn listed(default: &str, custom: &[&str]) -> Listed {
    Listed {
        default: default.to_string(),
        custom: custom.iter().map(|name| pin(name)).collect(),
    }
}

/// 开着 `/preset` 的框、列表已经到了的界面。
fn opened(default: &str, custom: &[&str]) -> App {
    let mut app = fresh();
    app.open_presets();
    assert_eq!(app.boxes.panel, Some(Panel::Presets));
    app.core(Update::Presets(listed(default, custom)));
    app
}

fn selected(app: &App) -> Option<String> {
    app.boxes.presets.as_ref().and_then(|list| list.picked())
}

fn notice(app: &App) -> Option<String> {
    app.typing.notice.as_ref().map(|n| n.text.clone())
}

#[test]
fn the_list_opens_on_the_preset_this_session_uses() {
    // 还没开会话、没另定：默认预设。
    let app = opened("web", &["notes", "web"]);
    assert_eq!(selected(&app).as_deref(), Some("web"));
    // 默认预设写的名字没有了：照全部功能。
    let app = opened("gone", &["web"]);
    assert_eq!(selected(&app).as_deref(), Some("full"));
    // 开了的会话：它钉着的那一个。
    let mut app = fresh();
    app.core(Update::Ready("s-1".to_string()));
    app.core(Update::Preset(Pinned {
        name: "bare".to_string(),
        state: PinState::Same,
    }));
    app.open_presets();
    app.core(Update::Presets(listed("web", &["web"])));
    assert_eq!(selected(&app).as_deref(), Some("bare"));
}

#[test]
fn before_the_first_request_enter_sets_the_default_and_switches_and_s_only_switches() {
    let mut app = opened("full", &["web"]);
    assert!(!app.preset_fixed());
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Down);
    assert_eq!(selected(&app).as_deref(), Some("web"));
    press(&mut app, KeyCode::Char('s'));
    assert_eq!(app.boxes.panel, None, "选了就关框");
    assert_eq!(app.link.next_preset.as_deref(), Some("web"));
    assert_eq!(notice(&app).as_deref(), Some("已切换：web"));
    // 再开：选中的是定好的下一个。回车设默认、同时切过去。
    app.open_presets();
    app.core(Update::Presets(listed("full", &["web"])));
    assert_eq!(selected(&app).as_deref(), Some("web"));
    press(&mut app, KeyCode::Up);
    press(&mut app, KeyCode::Enter);
    assert_eq!(app.link.next_preset.as_deref(), Some("bare"));
    assert_eq!(app.talk.loops, Some(false), "无功能的不列 /loop");
    assert_eq!(notice(&app).as_deref(), Some("已切换并设为默认：无功能"));
}

#[test]
fn once_the_session_has_started_the_preset_is_fixed() {
    let mut app = fresh();
    app.core(Update::Ready("s-1".to_string()));
    app.core(Update::Preset(Pinned {
        name: "full".to_string(),
        state: PinState::Same,
    }));
    assert!(app.preset_fixed());
    app.open_presets();
    app.core(Update::Presets(listed("full", &["web"])));
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Down);
    // 回车只改默认预设：会话照旧。
    press(&mut app, KeyCode::Enter);
    assert_eq!(notice(&app).as_deref(), Some("默认预设：web"));
    assert!(app.talk.transcript.session.is_some(), "会话没换");
    assert_eq!(app.link.next_preset, None);
    // `s`：用它新建一个会话。
    app.open_presets();
    app.core(Update::Presets(listed("web", &["web"])));
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Char('s'));
    assert!(
        app.talk.transcript.session.is_none(),
        "回到首页，等第一句话"
    );
    assert_eq!(app.link.next_preset.as_deref(), Some("web"));
    assert_eq!(app.talk.preset, None);
    assert_eq!(notice(&app).as_deref(), Some("已新建会话：web"));
}

#[test]
fn d_asks_once_more_and_built_in_presets_stay() {
    let mut app = opened("full", &["web"]);
    press(&mut app, KeyCode::Char('d'));
    assert_eq!(notice(&app).as_deref(), Some("自带的预设不能删除"));
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Char('d'));
    assert_eq!(notice(&app).as_deref(), Some("再按一次 d 删除「web」"));
    // 按了别的键：不删，再按 `d` 重新问。
    press(&mut app, KeyCode::Up);
    press(&mut app, KeyCode::Down);
    press(&mut app, KeyCode::Char('d'));
    assert_eq!(notice(&app).as_deref(), Some("再按一次 d 删除「web」"));
    press(&mut app, KeyCode::Char('d'));
    assert_eq!(notice(&app).as_deref(), Some("已删除「web」"));
    assert_eq!(app.boxes.panel, Some(Panel::Presets), "框还开着");
    // 引擎删完交回新的列表：光标不越过最后一行。
    app.core(Update::Presets(listed("full", &[])));
    assert_eq!(selected(&app).as_deref(), Some("bare"));
    press(&mut app, KeyCode::Esc);
    assert_eq!(app.boxes.panel, None);
}

#[test]
fn tab_completes_the_preset_after_new() {
    let names = ["full", "bare", "web", "web-lite", "notes"];
    assert_eq!(completed("n", &names).as_deref(), Some("notes"));
    assert_eq!(
        completed("w", &names).as_deref(),
        Some("web"),
        "补到共同的开头"
    );
    assert_eq!(
        completed("web", &names).as_deref(),
        Some("web-lite"),
        "再按换下一个"
    );
    assert_eq!(completed("", &names).as_deref(), Some("full"));
    assert_eq!(
        completed("notes", &names).as_deref(),
        Some("full"),
        "到头回第一个"
    );
    assert_eq!(completed("x", &names), None);
    let commands = Config::builtin().expect("读得懂").commands;
    assert_eq!(new_argument("/new ", &commands), Some(""));
    assert_eq!(new_argument("/new we", &commands), Some("we"));
    assert_eq!(
        new_argument("/new", &commands),
        None,
        "还没打空格：命令列表管"
    );
    assert_eq!(new_argument("/new a b", &commands), None);
    assert_eq!(new_argument("/compact we", &commands), None);
    assert_eq!(new_argument("new we", &commands), None);
}

#[test]
fn the_sidebar_names_the_preset_and_says_when_it_changed_since() {
    let config = Config::builtin().expect("读得懂");
    let text = config.text();
    let pinned = |name: &str, state| Pinned {
        name: name.to_string(),
        state,
    };
    assert_eq!(side(None, text), None);
    let full = pinned("full", PinState::Same);
    assert_eq!(side(Some(&full), text).as_deref(), Some("预设 全部功能"));
    let bare = pinned("bare", PinState::Same);
    assert_eq!(side(Some(&bare), text).as_deref(), Some("预设 无功能"));
    let changed = pinned("web", PinState::Changed);
    assert_eq!(
        side(Some(&changed), text).as_deref(),
        Some("预设 web（已修改）")
    );
    let gone = pinned("web", PinState::Gone);
    assert_eq!(
        side(Some(&gone), text).as_deref(),
        Some("预设 web（已删除）")
    );
}

#[test]
fn before_a_session_exists_the_preset_it_will_use_is_known() {
    let mut app = fresh();
    // `/status` 第一段「预设」那一行写的（LM-33：输入框下面那一行不再写预设名）。
    let status = |app: &App| crate::ui::status_box::facts(&app.seen()).preset;
    // 没另定、默认预设是全部功能：照全部功能写。
    assert_eq!(
        ahead(&app.talk, &app.link).map(|p| p.name).as_deref(),
        Some("full")
    );
    assert_eq!(status(&app).as_deref(), Some("全部功能"));
    // 默认预设是自己定义的：写它。
    app.link.default_preset = Some("web".to_string());
    assert_eq!(status(&app).as_deref(), Some("web"));
    // 另定了下一个（`--preset`、`/new <名字>`、`/preset` 里的 `s`）：照它。
    app.link.next_preset = Some("bare".to_string());
    assert_eq!(status(&app).as_deref(), Some("无功能"));
    // 会话开了：照引擎说的那一个，不再看默认预设。
    app.core(Update::Ready("s-1".to_string()));
    assert_eq!(ahead(&app.talk, &app.link), None);
    assert_eq!(status(&app), None, "引擎还没说");
    app.core(Update::Preset(Pinned {
        name: "cmd".to_string(),
        state: PinState::Changed,
    }));
    assert_eq!(status(&app).as_deref(), Some("cmd（已修改）"));
    // 列表交回来、配置界面存了：默认预设跟着换。
    app.core(Update::Presets(listed("notes", &["notes"])));
    assert_eq!(app.link.default_preset.as_deref(), Some("notes"));
    app.core(Update::Presets(listed("full", &["notes"])));
    assert_eq!(app.link.default_preset, None);
}
