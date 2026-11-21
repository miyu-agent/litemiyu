//! 配置界面和对话界面接在这里（Miyu 终端 `app/settings.rs`）：开着时终端的事件都交给它，关了回对话；存过的叫引擎换上新的
//! 配置。画的那一头在 `ui/mod.rs` 的 `draw` 开头。

use crossterm::event::Event;

use super::App;
use crate::core::Command;
use crate::settings::{Outcome, Settings};

impl App {
    /// 打开配置界面，停在主菜单；主菜单按 `Esc` 照 `leave`。开不了的照原因提示一句，留在对话。
    pub fn open_settings(&mut self, leave: Outcome) {
        match Settings::open(self.config.language.code(), leave) {
            Ok(mut settings) => {
                // 后台拉模型列表记的「拉过」和 `/model` 共用一份（每家最多一分钟拉一次）。
                settings.claims = self.lists.claims.clone();
                self.settings = Some(Box::new(settings));
            }
            Err(why) => self.hint(why, false),
        }
    }

    /// 第一次运行的开头（`settings/opening.rs`）：配置界面停在开头那几屏，走完了直接回对话。开不了的照原因提示一句。
    pub fn open_opening(&mut self) {
        self.open_settings(Outcome::Back);
        if let Some(open) = &mut self.settings {
            open.start_opening();
        }
    }

    /// 存过（改了就算，写盘在写线程里）：终端自己的几项（按键模式、时间线、明暗、铺底色……）照现在的值当场换上；写完读回的
    /// 不一样（没存成，退回了）的再换一次。「明暗」的挑选窗开着、挪了光标的，照光标那一档换（预览，`settings/preview.rs`，
    /// 不写盘）；窗关了照配置的值换回来。
    pub(super) fn apply_saved(&mut self) {
        let Some(settings) = &mut self.settings else {
            return;
        };
        let restyle = settings.restyle();
        if std::mem::take(&mut settings.saved) | restyle {
            crate::settings::prefs::apply(&mut self.config, |key| settings.looks(key));
        }
    }

    /// 回对话：存过的叫引擎换上新配置；还有没写完的，写线程留着接着写，写完再叫（引擎照盘上的读，`poll_closed_writes`）。
    pub(super) fn close_settings(&mut self) {
        let Some(settings) = self.settings.take() else {
            return;
        };
        // 预览着关的（挑选窗还开着就退了）：照配置的值换回来，预览的那一档不留。
        if !settings.looked.is_empty() {
            let resolved = &settings.snapshot.resolved;
            crate::settings::prefs::apply(&mut self.config, |key| {
                resolved.get(key).map(|(value, _)| value.clone())
            });
        }
        // 开头里换了界面语言（Miyu 引导「语言」那一步当场换）：对话界面的字照它换。
        if let Some(code) = &settings.switched {
            self.switch_language(code, &settings.snapshot);
        }
        // 默认的聊天模型照配置界面里的值换（开头里接好的那一家、模型页改的）：还没开会话的底栏当场写（Miyu 引导走完、
        // 配置变了以后的 `refresh_effort`）。没配的照旧。
        if let Some((lm_kernel::config::Value::Text(chat), _)) =
            settings.snapshot.resolved.get("models.chat")
        {
            self.chat_configured(Some(chat.to_string()));
        }
        // 默认预设照配置界面里的值换：首页当场照它写（`presets.rs`）。
        let default = settings.presets_listed().effective_default().to_string();
        self.link.default_preset = Some(default).filter(|name| name != lm_engine::presets::FULL);
        let changed = settings.changed;
        let edited = settings.preset_edited();
        let writer = settings.into_writer();
        if writer.busy() {
            self.writes = Some((writer, changed));
        } else if changed {
            self.link.core.send(Command::Reconfigure);
        }
        // 从 `/preset` 的框里进来的：回到那个框（`presets.rs`）。
        self.presets_back(edited);
    }

    /// 换界面语言（开头「语言」那一步，Miyu `app/language.rs` 的 `switch_language`）：照那一种重读界面的字，终端自己的几项照
    /// 配置的值换回去。读不了的照旧（下次启动照配置换）。
    fn switch_language(&mut self, code: &str, snapshot: &lm_engine::settings::Snapshot) {
        let Ok(table) = crate::language::LanguageTable::builtin() else {
            return;
        };
        let auto = code == lm_engine::settings::declared::AUTO;
        let language = match table.find(code) {
            Some(language) if !auto => language,
            _ => table.detect(|var| std::env::var(var).ok()),
        };
        if language == self.config.language {
            return;
        }
        if let Ok(mut config) = crate::config::Config::load(&language, auto) {
            crate::settings::prefs::apply(&mut config, |key| {
                snapshot.resolved.get(key).map(|(value, _)| value.clone())
            });
            self.config = config;
        }
    }

    /// 关了配置界面以后接着收写完了的：没存成的提示一句；都写完了叫引擎换上新配置。
    pub(super) fn poll_closed_writes(&mut self) {
        let Some((writer, _)) = &mut self.writes else {
            return;
        };
        let mut failed = None;
        while let Some(done) = writer.try_done() {
            if let Err(why) = done.result {
                failed = Some(why);
            }
        }
        let idle = !writer.busy();
        if let Some(why) = failed {
            let texts = self.config.text();
            let said = texts.settings_not_saved.replace("{why}", &texts.why(&why));
            self.hint(said, false);
        }
        if idle
            && let Some((_, changed)) = self.writes.take()
            && changed
        {
            self.link.core.send(Command::Reconfigure);
        }
    }

    /// 退出前收尾（`main.rs`）：配置界面关掉，屏幕上说「正在收尾」；交回还没写完的配置（最多等到 `until`）。
    pub fn closing(&mut self) {
        self.close_settings();
        let said = self.config.text().closing.clone();
        self.typing.notice = Some(crate::app::Notice {
            text: said,
            tail: String::new(),
            until: Some(std::time::Instant::now() + std::time::Duration::from_secs(60)),
            good: false,
        });
    }

    /// 等还没写完的配置写完，最多等到 `until`；交回到点时还剩没写完的。
    pub fn finish_writes(&mut self, until: std::time::Instant) -> bool {
        let Some((writer, _)) = &mut self.writes else {
            return false;
        };
        while writer.busy() {
            if !writer.wait(until) {
                return true;
            }
            while writer.try_done().is_some() {}
        }
        false
    }

    /// 配置界面开着时收一个终端事件：回对话的关掉它（存过的叫引擎换上新配置），要退出的退出。
    pub(super) fn settings_event(&mut self, event: &Event) {
        let Some(settings) = &mut self.settings else {
            return;
        };
        let outcome = settings.handle(event);
        let editing = settings.editing.take();
        self.apply_saved();
        match outcome {
            Outcome::Stay => {}
            Outcome::Back => self.close_settings(),
            Outcome::Quit => self.quit = true,
        }
        // 「提示词」那一行：让出终端开编辑器（`compose.rs`），退出以后状态行说一句。
        if let Some(file) = editing {
            self.edit = Some(crate::editor::Edit {
                editor: crate::editor::command(|name| std::env::var(name).ok()),
                file,
                then: crate::editor::Then::Prompt,
            });
        }
    }
}
