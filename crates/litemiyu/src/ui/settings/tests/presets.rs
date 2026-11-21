//! 画「预设」页（LM-29）：主菜单里有它；列表写清哪个是自带的、哪个是默认预设、自己定义的开了什么；表单分三组，全局开关
//! 关着的、已经没有的各有说明，跟着「其余项目」的写明。

use crossterm::event::KeyCode;

use super::{Scratch, press, screen};
use crate::settings::{Extra, Outcome, PresetTarget, Settings, View};

const CONFIG: &str = "[sessions]\ndefault = \"web\"\n\n[presets.web]\nunlisted = \"off\"\n\n[presets.web.features]\nshell = true\nmcp-files = true\nold_tool = true\n\n[mcp.files]\ncommand = [\"true\"]\n\n[features]\nmcp-files = false\n";

fn opened(name: &str) -> (Settings, Scratch) {
    let scratch = Scratch::new(name);
    std::fs::write(scratch.paths().config_file(), CONFIG).unwrap();
    let settings = Settings::at(scratch.paths(), "zh", Outcome::Back).unwrap();
    (settings, scratch)
}

#[test]
fn the_menu_has_a_presets_page_and_its_list_says_what_each_preset_is() {
    let (mut settings, _scratch) = opened("presets-list");
    let (text, _) = screen(&settings);
    assert!(
        text.contains("新建、编辑、删除预设，设置默认预设"),
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
    assert!(line("全部功能").contains("自带，不能修改"), "{text}");
    assert!(line("无功能").contains("自带，不能修改"), "{text}");
    let web = line("web");
    assert!(web.contains("默认预设"), "{web}");
    assert!(web.contains("其余项目关闭、单独打开 3 项"), "{web}");
    assert!(!web.contains("单独关闭"), "一项都没有的那一截不写：{web}");
    assert!(
        text.contains("空格 设为默认") && text.contains("a 新建"),
        "按键提示：{text}"
    );
    assert!(text.contains("1/3"), "右下角第几个：{text}");
}

#[test]
fn the_form_groups_the_items_and_explains_the_ones_that_cannot_work() {
    let (mut settings, _scratch) = opened("presets-form");
    settings.open_preset(PresetTarget::Edit("web".to_string()));
    let (text, _) = screen(&settings);
    for said in ["名称", "其余项目", "内置工具", "MCP 服务"] {
        assert!(text.contains(said), "{said}：{text}");
    }
    assert!(text.contains("1/16"), "右下角第几个：{text}");
    // 表单比这块屏幕高：光标挪到最后一行，下面那一组跟着露出来。
    for _ in 0..15 {
        press(&mut settings, KeyCode::Down);
    }
    let (below, _) = screen(&settings);
    assert!(
        below.contains("已经不存在的项目") && below.contains("16/16"),
        "{below}"
    );
    let text = format!("{text}\n{below}");
    let line = |name: &str| {
        text.lines()
            .find(|line| line.trim_start_matches(['┃', ' ']).starts_with(name))
            .unwrap_or_else(|| panic!("有 {name} 那一行：{text}"))
            .to_string()
    };
    assert!(line("名称").contains("web"));
    let rest = line("其余项目");
    assert!(
        rest.contains("关") && rest.contains("以后新增的 MCP 服务和技能也使用它"),
        "{rest}"
    );
    let shell = line("shell");
    assert!(
        shell.contains("开") && !shell.contains("跟随其余项目"),
        "单独设置过的：{shell}"
    );
    let read = line("read");
    assert!(
        read.contains("关") && read.contains("跟随其余项目"),
        "没单独设置的：{read}"
    );
    let files = line("files");
    assert!(
        files.contains("开") && files.contains("已在 MCP 页关闭，在预设里打开也不可用"),
        "全局开关关着的说清：{files}"
    );
    assert!(
        line("old_tool").contains("已经不存在，按 Enter 从预设中移除"),
        "{text}"
    );
    assert!(
        text.contains("恢复为跟随其余项目"),
        "表单的按键提示：{text}"
    );
}
