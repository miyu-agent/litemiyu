//! 预设在引擎线程里的几件事（LM-29，`docs/00-设计.md` 第六节第 14 条）：给 `/preset` 的框列预设、把一个记成默认预设、删掉
//! 一个；告诉界面正在说话的会话钉着哪个预设。
//!
//! 列、写都照配置文件，不用先造好会话要的那几样（同 `sessions.rs` 列会话）：`/preset` 开一下不把路由、模型目录、工具目录
//! 拉起来。写用配置服务（`lm_engine::settings`，同 `/model` 记默认模型）：只动那几行，注释、先后不变；写完让开着的会话
//! 换上重读的配置。应急模式不读不写配置，这几条命令什么都不做（界面那一头不列 `/preset`）。

use lm_engine::presets::{self, Chosen, Listed};
use lm_engine::settings::{Edit, Settings};
use lm_engine::store::paths::{Env, Paths};
use lm_kernel::config::Value;

use super::super::{PinState, Pinned, Update};
use super::Engine;

impl Engine {
    /// 是不是应急模式：备着还没用的，或者已经照它造好了。
    fn rescued(&self) -> bool {
        self.rescue.is_some() || self.ready.as_ref().is_some_and(|ready| ready.is_rescue())
    }

    /// 列预设：照这一刻的配置文件读，交回界面。
    pub(super) async fn presets(&self) {
        if self.rescued() {
            return;
        }
        let listed = lm_engine::spawn_blocking(|| {
            Paths::locate(&Env::current())
                .map(|paths| presets::read(&paths))
                .unwrap_or_default()
        })
        .await
        .unwrap_or_default();
        self.report(Update::Presets(listed));
    }

    /// 把预设 `name` 记成默认预设。
    pub(super) async fn preset_default(&mut self, name: String) {
        self.write(move |_| {
            vec![Edit::Set(
                presets::DEFAULT.to_string(),
                Value::Text(name.into()),
            )]
        })
        .await;
    }

    /// 删掉自己定义的预设 `name`：它正好是默认预设的，默认预设回到全部功能。
    pub(super) async fn preset_delete(&mut self, name: String) {
        self.write(move |listed| {
            listed
                .custom
                .iter()
                .find(|pin| pin.name == name)
                .map(|pin| presets::removed(pin, &listed.default))
                .unwrap_or_default()
        })
        .await;
    }

    /// 照这一刻的配置文件算出要改的几项、写进去；写成了让开着的会话换上重读的配置，再交一次列表和会话钉着的那一个。
    /// 写不成的交 [`Update::Failed`]。
    async fn write(&mut self, edits: impl FnOnce(&Listed) -> Vec<Edit> + Send + 'static) {
        if self.rescued() {
            return;
        }
        let written = lm_engine::spawn_blocking(move || {
            let paths = Paths::locate(&Env::current()).map_err(|error| error.to_string())?;
            let edits = edits(&presets::read(&paths));
            Settings::new(paths, |name| std::env::var(name).ok())
                .set(&edits)
                .map(|_| ())
                .map_err(|error| error.to_string())
        })
        .await
        .unwrap_or_else(|error| Err(error.to_string()));
        let reloaded = written.and_then(|()| match &self.ready {
            Some(ready) => ready.reconfigure(),
            None => Ok(()),
        });
        if let Err(why) = reloaded {
            self.report(Update::Failed(why));
        }
        self.presets().await;
        self.tell_preset();
    }

    /// 告诉界面正在说话的会话钉着哪个预设：开了会话、打开了旧会话、配置换了以后各说一次。自己定义的和这一刻配置里同名的
    /// 那一张比：改过了、没有了的照实说。没有会话的、应急模式的不说。
    pub(super) fn tell_preset(&self) {
        let (Some(session), Some(ready)) = (&self.session, &self.ready) else {
            return;
        };
        if ready.is_rescue() {
            return;
        }
        let Some(chosen) = session.preset() else {
            return;
        };
        let state = match chosen {
            Chosen::Full | Chosen::Bare => PinState::Same,
            Chosen::Custom(pin) => match presets::choose(&ready.values(), &pin.name) {
                Some(Chosen::Custom(now)) if now == *pin => PinState::Same,
                Some(_) => PinState::Changed,
                None => PinState::Gone,
            },
        };
        self.report(Update::Preset(Pinned {
            name: chosen.name().to_string(),
            state,
        }));
    }

    /// 多记一项被忽略的配置项 `key`（默认预设写的名字现在没有，照全部功能开了）：第一句话以后连同别的一起提示；这一次运行
    /// 已经提示过的当场另交一条。
    pub(super) fn note_ignored(&mut self, key: &str) {
        let noted = self
            .ready
            .as_mut()
            .is_some_and(|ready| ready.note_ignored(key));
        if !noted {
            self.report(Update::Ignored(lm_engine::checkup::Ignored {
                settings: vec![key.to_string()],
                ..lm_engine::checkup::Ignored::default()
            }));
        }
    }
}
