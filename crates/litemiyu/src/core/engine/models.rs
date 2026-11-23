//! `/model`、`/effort` 要的（Miyu 核心 `model.list` 的 `miyu-endpoint/src/models/entry.rs`，终端 `core/models.rs` 的
//! `choices`）：配好的几家各有哪些模型（配置里写的、供应商列过的、目录里那一家的）、能不能选；一个模型有哪几档思考程度。
//! 换模型（内核的 `Configure`）、换思考程度（`SetEffort`）、把选的记成新会话的默认（`models.chat`）也在这里。
//!
//! 整份目录不常驻：开 `/model` 时临时借一份（`ModelData::hold`，在阻塞线程里），排好就放掉、把堆还给系统。平常的对话不碰。

use std::collections::BTreeSet;
use std::sync::Arc;

use lm_engine::session::{ConfigSource, ModelData};
use lm_engine::settings::{Edit, Settings};
use lm_kernel::config::Value;
use lm_kernel::config::merge::Resolved;
use lm_kernel::kernel::session::Command as Kernel;
use lm_kernel::models::facts::facts;
use lm_kernel::models::provider;

use super::Engine;
use crate::core::{Choice, ChoiceState, Update};

/// 新会话默认的聊天模型那一项。
const CHAT: &str = "models.chat";

impl Engine {
    /// 列 `/model` 的一行行，交 [`Update::Choices`]。引擎还没起的先起（读配置、起读目录的任务）；起不了的交空的。
    pub(super) async fn choices(&mut self) {
        let Some((source, data, cwd)) = self.model_parts() else {
            return self.report(Update::Choices(Vec::new()));
        };
        data.wait().await;
        let listed = tokio::task::spawn_blocking(move || {
            let resolved = source.with_project(&cwd);
            let listed = list(&resolved, source.as_ref(), &data);
            lm_heap::trim();
            listed
        })
        .await
        .unwrap_or_default();
        self.report(Update::Choices(listed));
    }

    /// 换会话的模型（[`crate::core::Command::Configure`]，同命令行线）：还没开会话的先开一个（开不成的照「没发出去」报），被拒的
    /// 照原因码报。`/model` 只在开着会话时发它。
    pub(super) async fn configure(&mut self, model: String) {
        if self.session.is_none()
            && let Err(message) = self.open().await
        {
            self.report(Update::Unsent {
                reason: None,
                message,
            });
            return;
        }
        let outcome = self.ask_main(Kernel::Configure { model }).await;
        self.refused(outcome);
    }

    /// 引用 `reference`（模型 `<供应商>/<模型>`；`None` 照默认的聊天模型）有哪几档思考程度，交 [`Update::Efforts`]；池、解析
    /// 不出的一档都没有。
    pub(super) async fn efforts(&mut self, reference: Option<String>) {
        let Some((source, data, cwd)) = self.model_parts() else {
            return self.report(Update::Efforts {
                reference: reference.unwrap_or_default(),
                levels: Vec::new(),
            });
        };
        data.wait().await;
        let (reference, levels) = tokio::task::spawn_blocking(move || {
            let resolved = source.with_project(&cwd);
            let reference = reference.unwrap_or_else(|| match resolved.values().get(CHAT) {
                Some(Value::Text(chat)) => chat.to_string(),
                _ => String::new(),
            });
            let levels = data.with(|knowledge| {
                lm_kernel::models::effort::levels_of(&resolved, knowledge, &reference)
            });
            (reference, levels)
        })
        .await
        .unwrap_or_default();
        self.report(Update::Efforts { reference, levels });
    }

    /// 换这个会话的思考程度（`None` 回到跟着配置）：还没开会话的记着，开了会话再交（同 Miyu）。成了交
    /// [`Update::EffortSet`]。
    pub(super) async fn set_effort(&mut self, level: Option<String>) {
        if self.session.is_none() {
            self.pending_effort = Some(level.clone());
            return self.report(Update::EffortSet(level));
        }
        let outcome = self
            .ask_main(Kernel::SetEffort {
                level: level.clone(),
            })
            .await;
        match outcome {
            Some(lm_kernel::kernel::session::Outcome::Rejected { .. }) | None => {
                self.refused(outcome);
            }
            Some(_) => self.report(Update::EffortSet(level)),
        }
    }

    /// 开了会话以后交上还没开时选的思考程度（[`Engine::set_effort`]）。
    pub(super) async fn hand_pending_effort(&mut self) {
        if let Some(level) = self.pending_effort.take() {
            let outcome = self.ask_main(Kernel::SetEffort { level }).await;
            self.refused(outcome);
        }
    }

    /// 把 `reference` 记成新会话默认的聊天模型（Miyu `/model` 手动换的也记成新会话的默认，2026-10-02 项目主人定）：只改
    /// `models.chat` 那一行，注释、先后不动；写成了让开着的会话换上重读的配置。写不成的交 [`Update::Failed`]。
    ///
    /// 应急模式（设计第五节第 16 条）什么都不写：只记在引擎里，这一次运行里以后开的会话照它开（[`Engine::open`]）。
    pub(super) async fn chat_default(&mut self, reference: String) {
        if self.rescue.is_some() || self.ready.as_ref().is_some_and(|ready| ready.is_rescue()) {
            self.rescue_chat = Some(reference);
            return;
        }
        let Some(ready) = &self.ready else {
            return;
        };
        let paths = ready.paths().clone();
        let written = tokio::task::spawn_blocking(move || {
            let settings = Settings::new(paths, |name| std::env::var(name).ok());
            settings
                .set(&[Edit::Set(CHAT.to_string(), Value::Text(reference.into()))])
                .map(|_| ())
                .map_err(|error| error.to_string())
        })
        .await
        .unwrap_or_else(|error| Err(error.to_string()));
        let reloaded = written.and_then(|()| ready.reconfigure());
        if let Err(why) = reloaded {
            self.report(Update::Failed(why));
        }
    }

    /// 读配置、查目录要的几样：引擎没起的先起。
    fn model_parts(&mut self) -> Option<(Arc<dyn ConfigSource>, Arc<ModelData>, String)> {
        if self.ready.is_none() {
            match super::setup::Ready::new(self.rescue.take(), self.unattended) {
                Ok(ready) => self.ready = Some(ready),
                Err(why) => {
                    self.report(Update::Failed(why));
                    return None;
                }
            }
        }
        let ready = self.ready.as_ref()?;
        let cwd = super::here().unwrap_or_default();
        Some((ready.source(), ready.model_data(), cwd))
    }
}

/// 排出 `/model` 的一行行：每一家的模型，照 Miyu 终端 `core/models.rs` 的 `choices`（LM-8 砍了模型池，原来排在最前的池那几行没了）。
///
/// 列哪些（LM-47、LM-50，Miyu 核心 FX-18、FX-19，2026-11-22 项目主人：「已下架的模型就不该出现在列表里，上游的模型列表是
/// 什么样子显示就应该是什么样子」）：
///
/// - 供应商报得出自己的模型列表的（手上有它列过的、不是空的），就照它的列表列，一个不多一个不少：配置里只写了资料（窗口、
///   价格这类）、目录里对上这一家的模型，列表里没有的都不列；目录、配置只用来补窗口、价格这些资料。正在用的（新会话默认的
///   聊天模型、看图的模型）也不例外（LM-47 给它们留过例外、标「供应商已经没有这个模型」，LM-50 去掉了）：配置的值不在列表
///   里时，由显示这个配置值的地方（配置界面「默认模型」一页）提示。
/// - 报不出列表的（没有 `/models`、没拉到过）照旧：配置里写的、正在用的、目录里的都列。
pub fn list(resolved: &Resolved, source: &dyn ConfigSource, data: &ModelData) -> Vec<Choice> {
    let values = resolved.values();
    let mut out = Vec::new();
    let holding = data.hold();
    for id in provider::configured(&values) {
        let no_key = match values.get(&format!("providers.{id}.key")) {
            Some(Value::Secret(reference)) => source
                .secret(reference)
                .is_none_or(|secret| secret.expose().is_empty()),
            _ => false,
        };
        data.with_holding(&holding, false, |knowledge| {
            let Ok(found) = provider::provider(&values, knowledge, &id) else {
                return;
            };
            let mut listed: BTreeSet<String> = BTreeSet::new();
            let offered = knowledge
                .lists
                .get(&id)
                .filter(|list| !list.models.is_empty());
            match offered {
                Some(list) => {
                    for model in &list.models {
                        listed.insert(model.id.clone());
                    }
                }
                None => {
                    listed.extend(written_models(&values, &id));
                    listed.extend(used_models(&values, &id));
                    let catalog = found
                        .recognized
                        .as_ref()
                        .zip(knowledge.lookup.as_ref())
                        .and_then(|(recognized, lookup)| lookup.listing(&recognized.provider));
                    if let Some(entry) = &catalog {
                        listed.extend(entry.models.keys().cloned());
                    }
                }
            }
            for model in listed {
                let (facts, _) = facts(resolved, knowledge, &found, &model);
                let reference = format!("{id}/{model}");
                let state = if no_key {
                    ChoiceState::NoKey
                } else {
                    ChoiceState::Ok
                };
                out.push(row(&id, &model, &reference, &facts, state));
            }
        });
    }
    out
}

/// 一个模型那一行：名字写配置里起的、没起的写模型名；暗字写供应商（起了名字的写完整的引用）和窗口（Miyu 终端
/// `core/models.rs`，2026-10-26、10-27 项目主人）。
fn row(
    id: &str,
    model: &str,
    reference: &str,
    facts: &lm_kernel::models::facts::Facts,
    state: ChoiceState,
) -> Choice {
    let named = matches!(
        facts.name.source,
        lm_kernel::models::facts::Source::Config { .. }
    )
    .then(|| facts.name.value.clone());
    let shown = match named {
        None => id.to_string(),
        Some(_) => reference.to_string(),
    };
    let detail = match facts.window.value {
        Some(window) => format!(
            "{shown}{}{}",
            crate::sheet::sep(),
            crate::meter::short(window)
        ),
        None => shown,
    };
    let name = named.unwrap_or_else(|| model.to_string());
    Choice {
        reference: reference.to_string(),
        name: crate::tabs::printable(&name).into_owned(),
        detail: crate::tabs::printable(&detail).into_owned(),
        state,
    }
}

/// 配置里写了资料的那一家的模型：`providers.<id>.models.<model>`。
fn written_models(values: &lm_kernel::config::Values, id: &str) -> Vec<String> {
    lm_kernel::config::key::names(values.keys(), "providers.<id>.models.<model>", &[id])
}

/// 正在用的那一家的模型：新会话默认的聊天模型、看图的模型引用到它的。
fn used_models(values: &lm_kernel::config::Values, id: &str) -> Vec<String> {
    let prefix = format!("{id}/");
    ["models.chat", "models.vision"]
        .into_iter()
        .filter_map(|key| match values.get(key) {
            Some(Value::Text(text)) => text.strip_prefix(&prefix).map(str::to_string),
            _ => None,
        })
        .collect()
}
