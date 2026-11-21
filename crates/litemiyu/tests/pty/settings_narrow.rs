//! 配置界面、第一次运行在难看的终端里（配置线返工第三趟收尾）：16 色、`NO_COLOR`、窄到 50 列。每一页、每种窗、开头每一屏走一遍，
//! 每一屏查没有「字色和底色一样」看不见的字；设了 `LITEMIYU_SHOTS` 的照「变体-屏」的名字存样张，人再看截断、溢出、窗盖不住后面。
//! 数据在临时目录，供应商的地址是连不上的本机端口，key 是假的。

use crate::support::{Options, Session};

const DOWN: &[u8] = b"\x1b[B";
const ESC: &[u8] = b"\x1b";

/// 一家中转站、两个模型、主对话的模型、一个 MCP 服务（有服务主菜单才列 MCP，配置 七 B2），另写了一项通用页的
/// （恢复默认问一句要它）。
const CONFIG: &str = "[jobs]\nmax_running = \"10m\"\n\n[providers.relay]\ndriver = \"openai-chat\"\nbase_url = \"http://127.0.0.1:9/v1\"\nkey = { env = \"RELAY_KEY\" }\n\n\
[providers.relay.models.\"cline/v4\"]\nwindow = 128000\n\n[providers.relay.models.\"zhipu/glm\"]\nwindow = 64000\n\n\
[models]\nchat = \"relay/cline/v4\"\n\n[mcp.files]\ncommand = [\"mcp-files\"]\n";

/// 一种变体：名字（样张的前缀）、宽、多设的环境变量。
struct Variant {
    tag: &'static str,
    cols: u16,
    env: Vec<(&'static str, &'static str)>,
}

impl Variant {
    /// 配置界面选中的那一行行首的记号：有颜色的竖条，没有颜色的箭头（配置 七 C19，同 Miyu 8a2c7d25）。
    fn mark(&self) -> &'static str {
        if self.env.iter().any(|(name, _)| *name == "NO_COLOR") {
            "❯"
        } else {
            "┃"
        }
    }

    fn options(&self) -> Options {
        Options {
            cols: Some(self.cols),
            env: self.env.clone(),
            ..Options::default()
        }
    }
}

/// 照现在的屏幕查看不见的字：有字的格子，字色和底色都不是终端默认的、又一样（反显的先换过来）。存样张。
fn look(session: &Session, variant: &Variant, name: &str) {
    let mut parser = vt100::Parser::new(45, variant.cols, 0);
    parser.process(&session.formatted());
    let screen = parser.screen();
    let mut hidden = Vec::new();
    for row in 0..45 {
        for col in 0..variant.cols {
            let Some(cell) = screen.cell(row, col) else {
                continue;
            };
            let text = cell.contents();
            if text.trim().is_empty() {
                continue;
            }
            let (mut fg, mut bg) = (cell.fgcolor(), cell.bgcolor());
            if cell.inverse() {
                std::mem::swap(&mut fg, &mut bg);
            }
            if fg != vt100::Color::Default && fg == bg {
                hidden.push((row, col, text, fg));
            }
        }
    }
    // 16 色下暗色的字落在选中的底、悬浮窗的底上降成同一格的，界面线第十六趟在降色深那里换成了终端默认的字色
    // （`theme/depth.rs`）：原来放过的那一种撞色不再放过（界面线改的，配置线的文件）。
    let other: Vec<_> = hidden.iter().collect();
    assert!(
        other.is_empty(),
        "{} {name}：看不见的字 {other:?}\n{}",
        variant.tag,
        session.screen()
    );
    session.shot(&format!("{}-{name}", variant.tag));
}

/// 往下挪，直到选中的那一行（配置界面 `┃`、开头 `❯` 开头）写着 `name`。
fn down_to(session: &Session, name: &str) {
    let chosen = |l: &str| (l.contains('┃') || l.contains('❯')) && l.contains(name);
    for _ in 0..40 {
        if session.screen().lines().any(chosen) {
            return;
        }
        session.press(DOWN);
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    session.wait_until(name, |s| s.screen().lines().any(chosen));
}

/// 配置界面每一页、每种窗。
fn settings_tour(variant: &Variant) {
    let session = Session::launch_with(Some(CONFIG), &[], &["config"], &variant.options());
    session.wait_for("Enter 进入", "主菜单");
    look(&session, variant, "menu");
    session.press_enter();
    session.wait_for("◉ 通用", "通用页");
    look(&session, variant, "general");
    session.press_enter();
    session.wait_for("Enter 确定", "选择窗");
    look(&session, variant, "choose");
    session.press(ESC);
    session.wait_until("选择窗关上", |s| !s.screen().contains("Enter 确定"));
    down_to(&session, "后台命令");
    session.press_enter();
    session.wait_for("Enter 保存", "编辑窗");
    look(&session, variant, "edit");
    session.type_text("x");
    session.press_enter();
    // 读得成字、这一项不收的：同 Miyu 窗关上、状态行说没存成（配置 六 C14）。
    session.wait_for("的值无效", "填错的说没存成");
    session.wait_until("编辑窗关上", |s| !s.screen().contains("Enter 保存"));
    look(&session, variant, "edit-bad");
    session.type_text("d");
    session.wait_for("[ 恢复 ]", "恢复默认问一句");
    look(&session, variant, "unset");
    session.press(ESC);
    session.wait_until("问的窗关上", |s| !s.screen().contains("[ 恢复 ]"));
    session.press(ESC);
    session.wait_for("Enter 进入", "回主菜单");
    // 外观。
    session.press(DOWN);
    session.press_enter();
    session.wait_for("◉ 外观", "外观页");
    look(&session, variant, "look");
    session.press(ESC);
    session.wait_for("Enter 进入", "回主菜单");
    // 供应商和模型：三个分页、几种窗。
    session.press(DOWN);
    session.press_enter();
    session.wait_for(&format!("{} relay", variant.mark()), "供应商分页");
    look(&session, variant, "models-providers");
    session.press_enter();
    session.wait_for("[ 保存 s ]", "供应商的编辑窗");
    look(&session, variant, "models-provider-edit");
    session.press(ESC);
    session.wait_until("编辑窗关上", |s| !s.screen().contains("[ 保存 s ]"));
    session.type_text("ll");
    session.wait_for(&format!("{} cline/v4", variant.mark()), "模型栏");
    session.press_enter();
    session.wait_for("上下文窗口", "模型资料窗");
    look(&session, variant, "models-model-edit");
    session.press(ESC);
    session.wait_until("资料窗关上", |s| !s.screen().contains("上下文窗口"));
    session.type_text("hh");
    session.type_text("a");
    session.wait_for("自定义…", "添加供应商的目录窗");
    look(&session, variant, "models-add");
    session.press(ESC);
    session.wait_until("目录窗关上", |s| !s.screen().contains("自定义…"));
    session.type_text(".");
    session.wait_for("默认对话模型", "默认模型分页");
    look(&session, variant, "models-defaults");
    session.press_enter();
    session.wait_for("relay/zhipu/glm", "选默认模型的窗");
    look(&session, variant, "models-defaults-pick");
    session.press(ESC);
    session.wait_until("选模型的窗关上", |s| {
        !s.screen().contains("relay/zhipu/glm")
    });
    session.press(ESC);
    session.wait_for("Enter 进入", "回主菜单");
    // 隐私与安全、预设、MCP、技能、高级：照主菜单往下（隐私与安全照 Miyu 排在供应商和模型后面，配置 六 B1；预设页 LM-29
    // 加的，排在 MCP 前面）。
    for (name, anchor) in [
        ("privacy", "◉ 隐私与安全"),
        ("presets", "◉ 预设"),
        ("mcp", "◉ MCP"),
        ("skills", "◉ 技能"),
        ("advanced", "◉ 高级"),
    ] {
        session.press(DOWN);
        session.press_enter();
        session.wait_for(anchor, anchor);
        look(&session, variant, name);
        session.press(ESC);
        session.wait_for("Enter 进入", "回主菜单");
    }
    let written = std::fs::read_to_string(
        lm_engine::store::paths::Paths::under(session.data()).config_file(),
    )
    .expect("读得到配置");
    assert_eq!(written, CONFIG, "走一遍什么都没改");
}

/// 第一次运行的开头每一屏（配置 六，照 Miyu 引导）：语言、操作方式、图标、选一家、目录窗、自定义的表单、试不通、配色方案和它的
/// 选择窗、完成。
fn opening_tour(variant: &Variant) {
    let session = Session::launch_with(None, &[], &[], &variant.options());
    session.wait_for("界面语言", "开头第一屏");
    look(&session, variant, "opening-language");
    session.press_enter();
    session.wait_for("你喜欢的操作方式", "操作方式");
    look(&session, variant, "opening-keys");
    session.press_enter();
    session.wait_for("方块或乱码", "图标");
    look(&session, variant, "opening-icons");
    session.press_enter();
    session.wait_for("选择模型供应商", "接模型那一步");
    session.wait_for("更多供应商…", "常用的几家");
    look(&session, variant, "opening-model");
    down_to(&session, "更多供应商…");
    session.press_enter();
    session.wait_for("/ 搜索", "「更多供应商…」浮窗（Vim：按 / 才搜）");
    look(&session, variant, "opening-more");
    session.press(ESC);
    session.wait_until("浮窗关上", |s| !s.screen().contains("/ 搜索"));
    down_to(&session, "自定义");
    session.press_enter();
    session.wait_for("支持兼容接口", "自定义的表单");
    look(&session, variant, "opening-custom");
    session.type_text("jj");
    session.press_enter();
    session.paste("http://127.0.0.1:9/v1");
    session.press_enter();
    session.type_text("jj");
    session.press_enter();
    session.type_text("sk-FAKE-narrow-0000");
    session.press_enter();
    session.type_text("j");
    session.press_enter();
    // 连不上、自定义的这一家配置和目录里都挑不出能发一句的模型：同 Miyu 核心 `route/probe.rs` 的 `need_model`，先要模型名（配置
    // 七）。原来写成两条路都认（有能发的模型就直接「仍然保存」）：第二十四趟查了，`cargo test -p litemiyu` 和 `--workspace` 编出来的
    // 程序都只走这一条（线文档第二十四趟）。
    session.wait_for("未获取到模型列表", "列不出模型，先要模型名");
    look(&session, variant, "opening-no-list");
    session.press_enter();
    session.type_text("m1");
    session.press_enter();
    session.type_text("j");
    session.press_enter();
    session.wait_for("仍然保存", "填了模型名还不通：多一行仍然保存");
    look(&session, variant, "opening-failed");
    // 光标回到密钥：往下是模型名、测试连接、仍然保存。
    session.type_text("jjj");
    session.press_enter();
    session.wait_for("整个界面使用配色方案的底色", "配色方案");
    look(&session, variant, "opening-theme");
    session.press_enter();
    session.wait_for("Enter 确定", "明暗的选择窗");
    look(&session, variant, "opening-theme-pick");
    session.press(ESC);
    session.wait_until("选择窗关上", |s| !s.screen().contains("Enter 确定"));
    session.type_text("jj");
    session.press_enter();
    session.wait_for("按 Enter 开始对话", "完成");
    look(&session, variant, "opening-done");
    assert!(
        !session.screen().contains("sk-FAKE-narrow-0000"),
        "key 不上屏"
    );
}

fn sixteen() -> Variant {
    Variant {
        tag: "c16-50",
        cols: 50,
        env: vec![("LITEMIYU_COLOR", "16")],
    }
}

fn mono() -> Variant {
    Variant {
        tag: "mono-50",
        cols: 50,
        env: vec![("NO_COLOR", "1")],
    }
}

#[test]
fn the_settings_hold_up_with_sixteen_colors_in_fifty_columns() {
    settings_tour(&sixteen());
}

#[test]
fn the_settings_hold_up_without_color_in_fifty_columns() {
    settings_tour(&mono());
}

#[test]
fn the_opening_holds_up_with_sixteen_colors_in_fifty_columns() {
    opening_tour(&sixteen());
}

#[test]
fn the_opening_holds_up_without_color_in_fifty_columns() {
    opening_tour(&mono());
}
