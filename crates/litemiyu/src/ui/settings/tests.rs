//! 画配置界面：面包屑、主菜单的两行、一页的名字和值、「供应商和模型」的分页和几栏、窗盖在上面。临时目录用完就删（测试没
//! 过也删：放在 [`Scratch`] 里）。

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use lm_engine::store::paths::Paths;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use unicode_width::UnicodeWidthStr;

use super::draw;
use crate::settings::{Outcome, Settings};
use crate::ui::frame::Frame;

fn screen(settings: &Settings) -> (String, Option<ratatui::layout::Position>) {
    let mut buffer = Buffer::empty(Rect::new(0, 0, 120, 30));
    let caret = draw(&mut Frame::new(&mut buffer), settings);
    let text = (0..30)
        .map(|y| {
            // 宽字后面那一格是空的：跳过，同终端上看到的。
            let mut line = String::new();
            let mut skip = false;
            for x in 0..120 {
                let symbol = buffer[(x, y)].symbol();
                if !std::mem::take(&mut skip) {
                    line.push_str(symbol);
                    skip = symbol.width() > 1;
                }
            }
            line.trim_end().to_string()
        })
        .collect::<Vec<_>>()
        .join("\n");
    (text, caret)
}

/// 临时的数据目录：放下就删（断言没过、测试中途退出也删）。
struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(name: &str) -> Scratch {
        let dir = std::env::temp_dir().join(format!(
            "litemiyu-ui-settings-{}-{name}",
            std::process::id()
        ));
        std::fs::create_dir_all(Paths::under(&dir).config).unwrap();
        Scratch(dir)
    }

    fn paths(&self) -> Paths {
        Paths::under(&self.0)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if std::fs::remove_dir_all(&self.0).is_err() {}
    }
}

fn settings(name: &str) -> (Settings, Scratch) {
    let scratch = Scratch::new(name);
    (
        Settings::at(scratch.paths(), "zh", Outcome::Back).unwrap(),
        scratch,
    )
}

fn press(settings: &mut Settings, code: KeyCode) {
    settings.key(KeyEvent::new(code, KeyModifiers::NONE));
}

#[test]
fn the_menu_shows_each_page_with_its_groups() {
    let (settings, _scratch) = settings("menu");
    let (text, caret) = screen(&settings);
    assert!(text.contains("● 配置"), "{text}");
    assert!(text.contains("通用"), "{text}");
    assert!(
        text.contains("显示、新会话、会话、终端界面、时间线、交互"),
        "{text}"
    );
    assert!(text.contains("配色方案、图标"), "{text}");
    assert!(text.contains("供应商和模型"), "{text}");
    assert!(text.contains("供应商、默认模型"), "{text}");
    assert!(!text.contains("模型池"), "LM-8 砍了模型池：{text}");
    // LM-2 砍了联网搜索：高级页只剩压缩那一组。
    assert!(text.contains("压缩"), "{text}");
    assert!(!text.contains("联网"), "{text}");
    assert!(text.contains("Enter 进入"), "{text}");
    // 同 Miyu：主菜单右下角不写第几个。
    assert!(!text.contains("1/7"), "{text}");
    assert!(text.contains("q/Esc 返回"), "{text}");
    // 没有 MCP 服务的不列 MCP（同 Miyu `entries`）；技能页照旧在。
    assert!(
        text.contains("技能") && !text.contains("MCP 服务的状态、开关"),
        "{text}"
    );
    assert_eq!(caret, None);
}

#[test]
fn a_page_shows_names_and_values_without_origins_or_descriptions() {
    let (mut settings, _scratch) = settings("page");
    press(&mut settings, KeyCode::Enter);
    let (text, _) = screen(&settings);
    assert!(text.contains("● 配置 — ◉ 通用"), "{text}");
    assert!(text.contains("界面语言"), "{text}");
    assert!(text.contains("跟随系统"), "{text}");
    assert!(
        text.contains("Enter 编辑   d 恢复默认   Esc 返回"),
        "{text}"
    );
    // 同 Miyu：一页里不写值从哪来、不写选中那一项的说明。
    assert!(!text.contains("下次启动生效"), "{text}");
}

#[test]
fn an_edit_window_puts_the_caret_after_the_text() {
    let (mut settings, _scratch) = settings("edit");
    // 通用页「会话」一组的「后台命令最长运行时间」是数字：开它的编辑窗。
    let item = settings
        .schema
        .items
        .iter()
        .position(|shown| shown.item.key == "jobs.max_running")
        .unwrap();
    let (page, at) = (0..settings.entries())
        .find_map(|page| {
            settings
                .rows(page)
                .iter()
                .position(|row| *row == crate::settings::Row::Item(item))
                .map(|at| (page, at))
        })
        .unwrap();
    settings.view = crate::settings::View::Page { page, at };
    press(&mut settings, KeyCode::Enter);
    let (text, caret) = screen(&settings);
    assert!(text.contains("Enter 保存"), "{text}");
    assert!(caret.is_some(), "编辑窗有光标");
}

/// 第二十六趟（验收时报的）：选择窗下面的按键提示照按键模式写，同开头最下面那一行、对话里各框的下边框（`hint_keys.rs`）：
/// Vim 模式写「j/k 选择」，方向键写「↑/↓ 选择」。几种窗都认 `j` `k`（`settings/popup.rs`）。
#[test]
fn the_choose_window_writes_j_k_with_vim_keys_and_arrows_without() {
    for (keys, want, not) in [
        ("vim", "j/k 选择", "↑/↓"),
        ("arrows", "↑/↓ 选择", "j/k 选择"),
    ] {
        let scratch = Scratch::new(&format!("hint-{keys}"));
        let paths = scratch.paths();
        std::fs::write(paths.config_file(), format!("[tui]\nkeys = \"{keys}\"\n")).unwrap();
        let mut settings = Settings::at(paths, "zh", Outcome::Back).unwrap();
        let item = settings
            .schema
            .items
            .iter()
            .position(|shown| shown.item.key == "sessions.default")
            .unwrap();
        let (page, at) = (0..settings.entries())
            .find_map(|page| {
                settings
                    .rows(page)
                    .iter()
                    .position(|row| *row == crate::settings::Row::Item(item))
                    .map(|at| (page, at))
            })
            .unwrap();
        settings.view = crate::settings::View::Page { page, at };
        press(&mut settings, KeyCode::Enter);
        let (text, _) = screen(&settings);
        assert!(text.contains(want), "{keys}：{text}");
        assert!(!text.contains(not), "{keys}：{text}");
    }
}

#[test]
fn the_models_page_has_two_tabs_and_columns_like_miyu() {
    let scratch = Scratch::new("models");
    let paths = scratch.paths();
    let config = "[providers.fake]\nbase_url = \"http://127.0.0.1:9/v1\"\nkey = { secret = \"fake\" }\n\n\
                  [providers.fake.models.\"org/m1\"]\nwindow = 64000\n\n[models]\nchat = \"fake/org/m1\"\n";
    std::fs::write(paths.config_file(), config).unwrap();
    let mut settings = Settings::at(paths, "zh", Outcome::Back).unwrap();
    settings.show_models();
    let (text, _) = screen(&settings);
    assert!(text.contains("● 配置 — ◉ 供应商和模型"), "{text}");
    assert!(text.contains(" 供应商  ·  默认模型"), "{text}");
    assert!(!text.contains("模型池"), "LM-8 砍了模型池那一页：{text}");
    assert!(
        text.contains("供应商") && text.contains("组织") && text.contains("模型"),
        "{text}"
    );
    assert!(text.contains("┃ fake"), "选中的一行左边竖条：\n{text}");
    assert!(text.contains("全部") && text.contains("org/m1"), "{text}");
    assert!(text.contains("已获取 1 个模型"), "{text}");
    assert!(text.contains("h/l 切栏"), "{text}");
    assert!(text.contains(", . 换页"), "{text}");
    // Enter：编辑窗盖在正中，两个按钮。
    press(&mut settings, KeyCode::Enter);
    let (text, _) = screen(&settings);
    assert!(text.contains("编辑供应商 · fake"), "{text}");
    assert!(text.contains("[ 保存 s ]  [ 取消 Esc ]"), "{text}");
    assert!(
        text.contains("已保存到密钥库 · 留空保持不变") || text.contains("未设置"),
        "{text}"
    );
    press(&mut settings, KeyCode::Esc);
    // 默认模型分页：右边写当前的。
    press(&mut settings, KeyCode::Char('.'));
    let (text, _) = screen(&settings);
    assert!(text.contains("默认文本模型"), "{text}");
    assert!(text.contains("当前  fake/org/m1"), "{text}");
    // 默认模型是最后一页（LM-8 砍了原来第三页「模型池」）：再按 `.` 停在这里。
    press(&mut settings, KeyCode::Char('.'));
    let (text, _) = screen(&settings);
    assert!(text.contains("当前  fake/org/m1"), "{text}");
    assert!(!text.contains("旗舰池"), "{text}");
}

#[test]
fn the_mcp_page_lists_a_server_with_its_switch_and_state() {
    let scratch = Scratch::new("mcp");
    let paths = scratch.paths();
    std::fs::write(
        paths.config_file(),
        "[mcp.files]\ncommand = [\"mcp-files\"]\n\n[features]\nmcp-files = false\n",
    )
    .unwrap();
    let mut settings = Settings::at(paths, "zh", Outcome::Back).unwrap();
    // MCP 在「高级」前面（同 Miyu）。
    let mcp = (0..settings.entries())
        .find(|&e| settings.extra(e) == Some(crate::settings::Extra::Mcp))
        .unwrap();
    for _ in 0..mcp {
        press(&mut settings, KeyCode::Down);
    }
    press(&mut settings, KeyCode::Enter);
    let (text, _) = screen(&settings);
    assert!(text.contains("● 配置 — ◉ MCP"), "{text}");
    assert!(
        text.contains("files  关  命令 mcp-files · 尚未启动过"),
        "{text}"
    );
    assert!(text.contains("Enter 开关"), "{text}");
    assert!(text.contains("1/1"), "{text}");
}

mod mouse;
mod presets;
mod slow_disk;
mod status;

/// 编辑窗的名字那一栏照这一窗里最长的名字算（LM-9补）：英文的「Context window」「Thinking effort」比原来写死的 14 列宽，原来
/// 顶到值上（「Context windowunknown」）；中文的「API 密钥」照样放得下。名字和值之间至少空两格。
#[test]
fn form_labels_never_run_into_their_values() {
    let gap = |text: &str, label: &str| {
        // 名字那一栏的：这一行去掉左边的空格以后就是这个名字打头（值里也可能有这几个字，例如「认证  API 密钥」）。
        let line = text
            .lines()
            .find(|line| line.trim_start().starts_with(label))
            .unwrap_or_else(|| panic!("没有名字「{label}」：\n{text}"));
        let after = &line[line.find(label).unwrap() + label.len()..];
        assert!(after.starts_with("  "), "「{label}」顶到了值上：{line}");
    };
    let scratch = Scratch::new("labels");
    let paths = scratch.paths();
    let config = "[providers.fake]\nbase_url = \"http://127.0.0.1:9/v1\"\nkey = { secret = \"fake\" }\n\n\
                  [providers.fake.models.\"m1\"]\n\n[models]\nchat = \"fake/m1\"\n";
    std::fs::write(paths.config_file(), config).unwrap();
    let mut english = Settings::at(paths.clone(), "en", Outcome::Back).unwrap();
    english.show_models();
    // 状态行写这一家有几个模型：一个的是单数（LM-9补，原来写「1 models」）。
    let (text, _) = screen(&english);
    assert!(
        text.contains("1 model") && !text.contains("1 models"),
        "{text}"
    );
    // 挪到模型那一栏（模型名不带组织，没有组织那一栏），开模型的编辑窗。
    press(&mut english, KeyCode::Char('l'));
    press(&mut english, KeyCode::Enter);
    let (text, _) = screen(&english);
    for label in [
        "Context window",
        "Thinking effort",
        "Max output",
        "Input types",
    ] {
        gap(&text, label);
    }
    let mut chinese = Settings::at(paths, "zh", Outcome::Back).unwrap();
    chinese.show_models();
    press(&mut chinese, KeyCode::Enter);
    let (text, _) = screen(&chinese);
    gap(&text, "API 密钥");
    gap(&text, "显示名称");
}
