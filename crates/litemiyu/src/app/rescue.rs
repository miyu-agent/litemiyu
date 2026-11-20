//! 应急模式在界面这一头（设计第五节第 16 条，`crate::rescue`）：输入框上边框右边一个标记（[`Rescue::tag`]），要写盘或者
//! 应急模式没有的命令不列（同无功能的一档不列 `/loop`），起来时说一句配置读不了、没给 key。
//!
//! 不列的：`/sessions`（会话都在磁盘上）、`/config`（要写配置）、`/loop`（工具面上没有）；敲了照「没有这个命令」答。
//! `Ctrl+G` 的外部编辑器要写临时文件，按了照实说没有。侧边栏的「最近会话」不扫（`app/sessions.rs`）。

use super::App;
use crate::commands::{Run, Spec};
use crate::config::RescueWords;

/// 应急模式的界面要的：那几句字。
#[derive(Debug)]
pub struct Rescue {
    words: RescueWords,
}

impl Rescue {
    /// 输入框上边框右边的标记（`ui/input_box.rs`）。
    pub fn tag(&self) -> &str {
        &self.words.tag
    }
}

impl App {
    /// 进应急模式：记下字，配置读不了（`unreadable`，引擎的原话）、没给 key（`keyless`）的各弹一句提示。
    pub fn enter_rescue(&mut self, words: RescueWords, unreadable: Option<&str>, keyless: bool) {
        let mut notes = Vec::new();
        if let Some(why) = unreadable {
            notes.push(words.config_unreadable.replace("{why}", why));
        }
        if keyless {
            notes.push(words.no_key.replace("{name}", crate::rescue::API_KEY));
        }
        self.rescue = Some(Box::new(Rescue { words }));
        if !notes.is_empty() {
            self.hint(notes.join("\n"), false);
        }
    }

    /// 这条命令应急模式下列不列（见模块的说明）；不是应急模式的都列。
    pub(super) fn rescue_offers(&self, spec: &Spec) -> bool {
        self.rescue.is_none()
            || !matches!(
                spec.run,
                Run::Sessions | Run::Config | Run::Loop | Run::Model | Run::Effort
            )
    }

    /// 应急模式下按了要写盘的那一样（`Ctrl+G`）：照实说没有，交回真；不是应急模式的交回假，照常办。
    pub(super) fn rescue_refuses(&mut self) -> bool {
        let Some(rescue) = &self.rescue else {
            return false;
        };
        let note = rescue.words.unavailable.clone();
        self.hint(note, false);
        true
    }
}
