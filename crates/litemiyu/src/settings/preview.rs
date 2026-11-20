//! 「明暗」挑选窗挪到哪一档，整个界面当场换成哪一档（配置 五；项目主人 2026-11-14：「切换明暗这里移动选择项应该即时切换」）。
//!
//! 照 Miyu 终端引导「主题」那一步（`oobe/theme.rs`：选择窗 `Pressed::Moved` 时照挪到的值换样子，`Enter` 选好，`Esc` 换回开窗
//! 以前的；App 每一帧照 `ThemeStep::preview` 盖在配置项上挑一套，`app/theme.rs`）。Miyu 配置页里同一个选择窗挪光标不换样子
//! （`settings/pages/popup_keys.rs` 的 `Pressed::Moved` 只是留着窗）；LiteMiyu 照项目主人的话，配置页的「明暗」和开头的「明暗」
//! 都预览（两处开的是同一个窗、同一项）。Miyu 引导里一并预览的「配色」「主色」LiteMiyu 没有（只有跟随终端一种配色）；「铺底色」
//! 在配置页是开关、按一下当场换；开头里照 Miyu 开「开、关」的选择窗，挪着同样预览。
//!
//! 预览只换内存里的样子（`prefs::apply` → `theme::choose`），不写盘、不进写线程：选择窗开着时照光标那一档算（[`Settings::previewing`]），
//! 窗一关（`Enter` 存了、`Esc`、点了窗外）就照配置的值算回去。开头配色方案那一步照 Miyu `oobe/theme.rs`：选择窗里 `Enter` 只记下
//! 选的（`Opening::picks`），停在这一步时照它换样子，到「下一步」才一起存；`Esc` 回上一步时照配置的值换回去（Miyu
//! `theme_preview` 只在这一步、写成了以后照选的）。

use lm_kernel::config::{Kind, Value};

use super::Settings;
use super::popup::{Popup, Target, Window};

/// 挑选窗里挪光标就预览的几项。
const PREVIEWED: &[&str] = &["tui.theme_mode", "tui.theme_background"];

impl Settings {
    /// 正在预览的那一项和光标停的那一档：开着的是 [`PREVIEWED`] 里一项的选择窗才有。
    pub fn previewing(&self) -> Option<(String, Value)> {
        let Some(Popup {
            window: Window::Choose(choose),
            target: Target::Item(key) | Target::Pick(key),
        }) = &self.popup
        else {
            return None;
        };
        if !PREVIEWED.contains(&key.as_str()) {
            return None;
        }
        let shown = self
            .schema
            .items
            .iter()
            .find(|shown| shown.item.key == key)?;
        match shown.item.kind {
            Kind::Option(options) => {
                let option = options.get(choose.at)?;
                Some((key.clone(), Value::Text((*option).into())))
            }
            // 开头「铺底色」的「开、关」两行（`opening.rs`）。
            Kind::Bool => Some((key.clone(), Value::Bool(choose.at == 0))),
            _ => None,
        }
    }

    /// 盖在配置上的几项：开头停在配色方案那一步时选好还没存的，再盖上开着的选择窗光标那一档。
    pub fn previews(&self) -> Vec<(String, Value)> {
        let mut out = self
            .opening
            .as_ref()
            .filter(|opening| opening.step == super::opening::Step::Theme)
            .map(|opening| opening.picks.clone())
            .unwrap_or_default();
        if let Some((key, value)) = self.previewing() {
            out.retain(|(picked, _)| *picked != key);
            out.push((key, value));
        }
        out
    }

    /// 每收一个事件、每一拍以后看一次：预览的变了（挪了光标、开了窗、关了窗、开头里选好了、离开了配色方案那一步），记下，交回
    /// 要不要照它重换样子。
    pub fn restyle(&mut self) -> bool {
        let now = self.previews();
        if now == self.looked {
            return false;
        }
        self.looked = now;
        true
    }

    /// 一项现在给界面用的值：正在预览的照预览的那一档，别的照配置的最终值。
    pub fn looks(&self, key: &str) -> Option<Value> {
        match self.looked.iter().find(|(previewed, _)| previewed == key) {
            Some((_, value)) => Some(value.clone()),
            None => self
                .snapshot
                .resolved
                .get(key)
                .map(|(value, _)| value.clone()),
        }
    }
}
