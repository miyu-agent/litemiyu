//! 试一家供应商通不通（Miyu 终端 `settings/provider_test.rs` 的 `r`）的界面那一半：推一家、发、认都在引擎
//! （[`lm_engine::providers::probe`]），这里只管在自己的线程里发、状态行写「正在测试」、`Esc` 停、
//! 收到结果写一句、把列出来的模型名交给「供应商和模型」那一页。
//!
//! 界面不等它：结果放进通道，界面的主循环试着的时候照节拍醒来收（`app/deadline.rs`）。key 只进请求头，不进任何一句给人看的
//! 话、日志；出错的原话引擎已经去掉地址。

use std::sync::mpsc;
use std::time::Duration;

use lm_engine::providers::probe::{self, Outcome};
use tokio::sync::oneshot;

use super::texts::fill;
use super::{Settings, Tone};

/// 默认最多等多久：从发出到读完。
pub const WAIT: Duration = Duration::from_secs(20);

/// 正在试的一家。
#[derive(Debug)]
pub struct Running {
    /// 给人看的名字。
    pub name: String,
    /// 放下它就叫停。
    _cancel: oneshot::Sender<()>,
    /// 哪一家。
    id: String,
    /// 人点名的模型（焦点在模型上、开头里选好的主对话模型）：结果照它报这个模型能不能用。
    model: Option<String>,
    /// 结果，和列出来的模型名。
    result: mpsc::Receiver<(Outcome, Vec<String>)>,
}

impl Settings {
    /// 试编号 `id` 那一家（供应商页按 `r`）：`model` 是焦点在模型栏时选中的那个，列不出模型时拿它发一句；没有的照配置、
    /// 目录挑。已经在试的先停掉。
    pub(super) fn probe(&mut self, id: &str, model: Option<String>) {
        let name = self.provider_name(id);
        let Some(catalog) = self.catalog() else {
            return;
        };
        let env = |name: &str| std::env::var(name).ok();
        let target = probe::target(
            &self.service,
            &self.snapshot,
            &catalog,
            id,
            model.as_deref(),
            &env,
        );
        drop(catalog);
        let target = match target {
            Ok(target) => target,
            Err(why) => {
                self.say_probed(&name, model.as_deref(), &Outcome::Config(why));
                return;
            }
        };
        self.probe_target(id, name, model, target, true);
    }

    /// 照推好的 `target` 在自己的线程里试（`store`：列到的模型名存下；开头里试的是还没存的一家，选好模型存完才存名字）。
    pub(super) fn probe_target(
        &mut self,
        id: &str,
        name: String,
        model: Option<String>,
        target: probe::Target,
        store: bool,
    ) {
        let (cancel, cancelled) = oneshot::channel();
        let (sender, result) = mpsc::channel();
        let wait = self.wait;
        let paths = self.service.paths().clone();
        let listing = id.to_string();
        let spawned = std::thread::Builder::new()
            .name("probe".to_string())
            .spawn(move || {
                let outcome = probe::run(&target, wait, cancelled);
                // 列到了的存下（同 Miyu `provider.test` 的 `save`，只记名字）：在这条线程里写，界面不等盘。存不进的下次再列。
                if let Some((_, names)) = &outcome
                    && store
                    && !names.is_empty()
                    && lm_engine::providers::lists::store(&paths, &listing, names).is_err()
                {
                }
                // 界面已经不等了的（停了、关了配置界面）不用交。
                if let Some(outcome) = outcome
                    && sender.send(outcome).is_err()
                {}
            });
        if let Err(error) = spawned {
            self.say_probed(&name, model.as_deref(), &Outcome::Other(error.to_string()));
            return;
        }
        let said = self.texts.models.status("testing").replace("{name}", &name);
        self.status = Some((said, Tone::Busy));
        self.probing = Some(Running {
            id: id.to_string(),
            model,
            name,
            _cancel: cancel,
            result,
        });
    }

    /// 在试的停掉（`Esc`）：交回停没停。
    pub(super) fn stop_probe(&mut self) -> bool {
        let Some(running) = self.probing.take() else {
            return false;
        };
        let said = self
            .texts
            .models
            .status("stopped")
            .replace("{name}", &running.name);
        self.status = Some((said, Tone::Bad));
        self.opening_probe_stopped();
        true
    }

    /// 主循环醒来时收一下：试完了的写状态行，交回收到了没有。
    pub fn poll_probe(&mut self) -> bool {
        let Some(running) = &self.probing else {
            return false;
        };
        let (outcome, names) = match running.result.try_recv() {
            Ok(got) => got,
            Err(mpsc::TryRecvError::Empty) => return false,
            Err(mpsc::TryRecvError::Disconnected) => {
                (Outcome::Other("stopped".to_string()), Vec::new())
            }
        };
        let (id, name, model) = (
            running.id.clone(),
            running.name.clone(),
            running.model.clone(),
        );
        self.probing = None;
        // 开头里试的（接模型那一步）：结果归它，不写状态行（同 Miyu 引导写在表单下面）。
        if self.opening_tested(&outcome, &names) {
            return true;
        }
        self.say_probed(&name, model.as_deref(), &outcome);
        self.list_got(&id, names);
        true
    }

    /// 结果写进状态行（Miyu `provider_test.rs` 的 `tested`）：没点名模型的，通了「X 连得上，已获取 N 个模型」「X 连接成功：模型」，
    /// 没通「X 连接失败：为什么」；点名了模型 `chosen` 的，主结论是这个模型能不能用（「X：模型 可用，回话用了 1.2 秒 · 列出 N 个
    /// 模型」「X：模型 不可用：为什么」，项目主人 2026-11-14 定）。为什么照 LiteMiyu 认出来的那一种说。
    fn say_probed(&mut self, name: &str, chosen: Option<&str>, outcome: &Outcome) {
        let words = &self.texts.models;
        let (said, tone) = match outcome {
            Outcome::Works { model, ms, listed } => {
                let secs = format!("{:.1}", *ms as f64 / 1000.0);
                let mut said = words
                    .status("model_works")
                    .replace("{model}", model)
                    .replace("{secs}", &secs);
                if let Some(n) = listed {
                    said.push_str(&words.counted("also_listed", *n));
                }
                (said, Tone::Good)
            }
            Outcome::Listed(n) => (words.counted("tested", *n), Tone::Good),
            Outcome::Spoke(tried) => (
                words.status("tested_request").replace("{model}", tried),
                Tone::Good,
            ),
            failed => {
                let why = self.probe_why_for(failed, chosen.is_some());
                let said = match chosen {
                    Some(chosen) => words
                        .status("model_failed")
                        .replace("{model}", chosen)
                        .replace("{why}", &why),
                    None => words.status("test_failed").replace("{why}", &why),
                };
                (said, Tone::Bad)
            }
        };
        self.status = Some((said.replace("{name}", name), tone));
    }
}

impl Settings {
    /// 没通的原因写给人看（导入、存一家以后在后台拉列表没成时也照它说）。
    pub(super) fn probe_why(&self, failed: &Outcome) -> String {
        self.probe_why_for(failed, false)
    }

    /// 同 [`Settings::probe_why`]；`chosen` 是点名了模型：那个模型发不通的原话（带状态码）就是原因，不说「列不出模型」。
    fn probe_why_for(&self, failed: &Outcome, chosen: bool) -> String {
        let texts = &self.texts.probe;
        let (template, field, model) = match failed {
            // 点名的模型发不通：原话（带状态码）就是原因，不再说「列不出模型」。
            Outcome::Request { model, why } if chosen => {
                (&texts.other, why.clone(), model.as_str())
            }
            Outcome::Request { model, why } => (&texts.request, self.why(why), model.as_str()),
            Outcome::Auth(status) => (&texts.auth, status.to_string(), ""),
            Outcome::Status(status) => (&texts.status, status.to_string(), ""),
            Outcome::Timeout(secs) => (&texts.timeout, secs.to_string(), ""),
            Outcome::Connect(why) => (&texts.connect, self.why(why), ""),
            Outcome::Garbled => (&texts.garbled, String::new(), ""),
            Outcome::Cut => (&texts.cut, String::new(), ""),
            Outcome::BadUrl => (&texts.bad_url, String::new(), ""),
            Outcome::BadKey => (&texts.bad_key, String::new(), ""),
            Outcome::Config(why) => (&texts.config, self.why(why), ""),
            Outcome::NotFound => (&texts.not_found, String::new(), ""),
            Outcome::Other(why) | Outcome::Spoke(why) => (&texts.other, self.why(why), ""),
            Outcome::Listed(_) | Outcome::Works { .. } => (&texts.other, String::new(), ""),
        };
        fill(template, &[("n", &field), ("model", model)])
    }
}

#[cfg(test)]
mod tests;
