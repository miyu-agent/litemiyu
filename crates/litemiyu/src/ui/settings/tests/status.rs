//! 状态行（Miyu `ui/settings/mod.rs` 的 `status`）：有话写话（正在办的暗色）；没话的只有「供应商和模型」的供应商分页写那一家
//! 有几个模型（黄字），别的页、主菜单不写（LM-38）。

use crossterm::event::KeyCode;

use super::{Scratch, press, screen};
use crate::settings::{Outcome, Settings, Tone, View};

const ONE: &str = "[providers.fake]\nbase_url = \"http://127.0.0.1:9/v1\"\n\n[providers.fake.models.\"m1\"]\nwindow = 64000\n\n\
                   [providers.fake.models.\"m2\"]\nwindow = 64000\n";

/// 状态行（细线上面那一行）。
fn status_line(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let rule = lines
        .iter()
        .rposition(|l| l.contains("────"))
        .expect("有细线");
    lines[rule - 1].trim().to_string()
}

/// LM-38（同 Miyu 终端 11-20 项目主人报的「设置页底下一直写『暂无模型 · 保存后按 r 获取』」）：原来别的页没话时照「供应商和
/// 模型」那一页的选中写「已获取 N 个模型」「暂无模型 · 保存后按 r 获取」，那几页按 r 什么都不做；改成别的页没话就不写，
/// 那一句只在供应商分页写。
#[test]
fn other_pages_stay_quiet_when_nothing_is_said() {
    let scratch = Scratch::new("status-note");
    std::fs::write(scratch.paths().config_file(), ONE).unwrap();
    let mut settings = Settings::at(scratch.paths(), "zh", Outcome::Back).unwrap();
    assert_eq!(status_line(&screen(&settings).0), "", "主菜单不写");
    // 进通用页：没话不写。
    press(&mut settings, KeyCode::Enter);
    assert!(matches!(settings.view, View::Page { .. }));
    assert_eq!(status_line(&screen(&settings).0), "", "通用页没话不写");
    // 有话的写话。
    settings.status = Some(("正在测试 fake…".to_string(), Tone::Busy));
    assert_eq!(status_line(&screen(&settings).0), "正在测试 fake…");
    settings.status = None;
    // 「供应商和模型」的供应商分页照旧写这一家有几个模型；出来再进别的页还是不写。
    press(&mut settings, KeyCode::Esc);
    let models = settings
        .layout()
        .iter()
        .position(|e| matches!(e, crate::settings::Entry::Page(p) if settings.schema.pages[*p].id == "models"))
        .unwrap();
    settings.view = View::Menu(models);
    press(&mut settings, KeyCode::Enter);
    assert_eq!(status_line(&screen(&settings).0), "已获取 2 个模型");
    press(&mut settings, KeyCode::Esc);
    settings.view = View::Menu(0);
    press(&mut settings, KeyCode::Enter);
    assert_eq!(
        status_line(&screen(&settings).0),
        "",
        "别的页不带供应商那一页的话"
    );
}
