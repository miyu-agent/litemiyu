//! LiteMiyu 自己加的几项（`docs/00-设计.md` 第六节第 12、14 条）：界面语言、默认预设。Miyu 的界面语言 `ui.language`
//! 写在个人设置里，这里只有一份配置；默认预设在 Miyu 是 `preset.default`，这里是 `sessions.default`（LM-29 以前只有全部功能、
//! 无功能两档，键名沿用）。

lm_kernel::settings! {
    /// 界面这一块。
    pub struct UiSettings in "ui" {
        /// 界面语言：`auto` 跟系统（`LC_ALL`、`LC_MESSAGES`、`LANG`），`zh`、`en` 是手动选的。主程序启动时读一次，改了下次
        /// 启动用上。
        language: String = "auto" {
            kind: option ["auto", "zh", "en"],
            layers: [System],
            applies: head_start,
            ui: { page: "general", group: "display", control: select },
        },
    }
}

lm_kernel::settings! {
    /// 新会话。
    pub struct SessionDefaults in "sessions" {
        /// 默认预设（设计第六节第 14 条，LM-29，`crate::presets`）：新会话没另指定时用哪个预设。`full` 全部功能，`bare`
        /// 无功能（一件工具都不给、回合开始什么都不注入），别的是自己定义的预设的名字。造会话时读，钉在会话上，开着的会话
        /// 不变。写的名字现在没有的照全部功能开，主程序提示一次（[`crate::presets::default_of`]）。
        ///
        /// LM-29 以前是只有 `full`、`bare` 两个选项的下拉；现在是文字，配置界面照现有的预设列成下拉（主程序
        /// `settings/presets.rs`）。
        default: String = "full" {
            kind: text [64],
            layers: [System],
            applies: new_session,
            ui: { page: "general", group: "new_session", control: select },
        },
    }
}

lm_kernel::settings! {
    /// 终端自己的几项（照 Miyu 终端的 `tui/package.toml`：键、选项、出厂值、生效时机都照它）。引擎不读，主程序启动时照
    /// `config.toml` 读一次（`settings/startup.rs`），配置界面存了当场换上（`app/settings.rs`）。
    pub struct TuiSettings in "tui" {
        /// 按键模式：`vim` 时列表里 `j`、`k` 挪，`/` 才开始搜；`arrows` 打字就搜。出厂 Vim（2026-11-13 项目主人给 LiteMiyu 定的
        /// 「默认使用 vimkey」；Miyu 的出厂值是方向键）。
        keys: String = "vim" {
            kind: option ["arrows", "vim"],
            layers: [System],
            applies: now,
            ui: { page: "general", group: "keys", control: select },
        },
        /// 回复完成后把这一段的步骤折叠为一行。
        timeline_fold: bool = true {
            kind: bool,
            layers: [System],
            applies: now,
            ui: { page: "general", group: "timeline", control: toggle },
        },
        /// 思考默认显示全文。
        timeline_expand_thought: bool = false {
            kind: bool,
            layers: [System],
            applies: now,
            ui: { page: "general", group: "timeline", control: toggle },
        },
        /// 命令默认显示全文。
        timeline_expand_command: bool = false {
            kind: bool,
            layers: [System],
            applies: now,
            ui: { page: "general", group: "timeline", control: toggle },
        },
        /// 编辑的差异默认显示全文。
        timeline_expand_edit: bool = false {
            kind: bool,
            layers: [System],
            applies: now,
            ui: { page: "general", group: "timeline", control: toggle },
        },
        /// 进行中的步骤只显示最新的几行。
        timeline_limit_live: bool = true {
            kind: bool,
            layers: [System],
            applies: now,
            ui: { page: "general", group: "timeline", control: toggle },
        },
        /// 思考进行中滚着显示最新的几行。
        timeline_thought_rows: i64 = 15 {
            kind: int [3, 60],
            layers: [System],
            applies: now,
            ui: { page: "general", group: "timeline", control: number },
        },
        /// 时间线里命令最多预览几行。
        timeline_preview_rows: i64 = 10 {
            kind: int [3, 60],
            layers: [System],
            applies: now,
            ui: { page: "general", group: "timeline", control: number },
        },
        /// 侧边栏列几个会话，算上当前这个；0 是不显示。
        sidebar_sessions: i64 = 8 {
            kind: int [0, 100],
            layers: [System],
            applies: now,
            ui: { page: "general", group: "tui", control: number },
        },
        /// 侧边栏列这个会话里你说的几句话；0 是不显示。
        sidebar_jumps: i64 = 8 {
            kind: int [0, 100],
            layers: [System],
            applies: now,
            ui: { page: "general", group: "tui", control: number },
        },
        /// 明暗：跟随终端的底色，或者一直用深色、浅色。
        theme_mode: String = "auto" {
            kind: option ["auto", "dark", "light"],
            layers: [System],
            applies: now,
            ui: { page: "appearance", group: "theme", control: select },
        },
        /// 铺底色：整个界面铺配色的底色。
        theme_background: bool = true {
            kind: bool,
            layers: [System],
            applies: now,
            ui: { page: "appearance", group: "theme", control: toggle },
        },
        /// 用哪一套图标：Nerd Font 的图标，或者没装这种字体时用的普通字符。
        icons: String = "nerd" {
            kind: option ["nerd", "plain"],
            layers: [System],
            applies: now,
            ui: { page: "appearance", group: "icons", control: select },
        },
    }
}

/// 界面语言跟系统的那一档。
pub const AUTO: &str = "auto";

#[cfg(test)]
mod tests {
    use super::*;
    use lm_kernel::config::{Applies, Layer, Values};

    /// 按键模式出厂 Vim（2026-11-13 项目主人给 LiteMiyu 定的；Miyu 出厂方向键），当场生效。
    #[test]
    fn keys_default_to_vim_and_apply_now() {
        let defaults = TuiSettings::from(&Values::defaults(TuiSettings::ITEMS));
        assert_eq!(defaults.keys, "vim");
        let item = TuiSettings::ITEMS
            .iter()
            .find(|item| item.key == "tui.keys")
            .unwrap();
        assert_eq!(item.applies, Applies::Now);
    }

    #[test]
    fn language_follows_the_system_and_applies_next_start() {
        let defaults = UiSettings::from(&Values::defaults(UiSettings::ITEMS));
        assert_eq!(defaults.language, AUTO);
        let item = &UiSettings::ITEMS[0];
        assert_eq!(item.key, "ui.language");
        assert_eq!(item.layers, &[Layer::System]);
        assert_eq!(item.applies, Applies::HeadStart);
    }

    #[test]
    fn new_sessions_get_every_feature_by_default() {
        let defaults = SessionDefaults::from(&Values::defaults(SessionDefaults::ITEMS));
        assert_eq!(defaults.default, "full");
        let item = &SessionDefaults::ITEMS[0];
        assert_eq!(item.key, "sessions.default");
        assert_eq!(item.applies, Applies::NewSession);
    }
}
