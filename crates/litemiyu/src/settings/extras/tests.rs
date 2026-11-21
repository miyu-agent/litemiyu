//! 技能页、MCP 页：主菜单最后两页；列出来、开关存进配置（`features.<编号>`）、读不成的照实列；一个都没有的写「还没有」。

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use lm_engine::store::paths::Paths;

use crate::settings::{Extra, Outcome, Settings, View};

/// 临时的数据目录，用完删掉。
struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir =
            std::env::temp_dir().join(format!("litemiyu-extras-{}-{name}", std::process::id()));
        std::fs::create_dir_all(Paths::under(&dir).config).unwrap();
        Scratch(dir)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if std::fs::remove_dir_all(&self.0).is_err() {}
    }
}

fn press(settings: &mut Settings, code: KeyCode) {
    settings.key(KeyEvent::new(code, KeyModifiers::NONE));
    // 写盘在写线程里（毛病二）：按完等排着的写完，下面照盘上的、写完以后的状态行查。
    settings.flush_writes();
}

/// 从主菜单进 `extra` 那一页。
fn enter(settings: &mut Settings, extra: Extra) {
    let entry = (0..settings.entries())
        .find(|&e| settings.extra(e) == Some(extra))
        .expect("主菜单里有这一页");
    settings.view = View::Menu(entry);
    press(settings, KeyCode::Enter);
    assert!(matches!(settings.view, View::Page { page, at: 0 } if page == entry));
}

#[test]
fn skills_are_listed_switched_and_unreadable_ones_said() {
    let scratch = Scratch::new("skills");
    let paths = Paths::under(&scratch.0);
    for (name, head) in [
        (
            "alpha",
            "---\nname: alpha\ndescription: Does alpha things.\nmetadata:\n  display_name_zh: 阿尔法\n---\nbody\n",
        ),
        ("beta", "---\nname: beta\ndescription: Does beta.\n---\n"),
        ("broken", "no head at all\n"),
    ] {
        let dir = paths.skills().join(name);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("SKILL.md"), head).unwrap();
    }
    let mut settings = Settings::at(paths.clone(), "zh", Outcome::Back).unwrap();
    enter(&mut settings, Extra::Skills);
    let listed = settings.extra_list.clone();
    let names: Vec<_> = listed
        .entries
        .iter()
        .map(|e| (e.shown.as_str(), e.on))
        .collect();
    assert_eq!(
        names,
        [("阿尔法", true), ("beta", true)],
        "照语言挑名字，没写的算开"
    );
    assert_eq!(listed.entries[1].state, "Does beta.");
    assert_eq!(listed.broken.len(), 1, "{listed:?}");
    assert!(listed.broken[0].starts_with("无法读取：") && listed.broken[0].contains("broken"));
    // 第二个关掉：配置里写 false，状态行说已保存（同 Miyu），列表跟着变。
    press(&mut settings, KeyCode::Down);
    press(&mut settings, KeyCode::Enter);
    let config = std::fs::read_to_string(paths.config_file()).unwrap();
    assert!(config.contains("skill-beta = false"), "{config}");
    assert_eq!(settings.status.clone().unwrap().0, "已保存");
    assert!(!settings.extra_list.entries[1].on);
    assert!(settings.changed, "回对话时引擎换上新配置");
    // 再按一次空格开回来。
    press(&mut settings, KeyCode::Char(' '));
    assert!(settings.extra_list.entries[1].on);
    press(&mut settings, KeyCode::Esc);
    assert!(matches!(settings.view, View::Menu(_)));
}

#[test]
fn mcp_servers_show_how_they_start_and_switch() {
    let scratch = Scratch::new("mcp");
    let paths = Paths::under(&scratch.0);
    std::fs::write(
        paths.config_file(),
        "[mcp.files]\ncommand = [\"mcp-files\", \"--root\", \".\"]\n\n[mcp.web]\nurl = \"https://example.invalid/mcp\"\n\n[mcp.bad]\ncommand = 3\n",
    )
    .unwrap();
    let mut settings = Settings::at(paths.clone(), "zh", Outcome::Back).unwrap();
    enter(&mut settings, Extra::Mcp);
    let listed = settings.extra_list.clone();
    let rows: Vec<_> = listed
        .entries
        .iter()
        .map(|e| (e.name.as_str(), e.state.as_str()))
        .collect();
    assert_eq!(
        rows,
        [
            ("files", "命令 mcp-files --root . · 尚未启动过"),
            ("web", "网址 https://example.invalid/mcp · 尚未启动过"),
        ]
    );
    assert_eq!(listed.broken.len(), 1, "{listed:?}");
    assert_eq!(
        listed.broken,
        ["config.toml 第 7 行的 MCP 服务配置无效（写法不正确）"]
    );
    press(&mut settings, KeyCode::Enter);
    let config = std::fs::read_to_string(paths.config_file()).unwrap();
    assert!(config.contains("mcp-files = false"), "{config}");
    assert!(!settings.extra_list.entries[0].on);
}

#[test]
fn empty_pages_say_so_and_the_menu_lists_both_before_advanced() {
    let scratch = Scratch::new("empty");
    let mut settings = Settings::at(Paths::under(&scratch.0), "en", Outcome::Back).unwrap();
    // 同 Miyu（`pages/mod.rs` 的 `entries`）：没有 MCP 服务的不列 MCP；技能页在「高级」前面，「高级」排最后。
    let count = settings.entries();
    assert!(
        (0..count).all(|e| settings.extra(e) != Some(Extra::Mcp)),
        "没有服务不列 MCP"
    );
    assert_eq!(settings.extra(count - 2), Some(Extra::Skills));
    assert_eq!(settings.extra(count - 1), None);
    let last = settings.page_of(count - 1).unwrap();
    assert_eq!(settings.schema.pages[last].id, "advanced");
    assert_eq!(settings.extra(0), None);
    enter(&mut settings, Extra::Skills);
    // 空的页按 Enter、上下挪都不出事。
    press(&mut settings, KeyCode::Down);
    press(&mut settings, KeyCode::Enter);
    assert_eq!(settings.status, None);
    // 有服务了（手改配置）再打开：MCP 在「高级」前面，技能页挨着 MCP。
    let paths = Paths::under(&scratch.0);
    std::fs::create_dir_all(&paths.config).unwrap();
    std::fs::write(
        paths.config_file(),
        "[mcp.files]\ncommand = [\"mcp-files\"]\n",
    )
    .unwrap();
    let settings = Settings::at(paths, "en", Outcome::Back).unwrap();
    let count = settings.entries();
    assert_eq!(settings.extra(count - 3), Some(Extra::Mcp));
    assert_eq!(settings.extra(count - 2), Some(Extra::Skills));
    assert_eq!(settings.texts.extras.empty[1], "No MCP servers yet");
}

/// 只有写错了的服务也列 MCP：页里照行号说哪里不对（不然写错了的人在配置界面里找不到原因）。
#[test]
fn a_broken_server_alone_still_lists_the_mcp_page() {
    let scratch = Scratch::new("broken-only");
    let paths = Paths::under(&scratch.0);
    std::fs::create_dir_all(&paths.config).unwrap();
    std::fs::write(paths.config_file(), "[mcp.bad]\ncommand = 3\n").unwrap();
    let mut settings = Settings::at(paths, "zh", Outcome::Back).unwrap();
    enter(&mut settings, Extra::Mcp);
    assert!(settings.extra_list.entries.is_empty());
    assert_eq!(
        settings.extra_list.broken.len(),
        1,
        "{:?}",
        settings.extra_list
    );
}
