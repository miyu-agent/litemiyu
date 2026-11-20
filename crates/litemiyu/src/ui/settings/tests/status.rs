//! 状态行照 Miyu（`ui/settings/mod.rs` 的 `status`）：别的页没话时写「供应商和模型」那一页的那一句（「已获取 N 个模型」，黄字），
//! 主菜单不写；正在办的（测试连接）暗色。

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

#[test]
fn other_pages_say_what_the_models_page_says_when_nothing_else_is_said() {
    let scratch = Scratch::new("status-note");
    std::fs::write(scratch.paths().config_file(), ONE).unwrap();
    let mut settings = Settings::at(scratch.paths(), "zh", Outcome::Back).unwrap();
    assert_eq!(status_line(&screen(&settings).0), "", "主菜单不写");
    // 进通用页（还没进过「供应商和模型」）：照停在第一家算。
    press(&mut settings, KeyCode::Enter);
    assert!(matches!(settings.view, View::Page { .. }));
    assert_eq!(status_line(&screen(&settings).0), "已获取 2 个模型");
    // 有话的写话。
    settings.status = Some(("正在测试 fake…".to_string(), Tone::Busy));
    assert_eq!(status_line(&screen(&settings).0), "正在测试 fake…");
    settings.status = None;
    // 去「供应商和模型」换到默认模型分页再出来：别的页照走的时候那一句写（默认模型分页没话可写；LM-8 以前换的是模型池分页，
    // 写「N 个池」）。
    press(&mut settings, KeyCode::Esc);
    let models = settings
        .layout()
        .iter()
        .position(|e| matches!(e, crate::settings::Entry::Page(p) if settings.schema.pages[*p].id == "models"))
        .unwrap();
    settings.view = View::Menu(models);
    press(&mut settings, KeyCode::Enter);
    press(&mut settings, KeyCode::Char('.'));
    press(&mut settings, KeyCode::Char('.'));
    press(&mut settings, KeyCode::Esc);
    settings.view = View::Menu(0);
    press(&mut settings, KeyCode::Enter);
    let said = status_line(&screen(&settings).0);
    assert_eq!(said, "", "默认模型分页没话可写");
}
