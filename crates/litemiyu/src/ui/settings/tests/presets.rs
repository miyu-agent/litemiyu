//! 画「预设」页（LM-29、LM-33）：主菜单里有它；列表写清哪个是自带的、哪个是默认预设，不写说明；预设的窗浮在页上，名字
//! 一行，下面三组排成两列（窄了排一列），全局开关关着的、已经没有的各有说明；改名在窗里那一行改。

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use super::{Scratch, press, screen};
use crate::settings::{Extra, Outcome, PresetTarget, Settings, View};
use crate::ui::frame::Frame;

const CONFIG: &str = "[sessions]\ndefault = \"web\"\n\n[presets.web]\nunlisted = \"off\"\n\n[presets.web.features]\nshell = true\nmcp-files = true\nold_tool = true\n\n[mcp.files]\ncommand = [\"true\"]\n\n[features]\nmcp-files = false\n";

fn opened(name: &str) -> (Settings, Scratch) {
    let scratch = Scratch::new(name);
    std::fs::write(scratch.paths().config_file(), CONFIG).unwrap();
    let settings = Settings::at(scratch.paths(), "zh", Outcome::Back).unwrap();
    (settings, scratch)
}

/// 照 `width` 列、`height` 行画一帧，交回一行行的字（宽字后面那一格跳过）。
fn sized(settings: &Settings, width: u16, height: u16) -> String {
    let mut buffer = Buffer::empty(Rect::new(0, 0, width, height));
    super::super::draw(&mut Frame::new(&mut buffer), settings);
    (0..height)
        .map(|y| {
            let mut line = String::new();
            let mut skip = false;
            for x in 0..width {
                if skip {
                    skip = false;
                    continue;
                }
                let symbol = buffer[(x, y)].symbol();
                skip = unicode_width::UnicodeWidthStr::width(symbol) > 1;
                line.push_str(symbol);
            }
            line.trim_end().to_string()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn the_menu_has_a_presets_page_and_its_list_says_which_is_built_in_and_default() {
    let (mut settings, _scratch) = opened("presets-list");
    let (text, _) = screen(&settings);
    assert!(
        text.contains("新建、编辑、删除"),
        "主菜单里有预设页：{text}"
    );
    let entry = (0..settings.entries())
        .find(|&e| settings.extra(e) == Some(Extra::Presets))
        .expect("有这一页");
    let mcp = (0..settings.entries())
        .find(|&e| settings.extra(e) == Some(Extra::Mcp))
        .expect("配了服务，有 MCP 页");
    assert!(entry < mcp, "预设页排在 MCP 页前面");
    settings.view = View::Menu(entry);
    press(&mut settings, KeyCode::Enter);
    let (text, _) = screen(&settings);
    assert!(text.contains("● 配置 — ◉ 预设"), "{text}");
    let line = |name: &str| {
        text.lines()
            .find(|line| line.trim_start_matches(['┃', ' ']).starts_with(name))
            .unwrap_or_else(|| panic!("有 {name} 那一行：{text}"))
            .to_string()
    };
    assert!(line("全部功能").contains("自带"), "{text}");
    assert!(line("无功能").contains("自带"), "{text}");
    let web = line("web");
    assert!(web.contains("默认"), "{web}");
    assert!(!web.contains("项"), "不写开了几项：{web}");
    // 同 Miyu 预设页的按键：Enter 打开、a 新建、d 删除、Esc 返回，没有「设为默认」。
    assert!(
        text.contains("Enter 打开") && text.contains("a 新建") && text.contains("d 删除"),
        "按键提示：{text}"
    );
    assert!(!text.contains("设为默认"), "{text}");
    assert!(text.contains("1/3"), "右下角第几个：{text}");
}

#[test]
fn the_window_floats_over_the_list_in_two_columns_and_explains_what_cannot_work() {
    let (mut settings, _scratch) = opened("presets-form");
    settings.open_preset(PresetTarget::Edit("web".to_string()));
    let (text, _) = screen(&settings);
    // 底下还是列表那一页（面包屑、列表的按键提示），窗盖在上面。
    assert!(text.contains("◉ 预设"), "{text}");
    assert!(
        text.contains("Enter 打开"),
        "页底下照旧是列表的提示：{text}"
    );
    for said in [
        "名称  web",
        "内置工具  1/12",
        "MCP 服务  1/1",
        "技能  0/0",
        "还没有技能",
    ] {
        assert!(text.contains(said), "{said}：{text}");
    }
    // 两列：内置工具在左，MCP 服务在右，排在同一行上。
    let row = text
        .lines()
        .find(|line| line.contains("内置工具"))
        .expect("有组名");
    assert!(row.contains("MCP 服务"), "两列：{text}");
    let shell = text
        .lines()
        .find(|line| line.contains("执行命令"))
        .expect("有 shell 那一项");
    assert!(shell.contains("[*] 执行命令"), "{shell}");
    let read = text
        .lines()
        .find(|line| line.contains("读取"))
        .expect("read");
    assert!(read.contains("[ ] 读取"), "{read}");
    assert!(
        text.contains("[*] files  已在 MCP 页关闭"),
        "全局开关关着的说清：{text}"
    );
    assert!(
        text.contains("[*] old_tool  已不存在，Enter 移除"),
        "{text}"
    );
    // 出厂是 Vim 按键：箭头写成 j/k、h/l。
    for hint in [
        "j/k 选择",
        "h/l 换列",
        "空格 开关",
        "Ctrl+A 全开/全关",
        "d 删除",
        "Esc 关闭",
    ] {
        assert!(text.contains(hint), "窗的按键提示 {hint}：{text}");
    }
    // 光标挪到一项上：右上角暗色写它的工具名。
    press(&mut settings, KeyCode::Down);
    let (text, _) = screen(&settings);
    assert!(
        text.lines()
            .any(|line| line.contains("web") && line.trim_end().ends_with("read")),
        "标题那一行：{text}"
    );
    // 窄的窗口排成一列，没有「换列」。
    let narrow = sized(&settings, 50, 40);
    let row = narrow
        .lines()
        .find(|line| line.contains("内置工具"))
        .expect("有组名");
    assert!(!row.contains("MCP 服务"), "一列：{narrow}");
    assert!(narrow.contains("MCP 服务"), "{narrow}");
    assert!(!narrow.contains("换列"), "{narrow}");
}

#[test]
fn renaming_happens_in_the_name_row_of_the_window() {
    let (mut settings, _scratch) = opened("presets-rename");
    settings.open_preset(PresetTarget::Edit("web".to_string()));
    press(&mut settings, KeyCode::Enter);
    settings.key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
    for c in "新名".chars() {
        press(&mut settings, KeyCode::Char(c));
    }
    let (text, caret) = screen(&settings);
    assert!(text.contains("名称  新名"), "字写在窗里那一行：{text}");
    assert!(text.contains("内置工具"), "窗还在：{text}");
    assert!(text.contains("Enter 保存"), "按键提示换成改字的：{text}");
    assert!(caret.is_some(), "输入光标在那一行");
}
