//! 引用的写法（`docs/blueprint/models.md`「两种写法」，施工 8-6）：凡是要指定模型的地方怎么认一个引用，解析到一个端点。
//!
//! 在第一个 `/` 处切开，前面是供应商的编号、后面是模型名（模型名里还能有 `/`），两边都不能是空的；别的是错。供应商的编号照
//! 「路径里的名字」的写法，模型名照「短名字」的写法（`crate::config::key`），和配置里 `providers.<id>`、`models."<model>"`
//! 那两段一样。原来还有第二种 `@池`（施工 8-8），LM-8 砍了模型池：以前的 `@<池>` 照写法不对报（8-8 补去掉的挡位名也是）。
//!
//! 解析到端点（施工 8-8，「怎么走」第三条第 1 条，照这一回合冻结的配置）：配置里有那一家就算（模型名不查：供应商的列表
//! 不一定全），那一家这一轮的样子照 [`crate::models::provider`]。
//!
//! 造会话（`session.create` 的 `model`）、换模型（`session.configure`）记进会话的是这时查过的模型（[`record`]）。

use std::fmt;

use crate::config::Values;
use crate::config::key::{self, MODEL};

use crate::models::knowledge::Knowledge;
use crate::models::provider::{self, NoModel, Target};
use crate::models::settings::UseSettings;

/// 读好的一个引用：一个模型，哪一家供应商、它那边叫什么。
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Reference {
    /// 供应商的编号。
    pub provider: String,
    /// 模型名，照供应商那边的叫法。
    pub model: String,
}

impl fmt::Display for Reference {
    /// 照原来的写法写回去。
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.provider, self.model)
    }
}

/// 读不成的引用。原话照 `models.md`「出错」那张表，英文，进 `model.called` 的原话。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Bad {
    /// 不是模型（以前的 `@池`、挡位名也算）。
    NotAReference(String),
}

impl fmt::Display for Bad {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Bad::NotAReference(text) => write!(f, "{text:?} is not a model"),
        }
    }
}

impl Reference {
    /// 读 `text`。
    ///
    /// # Errors
    ///
    /// 不是模型的写法：[`Bad::NotAReference`]。
    pub fn parse(text: &str) -> Result<Reference, Bad> {
        match text.split_once('/') {
            Some((provider, model))
                if key::valid_provider(provider) && key::valid(MODEL, model) =>
            {
                Ok(Reference {
                    provider: provider.to_string(),
                    model: model.to_string(),
                })
            }
            _ => Err(Bad::NotAReference(text.to_string())),
        }
    }
}

/// 造会话时记下的引用（施工 8-8，`session.create` 的 `model`；`session.configure` 也照它）：照写法读，那一家要配了。交回记下的
/// 那一个，原样的写法。
///
/// # Errors
///
/// 读不成（以前的 `@池`、挡位名也算）；引用的供应商没有（协议上都是 `unknown_model`）。
pub fn record(values: &Values, text: &str) -> Result<String, NoModel> {
    let reference = Reference::parse(text).map_err(bad)?;
    if !provider::configured(values).contains(&reference.provider) {
        return Err(NoModel(format!("no provider {:?}", reference.provider)));
    }
    Ok(reference.to_string())
}

/// 引用 `text` 这一轮发给哪一家的哪个模型，照这一轮的配置 `values`、手头的资料 `knowledge`。
///
/// # Errors
///
/// 读不成；引用的供应商没有；那一家用不了（[`provider::provider`]）。
pub fn resolve(values: &Values, knowledge: &Knowledge<'_>, text: &str) -> Result<Target, NoModel> {
    let Reference {
        provider: id,
        model,
    } = Reference::parse(text).map_err(bad)?;
    Ok(Target {
        provider: provider::provider(values, knowledge, &id)?,
        model,
    })
}

/// 用途里点名的模型（施工 8-8，`model.list` 照它列）：交回供应商的编号和模型名，照 `models.chat`、`vision` 的先后，可能重；
/// 写法不对的不算。
pub fn named(values: &Values) -> Vec<(String, String)> {
    let uses = UseSettings::from(values);
    [uses.chat, uses.vision]
        .into_iter()
        .flatten()
        .filter_map(|text| Reference::parse(&text).ok())
        .map(|reference| (reference.provider, reference.model))
        .collect()
}

/// 读不成的，说成没有模型的原话。
fn bad(bad: Bad) -> NoModel {
    NoModel(bad.to_string())
}

#[cfg(test)]
mod tests;
