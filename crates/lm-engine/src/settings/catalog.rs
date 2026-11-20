//! 配置界面要的目录（Miyu `provider.catalog`，`miyu-endpoint/src/providers.rs`）：常用的几家、目录和档案里的每一家、一家的
//! 模型。
//!
//! 整份目录不常驻（`docs/00-设计.md` 第五节第 4 条）：界面要列目录时 [`Catalog::load`] 读一份缓存目录里取来的，界面用完放掉，
//! 放的时候把空着的堆还给系统。平常的对话不碰这里。
//!
//! 程序不带目录（LM-9）：缓存里还没有、读不了的，读出来是空的目录（[`Catalog::is_empty`]），照样能用——常用的几家、全目录
//! 列不出一家（档案里认得的几家也不列：没有目录时不给预设），自定义的一家照配置推驱动、地址（档案照用），试、列模型都照常；
//! 模型只列配置里写了的、供应商列出来的，资料照配置写的。目录怎么取在 [`crate::models::refresh`]。

use std::collections::BTreeMap;

use lm_kernel::config::Values;
use lm_kernel::config::merge::Resolved;
use lm_kernel::models::catalog::{CatalogLookup, CatalogModel, CatalogSource, Loaded};
use lm_kernel::models::facts::facts;
use lm_kernel::models::matching::{Vendors, recognize};
use lm_kernel::models::observed::{Learned, ProviderList};
use lm_kernel::models::onboard::{self, Listed, Offered};
use lm_kernel::models::profile::Profiles;
use lm_kernel::models::{Knowledge, provider};

use crate::models::catalog::{Places, load_again};
use crate::store::paths::Paths;

/// 常用的几家怎么列（施工 8-11 再补）。
const FEATURED: &str = include_str!("../../../../resources/models/featured.toml");

/// 读好的一份目录，和档案、认原厂的表。
pub struct Catalog {
    loaded: Loaded,
    /// 缓存里读到了目录（LM-9）：没读到的 `loaded` 是空的。
    have: bool,
    profiles: Profiles,
    vendors: Vendors,
}

impl std::fmt::Debug for Catalog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Catalog")
            .field("providers", &self.loaded.catalog.providers().count())
            .finish_non_exhaustive()
    }
}

/// 列不出模型时拿来试一家的那个模型（[`Catalog::trial`]）。
#[derive(Debug, Clone)]
pub struct Trial {
    /// 模型名。
    pub model: String,
    /// 发给它时走的那一家：照这个模型的驱动（[`provider::Provider::for_model`]）。
    pub speaking: provider::Provider,
    /// 这个模型配置的默认温度。
    pub temperature: Option<f64>,
}

/// 目录里的一个模型，界面上列的那几格。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Model {
    /// 模型名（照供应商那边的叫法）。
    pub id: String,
    /// 显示名；没有的没有。
    pub name: Option<String>,
    /// 窗口，单位 token。
    pub window: Option<u64>,
    /// 能看图。
    pub image: bool,
    /// 发布日期（`2026-09-10`）。
    pub released: Option<String>,
}

impl Catalog {
    /// 读一份：缓存目录（`paths` 的模型缓存）里取来的。缓存里没有、读不了的是空的目录（LM-9）。在阻塞的地方调：读、解目录要
    /// 几十毫秒。
    ///
    /// # Errors
    ///
    /// 出厂的档案读不进来（安装坏了）。
    pub fn load(paths: &Paths) -> Result<Catalog, String> {
        let (profiles, vendors) = crate::models::shipped()?;
        let places = Places {
            cache: Some(paths.model_cache()),
        };
        let loaded = load_again(&places);
        Ok(Catalog {
            have: loaded.is_some(),
            loaded: loaded.unwrap_or_else(|| Loaded {
                catalog: lm_kernel::models::catalog::Catalog::default(),
                source: CatalogSource::Cache,
                fetched: String::new(),
            }),
            profiles,
            vendors,
        })
    }

    /// 刚取到的一份 `loaded`（配置界面在后台取的，LM-9，[`crate::models::refresh::fetch_now`]）：不用再读一遍缓存。
    ///
    /// # Errors
    ///
    /// 出厂的档案读不进来（安装坏了）。
    pub fn with(loaded: Loaded) -> Result<Catalog, String> {
        let (profiles, vendors) = crate::models::shipped()?;
        Ok(Catalog {
            have: true,
            loaded,
            profiles,
            vendors,
        })
    }

    /// 缓存里没有目录（LM-9：还没取到、取不到）：列不出一家，只能自定义。
    pub fn is_empty(&self) -> bool {
        !self.have
    }

    /// 目录和档案里的每一家（[`onboard::listed`]），照编号排。没有目录的一家都不列（LM-9：档案里认得的几家不当预设）。
    pub fn providers(&self) -> Vec<Listed> {
        if !self.have {
            return Vec::new();
        }
        let learned = Learned::default();
        let lists = std::collections::BTreeMap::new();
        let knowledge = Knowledge {
            profiles: &self.profiles,
            vendors: &self.vendors,
            catalog: Some(&self.loaded),
            lookup: Some(CatalogLookup::Full(&self.loaded)),
            learned: &learned,
            lists: &lists,
        };
        onboard::listed(&knowledge)
    }

    /// 常用的几家，照 `featured.toml` 的先后，名字照 `language` 挑；目录、档案里没有的不列，没有目录的一家都不列。
    ///
    /// # Errors
    ///
    /// `featured.toml` 写错了（安装坏了，测试守着出厂的那份）。
    pub fn featured(&self, language: &str) -> Result<Vec<Listed>, String> {
        let wanted = onboard::featured(FEATURED)?;
        let listed = self.providers();
        Ok(wanted
            .iter()
            .filter_map(|one| {
                let id = one.id(language);
                let mut found = listed.iter().find(|entry| entry.id == id)?.clone();
                if let Some(name) = one.name.pick(language) {
                    found.name = name.to_string();
                }
                Some(found)
            })
            .collect())
    }

    /// 配置 `values` 里编号 `id` 那一家这时的样子（驱动、地址、key 的引用），照档案、目录推，和会话的路由同一个
    /// [`provider::provider`]：配置界面试一家通不通时用。
    ///
    /// # Errors
    ///
    /// 配置里没有这一家；推不出地址。
    pub fn provider(
        &self,
        values: &Values,
        id: &str,
    ) -> Result<provider::Provider, provider::NoModel> {
        let (learned, lists) = (Learned::default(), BTreeMap::new());
        provider::provider(values, &self.knowledge(&learned, &lists), id)
    }

    /// 列不出模型时拿哪个模型试 `provider` 这一家（Miyu `provider.test` 第 4 步，`miyu-session/src/route/probe.rs` 的
    /// `catalog_models`、`pick`）：配置 `values` 里这一家写了模型的（连同主对话 `models.chat` 指到它的那个）照写的挑，没写的
    /// 照目录里对上的那一家的模型；照推荐挑
    /// （[`onboard::recommend`]：能调工具、窗口够、不是停用的里面最新的，都不够格的取第一个）。一个都挑不出的是 `None`；
    /// 挑出来了、这个模型要的驱动还没有的是 `Some(Err)`。只读。
    pub fn trial(
        &self,
        values: &Values,
        resolved: &Resolved,
        provider: &provider::Provider,
    ) -> Option<Result<Trial, provider::NoModel>> {
        let (learned, lists) = (Learned::default(), BTreeMap::new());
        let knowledge = self.knowledge(&learned, &lists);
        let id = provider.id.as_str();
        let mut models =
            lm_kernel::config::key::names(values.keys(), "providers.<id>.models.<model>", &[id]);
        // 主对话指到这一家的那个模型也算写了的（`[providers.x.models."m"]` 什么都没写时配置里没有它的键）。
        let chat = provider::chat(values);
        if let Some(model) = chat
            .as_deref()
            .and_then(|chat| chat.strip_prefix(&format!("{id}/")))
            && !models.iter().any(|written| written == model)
        {
            models.push(model.to_string());
        }
        if models.is_empty() {
            models = provider
                .recognized
                .as_ref()
                .and_then(|found| self.loaded.catalog.provider(&found.provider))
                .map(|entry| entry.models.keys().map(ToString::to_string).collect())
                .unwrap_or_default();
        }
        let offered: Vec<Offered> = models
            .iter()
            .map(|model| {
                let (facts, found) = facts(resolved, &knowledge, provider, model);
                Offered::of(model, &facts, onboard::released(Some(&self.loaded), &found))
            })
            .collect();
        let model = onboard::recommend(&offered)?.to_string();
        Some(self.trial_model(resolved, provider, &model))
    }

    /// 拿指定的 `model` 试 `provider` 这一家（Miyu `provider.test` 带 `model`：焦点在模型栏时试选中的那个）。这个模型要的驱动
    /// 还没有的是 `Err`。只读。
    ///
    /// # Errors
    ///
    /// 这个模型要的驱动还没有。
    pub fn trial_model(
        &self,
        resolved: &Resolved,
        provider: &provider::Provider,
        model: &str,
    ) -> Result<Trial, provider::NoModel> {
        let (learned, lists) = (Learned::default(), BTreeMap::new());
        let knowledge = self.knowledge(&learned, &lists);
        let (facts, _) = facts(resolved, &knowledge, provider, model);
        let speaking = provider.for_model(model, &facts.wire, &self.profiles.npm)?;
        Ok(Trial {
            model: model.to_string(),
            speaking,
            temperature: facts.temperature.value,
        })
    }

    /// 配好的 `id` 那一家在配置界面「供应商和模型」那一页列的模型（Miyu `model.list` 的一家）：配置里写了的（连同主对话指到
    /// 它的那个），这一趟试出来的 `listed`，目录里对上的那一家的（新发布的在前）；一个名字只列一次，每个带资料。推不出这一家
    /// 的（没地址这类）照写了的、试出来的列，不带资料，原话放在 `problem`。只读。
    pub fn offer(
        &self,
        values: &Values,
        resolved: &Resolved,
        id: &str,
        listed: &[String],
    ) -> Offer {
        let (learned, lists) = (Learned::default(), BTreeMap::new());
        let knowledge = self.knowledge(&learned, &lists);
        let found = provider::provider(values, &knowledge, id);
        let mut names: Vec<(String, &'static str)> = Vec::new();
        let mut push = |name: String, from: &'static str| {
            if !names.iter().any(|(have, _)| *have == name) {
                names.push((name, from));
            }
        };
        for name in
            lm_kernel::config::key::names(values.keys(), "providers.<id>.models.<model>", &[id])
        {
            push(name, "config");
        }
        if let Some(chat) = provider::chat(values)
            && let Some(model) = chat.strip_prefix(&format!("{id}/"))
        {
            push(model.to_string(), "config");
        }
        for name in listed {
            push(name.clone(), "provider");
        }
        let recognized = found
            .as_ref()
            .ok()
            .and_then(|found| found.recognized.as_ref())
            .map(|recognized| recognized.provider.clone());
        if let Some(vendor) = &recognized {
            for model in self.models(vendor) {
                push(model.id, "catalog");
            }
        }
        let models = names
            .into_iter()
            .map(|(name, from)| {
                let facts = found
                    .as_ref()
                    .ok()
                    .map(|provider| facts(resolved, &knowledge, provider, &name).0);
                OfferedModel { name, from, facts }
            })
            .collect();
        Offer {
            models,
            problem: found.err().map(|missing| missing.0),
        }
    }

    /// 推一家、查模型资料要的手头资料：整份目录、档案、认原厂的表；没学到的、没拉过列表的都是空的。
    fn knowledge<'a>(
        &'a self,
        learned: &'a Learned,
        lists: &'a BTreeMap<String, ProviderList>,
    ) -> Knowledge<'a> {
        Knowledge {
            profiles: &self.profiles,
            vendors: &self.vendors,
            catalog: Some(&self.loaded),
            lookup: Some(CatalogLookup::Full(&self.loaded)),
            learned,
            lists,
        }
    }

    /// 一家配好的供应商在目录里是哪一家：编号、地址、手写的 `catalog` 照 Miyu 的认法（[`recognize`]）。认不出的没有。
    pub fn recognize(
        &self,
        id: &str,
        base_url: Option<&str>,
        written: Option<&str>,
    ) -> Option<String> {
        recognize(&self.loaded.catalog, id, base_url, written).map(|found| found.provider)
    }

    /// 出厂的档案里写了编号 `id` 那一家的地址（DeepSeek、Anthropic、OpenAI 这几家）：没写地址、对上它的一家照档案推得出地址，
    /// 有没有目录都能用（LM-9：从 Miyu 导入没写地址的一家时，目录还没取到也认它）。
    pub fn profiled(&self, id: &str) -> bool {
        self.profiles
            .providers
            .get(id)
            .is_some_and(|profile| profile.base_url.is_some())
    }

    /// 目录里 `provider` 那一家的模型：新发布的在前，一样新的照名字。没有这一家的是空的。
    pub fn models(&self, provider: &str) -> Vec<Model> {
        let Some(entry) = self.loaded.catalog.provider(provider) else {
            return Vec::new();
        };
        let mut models: Vec<Model> = entry
            .models
            .iter()
            .map(|(id, model)| row(id, model))
            .collect();
        models.sort_by(|a, b| b.released.cmp(&a.released).then_with(|| a.id.cmp(&b.id)));
        models
    }
}

impl Drop for Catalog {
    /// 整份目录有几 MB：放掉以后把空着的堆还给系统（同读完目录那一下，`crate::models::begin`）。
    fn drop(&mut self) {
        self.loaded = Loaded {
            catalog: lm_kernel::models::catalog::Catalog::default(),
            source: self.loaded.source,
            fetched: String::new(),
        };
        lm_heap::trim();
    }
}

/// 配置界面列的一家的模型（[`Catalog::offer`]）。
#[derive(Debug, Clone)]
pub struct Offer {
    /// 模型，照写了的、试出来的、目录的先后。
    pub models: Vec<OfferedModel>,
    /// 推不出这一家的原话（没地址这类）；推得出的没有。
    pub problem: Option<String>,
}

/// 列的一个模型。
#[derive(Debug, Clone)]
pub struct OfferedModel {
    /// 模型名（照供应商那边的叫法）。
    pub name: String,
    /// 从哪列出来的：`config`、`provider`（这一趟试出来的）、`catalog`。
    pub from: &'static str,
    /// 资料；推不出这一家的没有。
    pub facts: Option<lm_kernel::models::facts::Facts>,
}

/// 界面上列的一行。
fn row(id: &str, model: &CatalogModel) -> Model {
    Model {
        id: id.to_string(),
        name: model.name.as_deref().map(str::to_string),
        window: model.window,
        image: model.inputs.is_some_and(|inputs| inputs.has("image")),
        released: model.release_date.as_deref().map(str::to_string),
    }
}

#[cfg(test)]
mod tests {
    use super::Catalog;
    use crate::store::paths::Paths;
    use crate::store::test_support::Scratch;

    /// 真目录裁出来的一份（`lm-kernel/testdata`）。
    const TRIMMED: &str =
        include_str!("../../../lm-kernel/testdata/models/models-dev-trimmed.json");

    /// 缓存里取到过目录的：常用的几家里目录有的都在，DeepSeek 有模型，照地址认得出。
    #[test]
    fn a_fetched_catalog_lists_featured_providers_and_their_models() {
        let scratch = Scratch::new();
        let paths = Paths::under(scratch.path());
        std::fs::create_dir_all(paths.model_cache()).expect("建得了");
        std::fs::write(paths.model_cache().join("models-dev.json"), TRIMMED).expect("写得进");
        let catalog = Catalog::load(&paths).expect("读得了");
        assert!(!catalog.is_empty());
        let featured = catalog.featured("zh").expect("featured.toml 写对了");
        assert!(
            featured
                .iter()
                .any(|one| one.id == "deepseek" && one.name == "DeepSeek")
        );
        assert!(catalog.providers().len() > featured.len());
        let models = catalog.models("deepseek");
        assert!(!models.is_empty());
        assert!(
            models
                .windows(2)
                .all(|pair| pair[0].released >= pair[1].released)
        );
        assert!(catalog.models("no-such-provider").is_empty());
        assert_eq!(
            catalog
                .recognize("my-relay", None, Some("deepseek"))
                .as_deref(),
            Some("deepseek")
        );
        assert_eq!(
            catalog.recognize("deepseek", None, None).as_deref(),
            Some("deepseek")
        );
    }

    /// 缓存里没有目录（LM-9，程序不带目录）、缓存里的坏了、半截、一家都没有的：读出来是空的目录，不报错；常用的几家、全目录
    /// 一家都不列（档案里的几家不当预设），认不出、列不出模型。
    #[test]
    fn no_catalog_lists_nothing_and_still_loads() {
        let scratch = Scratch::new();
        let paths = Paths::under(scratch.path());
        let empty = |catalog: &Catalog| {
            assert!(catalog.is_empty());
            assert!(catalog.providers().is_empty(), "档案里的几家不列");
            assert!(catalog.featured("zh").expect("写对了").is_empty());
            assert!(catalog.models("deepseek").is_empty());
            assert_eq!(catalog.recognize("deepseek", None, None), None);
        };
        empty(&Catalog::load(&paths).expect("没有目录也读得了"));
        std::fs::create_dir_all(paths.model_cache()).expect("建得了");
        let half = &TRIMMED[..TRIMMED.len() / 2];
        for broken in ["{not json", half, "{}", "[]", "\u{0}"] {
            std::fs::write(paths.model_cache().join("models-dev.json"), broken).expect("写得进");
            empty(&Catalog::load(&paths).expect("坏了也读得了"));
        }
    }
}
