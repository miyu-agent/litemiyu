//! 刚启动时的 [`App`]：照配置摆好各块（从 `mod.rs` 分出来）。

use super::*;

impl App {
    /// 照配置 `config` 摆好；说的话交给 `core`。
    pub fn new(config: Config, core: Core) -> Self {
        let config_tips = config.front.tips.len();
        let layout = &config.layout;
        let mut input = InputBox::new(
            layout.max_rows,
            Duration::from_millis(layout.double_click_ms),
        );
        input.set_paste_rule(PasteRule {
            lines: layout.paste_fold_lines,
            chars: layout.paste_fold_chars,
            label: config.front.paste_label.clone(),
        });
        let md_keep = layout.markdown_cache;
        Self {
            config,
            screen: super::screen::Screen::new(),
            talk: super::talk::Talk::default(),
            link: super::link::Link {
                core,
                human: Human::default(),
                default_chat: None,
            },
            typing: super::typing::Typing {
                input,
                menu: Menu::default(),
                history: History::default(),
                esc_at: None,
                tips: Tips::new(
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map_or(1, |d| d.as_nanos() as u64 >> 3),
                    config_tips,
                ),
                notice: None,
                focus: crate::focus::Focus::Input,
                drafts: Default::default(),
                staging: None,
            },
            boxes: Default::default(),
            media: super::media::Media::new(md_keep),
            cwd: std::env::current_dir()
                .map(|d| crate::local::home_short(&d.display().to_string()))
                .unwrap_or_default(),
            quit: false,
            settings: None,
            writes: None,
            lists: Default::default(),
            suspend: false,
            edit: None,
            rescue: None,
        }
    }
}
