//! 「供应商和模型」那一页看的数据（Miyu 终端 `settings/data.rs`）：配好的供应商（连同列的模型）、默认用途。Miyu 照核心的
//! `model.list` 读；这里照读到的配置（[`Snapshot`]）和目录（[`Catalog::offer`]）拼，存完重拼。Miyu 编辑窗按 `s` 当场存，
//! 没有攒着的草稿，这里也不带。

use std::collections::BTreeMap;

use lm_engine::settings::Snapshot;
use lm_engine::settings::catalog::Catalog;
use lm_kernel::config::key::fill;
use lm_kernel::config::secret::Reference;
use lm_kernel::config::{Value, Values};
use lm_kernel::models::facts::{Facts, Source};

/// 读来的一份。
#[derive(Debug, Clone, Default)]
pub struct Data {
    /// 配好的供应商，照编号排。
    pub providers: Vec<Provider>,
    /// 默认的文本模型（`models.chat`）。
    pub chat: Option<String>,
    /// 默认的视觉模型（`models.vision`）。
    pub vision: Option<String>,
}

/// 一家供应商。
#[derive(Debug, Clone, Default)]
pub struct Provider {
    /// 编号。
    pub id: String,
    /// 显示名；没写的界面写编号。
    pub name: Option<String>,
    /// 写了的地址：写死的是字，环境变量引用的写成 `env:<变量>`；没写的是 `None`。
    pub base_url: Option<String>,
    /// 写了的驱动；没写的是 `None`（照目录、地址推）。
    pub driver: Option<String>,
    /// 这一家的 key：引用和设没设；没写 key 的是 `None`。
    pub key: Option<(Reference, bool)>,
    /// 模型：这一家报得出列表的就是它列的那些；报不出的是配置里写了的、目录里的（`Catalog::offer`）。
    pub models: Vec<Model>,
    /// 这一家报出过自己的模型列表（存下来的、这一趟拉到的），`models` 就是照它列的。
    pub reported: bool,
    /// 推不出这一家的原话（没地址这类）。
    pub problem: Option<String>,
}

/// 一个模型。
#[derive(Debug, Clone, Default)]
pub struct Model {
    /// 模型名（照供应商那边的叫法，可能带 `组织/`）。
    pub name: String,
    /// 引用：`供应商/模型`。
    pub reference: String,
    /// 从哪列出来的：`config`、`provider`、`catalog`。
    pub listed: &'static str,
    /// 资料；推不出这一家的没有。
    pub facts: Option<Facts>,
}

impl Data {
    /// 照 `snapshot` 拼：每家的模型照 `catalog`（没有目录的只列配置里写了的）；`listed` 是供应商列出来的模型名（存下来的、
    /// 这一趟拉到的），照编号。
    pub fn read(
        snapshot: &Snapshot,
        catalog: Option<&Catalog>,
        listed: &BTreeMap<String, Vec<String>>,
    ) -> Data {
        let values = snapshot.resolved.values();
        let text = |key: &str| match snapshot.written.get(key) {
            Some(Value::Text(text)) => Some(text.to_string()).filter(|t| !t.is_empty()),
            _ => None,
        };
        let providers = lm_kernel::models::provider::configured(&values)
            .into_iter()
            .map(|id| {
                let item = |leaf: &str| fill(&format!("providers.<id>.{leaf}"), &[&id]);
                let key = match snapshot.written.get(&item("key")) {
                    Some(Value::Secret(reference)) => {
                        let set = match reference {
                            Reference::Secret(name) => snapshot.secrets.contains(name),
                            Reference::Env(_) => true,
                        };
                        Some((reference.clone(), set))
                    }
                    _ => None,
                };
                let base_url = match snapshot.written.get(&item("base_url")) {
                    Some(Value::Text(url)) => Some(url.to_string()),
                    Some(Value::Secret(Reference::Env(name))) => Some(format!("env:{name}")),
                    Some(other) => Some(other.toml()),
                    None => None,
                };
                let empty = Vec::new();
                let tried = listed.get(&id).unwrap_or(&empty);
                let (models, problem) = match catalog {
                    Some(catalog) => {
                        let offer = catalog.offer(&values, &snapshot.resolved, &id, tried);
                        let models = offer
                            .models
                            .into_iter()
                            .map(|one| Model {
                                reference: format!("{id}/{}", one.name),
                                name: one.name,
                                listed: one.from,
                                facts: one.facts,
                            })
                            .collect();
                        (models, offer.problem)
                    }
                    None => (written_models(&values, &id), None),
                };
                Provider {
                    reported: !tried.is_empty(),
                    name: text(&item("name")),
                    driver: text(&item("driver")),
                    base_url,
                    key,
                    models,
                    problem,
                    id,
                }
            })
            .collect();
        Data {
            providers,
            chat: text("models.chat"),
            vision: text("models.vision"),
        }
    }

    /// 照编号找供应商。
    pub fn provider(&self, id: &str) -> Option<&Provider> {
        self.providers.iter().find(|p| p.id == id)
    }

    /// 照引用找模型（`供应商/模型`）。
    pub fn model(&self, reference: &str) -> Option<(&Provider, &Model)> {
        self.providers.iter().find_map(|p| {
            p.models
                .iter()
                .find(|m| m.reference == reference)
                .map(|m| (p, m))
        })
    }
}

/// 没有目录时一家列的模型：配置里写了的。
fn written_models(values: &Values, id: &str) -> Vec<Model> {
    lm_kernel::config::key::names(values.keys(), "providers.<id>.models.<model>", &[id])
        .into_iter()
        .map(|name| Model {
            reference: format!("{id}/{name}"),
            name,
            listed: "config",
            facts: None,
        })
        .collect()
}

impl Provider {
    /// 界面上写的名字：显示名，没有的写编号。
    pub fn shown(&self) -> &str {
        self.name.as_deref().unwrap_or(&self.id)
    }
}

impl Model {
    /// 上下文窗口。
    pub fn window(&self) -> Option<u64> {
        self.facts.as_ref().and_then(|f| f.window.value)
    }

    /// 能收哪几种（`text`、`image`、`pdf`……）。
    pub fn inputs(&self) -> Vec<String> {
        self.facts
            .as_ref()
            .map_or_else(|| vec!["text".to_string()], |f| f.inputs.value.clone())
    }

    /// 能收图。
    pub fn sees(&self) -> bool {
        self.inputs().iter().any(|i| i == "image")
    }

    /// 只在配置里写着（`n` 加的）：只有这种能删。
    pub fn custom(&self) -> bool {
        self.listed == "config"
    }
}

/// 一格资料从哪来，写成 Miyu 的来源名：`config:system`、`provider`、`catalog`、`default`。
pub fn from(source: &Source) -> &'static str {
    match source {
        Source::Config { .. } => "config:system",
        Source::Learned { .. } | Source::Provider { .. } => "provider",
        Source::Catalog { .. } => "catalog",
        Source::Default => "default",
    }
}
