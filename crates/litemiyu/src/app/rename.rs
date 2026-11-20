//! `/rename`：给会话改名、去掉标题（Miyu `app/session.rs` 的 `rename`，蓝图「改名」）。改名交引擎（内核的 `SetMeta`，
//! [`Command::Title`]），改成了弹一句。
//!
//! 只打 `/rename` 让模型照最近的对话重起一个（Miyu 核心 TT-1，引擎的 `Handle::retitle`）：收下了弹「正在重新起标题…」，起好了
//! 照推来的新标题换，没起成弹「更改标题失败」。

use super::App;
use crate::core::{Command, Update};

impl App {
    /// `/rename`：去掉前后空白交给引擎改名，太长的当场提示不发；只写一个 `-` 的去掉标题（交空的标题，同 Miyu
    /// `session.meta_changed` 的写法）。
    pub(super) fn rename(&mut self, words: Option<&str>) {
        let Some(title) = words.map(str::trim).filter(|w| !w.is_empty()) else {
            self.link.core.send(Command::Retitle);
            return;
        };
        if title == "-" {
            self.link.core.send(Command::Title(String::new()));
            return;
        }
        let max = self.config.layout.title_max;
        if title.chars().count() > max {
            let note = self
                .config
                .text()
                .rename
                .too_long
                .replace("{max}", &max.to_string());
            self.hint(note, false);
            return;
        }
        self.link.core.send(Command::Title(title.to_string()));
    }

    /// 改成了（[`Update::Renamed`]）：弹一句，不写进正文；重起标题的收下了、没起成也弹一句（Miyu 视图流的
    /// `retitle_failed`）。别的交回假，照常办。
    pub(super) fn rename_update(&mut self, update: &Update) -> bool {
        match update {
            Update::Retitling => {
                let note = self.config.text().rename.retitling.clone();
                self.hint(note, false);
                return true;
            }
            // 没起成（内核 TT-3）：说内核那一句，后面接那一次的错；标题不变（同 miyu-tui `355ba92c`）。
            Update::RetitleFailed { said, error } => {
                let note = self.say(said);
                let tail = (!error.is_empty()).then(|| format!("{}{error}", crate::sheet::sep()));
                self.hint_keeping(note, tail.unwrap_or_default(), false);
                return true;
            }
            _ => {}
        }
        let Update::Renamed(title) = update else {
            return false;
        };
        let texts = &self.config.text().rename;
        let note = title.as_deref().map_or_else(
            || texts.removed.clone(),
            |title| texts.done.replace("{title}", title),
        );
        self.hint(note, false);
        true
    }
}
