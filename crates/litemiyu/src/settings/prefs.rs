//! 终端自己的几项（`tui.*`，照 Miyu 终端 `tui/package.toml`）换到界面上：按键模式、时间线、侧边栏、明暗、铺底色、图标。
//! 启动时照 `config.toml` 写了的换（`startup.rs` 读），配置界面存了照读回的最终值当场换（`app/settings.rs`）；没写的照
//! `layout.json`、`timeline.json` 出厂的（和 Miyu 的出厂值一样；按键模式 2026-11-13 项目主人定出厂 Vim，引擎登记的出厂值已改，
//! `layout.json` 那一格归界面线改）。

use lm_kernel::config::Value;

use crate::config::Config;
use crate::theme::{self, Choice, Mode};

/// 照 `value`（键 → 值，没写的是 `None`）换到 `config` 和配色上。认不得的值（手改坏了的）不换。
pub fn apply(config: &mut Config, value: impl Fn(&str) -> Option<Value>) {
    let text = |key: &str| match value(key) {
        Some(Value::Text(text)) => Some(text.into_owned()),
        _ => None,
    };
    let flag = |key: &str| match value(key) {
        Some(Value::Bool(on)) => Some(on),
        _ => None,
    };
    let count = |key: &str| match value(key) {
        Some(Value::Int(n)) => usize::try_from(n).ok(),
        _ => None,
    };
    match text("tui.keys").as_deref() {
        Some("vim") => config.layout.vim = true,
        Some("arrows") => config.layout.vim = false,
        _ => {}
    }
    if let Some(name) = text("tui.icons")
        && name != config.icons.name
        && let Ok(icons) = crate::config::named_icons(&name)
    {
        config.icons = icons;
        config.layout.icons = name;
    }
    let timeline = &mut config.timeline;
    let flags = [
        ("tui.timeline_fold", &mut timeline.fold),
        ("tui.timeline_expand_thought", &mut timeline.expand.thought),
        ("tui.timeline_expand_command", &mut timeline.expand.command),
        ("tui.timeline_expand_edit", &mut timeline.expand.edit),
        ("tui.timeline_limit_live", &mut timeline.limit_live),
    ];
    for (key, field) in flags {
        if let Some(on) = flag(key) {
            *field = on;
        }
    }
    let counts = [
        ("tui.timeline_thought_rows", &mut timeline.thought_rows),
        ("tui.timeline_preview_rows", &mut timeline.preview_rows),
        ("tui.sidebar_sessions", &mut config.layout.sidebar_sessions),
        ("tui.sidebar_jumps", &mut config.layout.sidebar_jumps),
    ];
    for (key, field) in counts {
        if let Some(n) = count(key) {
            *field = n;
        }
    }
    let mode = text("tui.theme_mode").and_then(|mode| Mode::read(&mode));
    let ground = flag("tui.theme_background");
    if mode.is_some() || ground.is_some() {
        theme::choose(Choice {
            mode: mode.unwrap_or(Choice::DEFAULT.mode),
            ground: ground.unwrap_or(Choice::DEFAULT.ground),
        });
    }
}

#[cfg(test)]
mod tests {
    use lm_kernel::config::Value;

    use super::apply;
    use crate::config::Config;

    #[test]
    fn written_items_change_the_interface_and_unknown_values_are_left_alone() {
        let mut config = Config::builtin().unwrap();
        apply(&mut config, |key| match key {
            "tui.keys" => Some(Value::Text("vim".into())),
            "tui.icons" => Some(Value::Text("plain".into())),
            "tui.timeline_fold" => Some(Value::Bool(false)),
            "tui.timeline_preview_rows" => Some(Value::Int(4)),
            "tui.sidebar_jumps" => Some(Value::Int(0)),
            "tui.sidebar_sessions" => Some(Value::Int(-1)),
            _ => None,
        });
        assert!(config.layout.vim);
        assert_eq!(config.icons.name, "plain");
        assert!(!config.timeline.fold);
        assert_eq!(config.timeline.preview_rows, 4);
        assert_eq!(config.layout.sidebar_jumps, 0);
        assert_eq!(config.layout.sidebar_sessions, 8, "负数不换");
        apply(&mut config, |key| {
            (key == "tui.keys").then(|| Value::Text("arrows".into()))
        });
        assert!(!config.layout.vim, "改回方向键");
        apply(&mut config, |key| {
            (key == "tui.keys").then(|| Value::Text("emacs".into()))
        });
        assert!(!config.layout.vim, "认不得的值不换");
    }
}
