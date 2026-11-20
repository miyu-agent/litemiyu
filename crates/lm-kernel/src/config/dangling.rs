//! 引用的东西没有（`docs/blueprint/config.md`「报错」的 `bad_reference`，`models.md`「怎么走」第三条第 2 条，施工 8-8）：
//! 引用（`models.chat`）、模型的列表指的供应商，在合出来的最终值里没有（LM-8 砍了模型池，指的池那一半没了）。
//!
//! 这是跨项的查：一项写在项目配置里，指的供应商可以配在系统配置里，所以照最终值查，不照这一份文件查。和引用的密钥、
//! 环境变量取不到（[`crate::config::secret::missing`]）一样，读进来以后另查一遍、只报不丢：值照样用，路由当场照它说 `no_model`
//! 和为什么（`models.md`「怎么走」第一条第 7 条）。写法不对的（`bad_format`）解析时已经丢了，这里碰不到。
//!
//! 什么算「有」由用它的一方说（`provider` 闭包）：这一层只认引用的写法，不认识模型那一块的键。

use crate::config::item::provider_of;
use crate::config::item::{Item, Kind, Layer};
use crate::config::parse::{Entry, Parsed};
use crate::config::problem::{Code, Problem};
use crate::config::value::Value;

/// 一层配置 `parsed`（当成 `layer` 读的）里，引用、模型的列表指的供应商没有的：一处一条 `NoProvider`（协议上是
/// `bad_reference`，错误），`name` 是指的那一家。只看这一层算数的项，列表里每一个各查各的。`provider` 说一家供应商配没配。
pub fn dangling(
    items: &[Item],
    parsed: &Parsed,
    layer: Layer,
    provider: &dyn Fn(&str) -> bool,
) -> Vec<Problem> {
    let mut found = Vec::new();
    for (key, entry, text) in texts(items, parsed, |kind| {
        // `model_or` 列出的那几个字照引用读不出指的是什么，下面自然跳过（施工 R-5 补）。
        matches!(kind, Kind::Reference | Kind::Model | Kind::ModelOr(_))
    }) {
        let Some(name) = provider_of(text).filter(|name| !provider(name)) else {
            continue;
        };
        found.push(problem(Code::NoProvider, layer, &key, entry, name));
    }
    found
}

/// 这一层算数的项里，类型合 `wanted` 的字：本身、列表里的每一个；交回问题的键（列表照真的键）、那一项、那个字。
fn texts<'p>(
    items: &[Item],
    parsed: &'p Parsed,
    wanted: impl Fn(Kind) -> bool,
) -> Vec<(String, &'p Entry, &'p str)> {
    let mut found = Vec::new();
    for (key, entry) in parsed.entries.iter().filter(|(_, entry)| entry.counts) {
        let Some(item) = items.iter().find(|item| item.key == entry.item) else {
            continue;
        };
        match (item.kind, &entry.value) {
            (kind, Value::Text(text)) if wanted(kind) => {
                found.push((key.clone(), entry, text.as_ref()))
            }
            (Kind::List(inner), Value::List(values)) if wanted(*inner) => {
                found.extend(values.iter().filter_map(|value| match value {
                    Value::Text(text) => Some((key.clone(), entry, text.as_ref())),
                    _ => None,
                }));
            }
            _ => {}
        }
    }
    found
}

/// 一条：在这一项的值那里，`name` 是指的那个。
fn problem(code: Code, layer: Layer, key: &str, entry: &Entry, name: &str) -> Problem {
    let mut problem = Problem::item(code, layer, key, entry.at, &entry.raw);
    problem.name = Some(name.to_string());
    problem
}

#[cfg(test)]
mod tests;
