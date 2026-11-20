//! 一个模型的价格和倍率（Miyu 施工 8-7、8-15，`docs/blueprint/models.md`「模型的资料」`price`、`multiplier` 两行；LiteMiyu
//! 核心线第二趟恢复，只为算这个会话花了多少）：照 Miyu `facts.rs` 的 `price` 和倍率那一格、`price.rs` 的 `Tariff::of` 合成一步。
//!
//! - 价格是一整格：手写了一项就整份用手写的，币种不写是美元；本机的服务只认手写的，没写的是 0，来源 `local`；目录的照
//!   [`crate::models::matching`] 挑的那家借不借（`Matched::price`）；都没有的没有（不算金额）。
//! - 倍率：模型手写的，没有是 1（Miyu 另认供应商这一层写的，LiteMiyu 没登记那一项，见 `models/settings.rs`）。
//!
//! 和 Miyu 不一样：Miyu 把这两格放在 [`Facts`](super::Facts) 里，`model.list` 也列出来；LiteMiyu 的 `Facts` 不带价格
//! （主程序照它的样子造、照来源逐种认），路由挑定了真发的那个模型才照这里取一次，算完跟着那一次请求放掉，不常驻。

use crate::config::Layer;
use crate::config::merge::Resolved;
use crate::models::catalog::{Price, Rates, USD};
use crate::models::knowledge::Knowledge;
use crate::models::matching::Found;
use crate::models::price::Tariff;
use crate::models::provider::Provider;

use super::{Source, Written, looked_up};

/// 这家 `provider` 的模型 `model` 照这一轮的配置 `resolved`、手头的资料 `knowledge` 算金额要的（[`Tariff`]）；没有价格的
/// 没有。手写的出处写成哪份文件，由 `file` 照层给。
pub fn tariff(
    resolved: &Resolved,
    knowledge: &Knowledge<'_>,
    provider: &Provider,
    model: &str,
    file: &dyn Fn(Layer) -> String,
) -> Option<Tariff> {
    let written = Written {
        resolved,
        provider: &provider.id,
        model,
    };
    let (price, from) = price(&written, knowledge, provider)?;
    let multiplier = written
        .float(&["price_multiplier"])
        .map_or(1.0, |(value, _)| value);
    let source = match from {
        From::Catalog(entry) => format!("catalog:{entry}"),
        From::Config(layer, line) => format!("config:{}:{line}", file(layer)),
        From::Local => "local".to_string(),
    };
    Some(Tariff {
        price,
        multiplier,
        source,
    })
}

/// 价格从哪来（写进 `cost.source`）：Miyu 照 `Source` 的三种（`Catalog`、`Config`、`Local`）；LiteMiyu 的 [`Source`] 没有
/// `Local` 这一种（主程序照它逐种认，加一种编不过），这里单列。
enum From {
    /// 手写的：哪一层、第几行。
    Config(Layer, usize),
    /// 目录里的 `<供应商>/<模型>`。
    Catalog(String),
    /// 本机的服务。
    Local,
}

/// 价格和它从哪来：手写的整份；本机的服务没写是 0；目录的照挑的那家借不借；都没有的没有。
fn price(
    written: &Written<'_>,
    knowledge: &Knowledge<'_>,
    provider: &Provider,
) -> Option<(Price, From)> {
    let item = |name: &str| written.float(&["price", name]);
    let rates = ["input", "output", "cache_read", "cache_write"].map(item);
    let currency = written.text(&["price", "currency"]);
    let first = rates
        .iter()
        .flatten()
        .map(|(_, source)| source.clone())
        .chain(currency.iter().map(|(_, source)| source.clone()))
        .next();
    if let Some(source) = first {
        let [input, output, cache_read, cache_write] =
            rates.map(|rate| rate.map(|(value, _)| value));
        let rates = Rates {
            input,
            output,
            cache_read,
            cache_write,
        };
        let currency = currency.map_or_else(|| USD.to_string(), |(text, _)| text);
        // 手写的来源只会是配置文件的哪一行（`Written::at`）。
        let Source::Config { layer, line } = source else {
            return None;
        };
        return Some((Price::of(rates, &currency), From::Config(layer, line)));
    }
    if provider.local {
        return Some((Price::free(), From::Local));
    }
    let (side, _) = looked_up(written, knowledge, provider);
    match (side.found, side.entry) {
        (Found::Matched(matched), Some(entry)) if matched.price => {
            let price = entry.price.as_deref().cloned()?;
            Some((
                price,
                From::Catalog(format!("{}/{}", matched.provider, matched.model)),
            ))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests;
