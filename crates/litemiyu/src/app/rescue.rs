//! 应急模式在界面这一头（设计第五节第 16 条，`crate::rescue`）：输入框上边框右边一个标记（[`Rescue::tag`]），离不开磁盘
//! 的命令不列（同无功能的一档不列 `/loop`），起来时说一句配置读不了、没给 key。
//!
//! 不列的：`/sessions`（会话都在磁盘上）、`/config`（要写配置）、`/export`（照磁盘上的会话日志导出，应急模式的日志只在
//! 内存里）、`/delete`（删的是磁盘上的会话目录；应急模式的会话 `/new` 就没了）、`/preset`（预设在配置里，应急模式不读
//! 配置，工具面是固定的；`/new` 后面只认自带的 `full`、`bare`）；敲了照「没有这个命令」答，`/help` 也不列。
//! `/model`、`/effort`、`/loop` 照常（只在这一次运行里生效，不写盘：`app/models.rs`、`core/engine/models.rs`）。
//! `Ctrl+G` 的外部编辑器要写临时文件，按了照实说没有。侧边栏的「最近会话」不扫（`app/sessions.rs`）。

use super::App;
use crate::commands::{Run, Spec};
use crate::config::RescueWords;
use crate::core::{Choice, ChoiceState};

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
            notes.push(
                words
                    .config_unreadable
                    .replace("{why}", &self.config.text().why(why)),
            );
        }
        if keyless {
            notes.push(words.no_key.replace("{name}", crate::rescue::API_KEY));
        }
        self.rescue = Some(Box::new(Rescue { words }));
        if !notes.is_empty() {
            self.hint(notes.join("\n"), false);
        }
    }

    /// 应急模式下 `/model` 的一行行：正在用的那一个（`current`）引擎没列的补一行（打名字换成的、配置读不了时），交回
    /// 打的字当模型名用在哪一家下面（正在用的那一家）。不是应急模式的原样交回、没有那一家。
    pub(super) fn rescue_models(
        &self,
        mut all: Vec<Choice>,
        current: Option<&str>,
    ) -> (Vec<Choice>, Option<String>) {
        let Some((provider, model)) = current
            .filter(|_| self.rescue.is_some())
            .and_then(|current| current.split_once('/'))
        else {
            return (all, None);
        };
        if !all.iter().any(|c| Some(c.reference.as_str()) == current) {
            let prefix = format!("{provider}/");
            let state = all
                .iter()
                .find(|c| c.reference.starts_with(&prefix))
                .map_or(ChoiceState::Ok, |c| c.state);
            all.push(Choice {
                reference: format!("{provider}/{model}"),
                name: crate::tabs::printable(model).into_owned(),
                detail: crate::tabs::printable(provider).into_owned(),
                state,
            });
        }
        (all, Some(provider.to_string()))
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

/// 这条命令列不列、认不认：会话说了不认循环的（`loops` 是假：无功能的那一档）不列 `/loop`；应急模式（`rescue`）不列离不开
/// 磁盘的那几条（见模块的说明）。不列的敲了照「没有这个命令」答，`/help` 也不列。
pub fn offers(loops: Option<bool>, rescue: bool, spec: &Spec) -> bool {
    let no_disk = matches!(
        spec.run,
        Run::Sessions | Run::Config | Run::Export | Run::Delete | Run::Preset
    );
    (spec.run != Run::Loop || loops != Some(false)) && !(rescue && no_disk)
}
