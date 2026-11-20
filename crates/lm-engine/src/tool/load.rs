//! 读工具的字（`26-提示词.md` 第八节）：`software/<软件包>/tools/<工具>.json` 是给模型看的说明和参数格式，
//! `software/<软件包>/<工具>/<名字>.txt` 是输出里给她看的几句（施工 4-4 下）。
//!
//! Miyu 从资源目录读；LiteMiyu 只有一个程序文件，字编进程序（`super::shipped`，L3 四），路径照资源目录里的相对位置认。
//! 自己带着字的软件包（Miyu 的 `spec_in`、`text_in`）跟着软件包一起没有了。

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

use serde::Deserialize;

use crate::tool::{Scope, Spec};
use lm_kernel::kernel::raw::RawJson;
use lm_kernel::kernel::template::Template;
use lm_kernel::kernel::tool::{Access, Role};

/// 一份字读不出来，或者写法不对。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadError {
    /// 哪一份。
    pub file: PathBuf,
    /// 为什么。
    pub why: String,
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.file.display(), self.why)
    }
}

impl std::error::Error for LoadError {}

/// `tools/<工具>.json` 的样子。
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SpecFile {
    description: String,
    parameters: RawJson,
    /// 职能（施工 XG-2 上，设计 64 第四节）：认不得的读不进。
    #[serde(default)]
    role: Option<Role>,
    /// 范围（施工 XG-2 上，设计 64 第五节）：没写的哪种会话都给。
    #[serde(default)]
    scope: Scope,
}

/// 读软件包 `package` 的工具 `name` 的说明和参数格式，访问类别是 `access`。
///
/// # Errors
///
/// 程序里没有这一份；不是 `{description, parameters}` 的样子。
pub fn spec(package: &str, name: &str, access: Access) -> Result<Spec, LoadError> {
    let file = spec_file(package, name);
    parse_spec(&file, read(&file)?, name, access)
}

/// 照 `text` 读工具 `name` 的说明（`file` 是它在哪，报错用）。
fn parse_spec(file: &str, text: &str, name: &str, access: Access) -> Result<Spec, LoadError> {
    let parsed: SpecFile = lm_kernel::kernel::json::from_str(text).map_err(|error| LoadError {
        file: PathBuf::from(file),
        why: error.to_string(),
    })?;
    Ok(Spec {
        name: name.to_string(),
        description: parsed.description,
        parameters: parsed.parameters,
        access,
        role: parsed.role,
        scope: parsed.scope,
    })
}

/// 同 [`spec`]，说明是一段模板，换进 `fields`：说明里要写这台机器上的东西的（`shell` 写用的是哪种 shell，施工
/// 4-8）。起来时换一次，会话里不变。别的工具的说明不当模板读：里面的花括号是字面的。
///
/// # Errors
///
/// 同 [`spec`]；说明不是合写法的模板，或者要了没给的字段。
pub fn spec_filled(
    package: &str,
    name: &str,
    access: Access,
    fields: &[(&str, &str)],
) -> Result<Spec, LoadError> {
    let mut spec = spec(package, name, access)?;
    let bad = |why: String| LoadError {
        file: PathBuf::from(spec_file(package, name)),
        why,
    };
    let template = Template::parse(&spec.description).map_err(|error| bad(error.to_string()))?;
    let fields: BTreeMap<&str, &str> = fields.iter().copied().collect();
    spec.description = template
        .render(&fields)
        .map_err(|error| bad(error.to_string()))?;
    Ok(spec)
}

/// 读软件包 `package` 的工具 `tool` 输出里的一句 `name`，拿 `fields` 里的每个字段试换一次。
///
/// # Errors
///
/// 程序里没有这一份；不是合写法的模板，或者要了 `fields` 以外的字段。
pub fn text(package: &str, tool: &str, name: &str, fields: &[&str]) -> Result<Template, LoadError> {
    let file = text_file(package, tool, name);
    let source = read(&file)?;
    let bad = |why: String| LoadError {
        file: PathBuf::from(&file),
        why,
    };
    let template = Template::parse(source).map_err(|error| bad(error.to_string()))?;
    let trial: BTreeMap<&str, &str> = fields.iter().map(|field| (*field, "")).collect();
    template
        .render(&trial)
        .map_err(|error| bad(error.to_string()))?;
    Ok(template)
}

/// 读软件包 `package` 的工具 `tool` 的一份字 `name` 的原文，不当模板读（施工 M-4：`shell` 说明里的例子，里面有花括号）。
///
/// # Errors
///
/// 程序里没有这一份。
pub fn raw(package: &str, tool: &str, name: &str) -> Result<String, LoadError> {
    read(&text_file(package, tool, name)).map(str::to_string)
}

/// 照模板 `template` 换进字段 `fields`。
///
/// # Panics
///
/// 实际不会：造的时候试换过，字段都有。
pub fn say(template: &Template, fields: &[(&str, &str)]) -> String {
    let fields: BTreeMap<&str, &str> = fields.iter().copied().collect();
    template.render(&fields).expect("造的时候试换过，字段都有")
}

/// 工具 `name` 的说明在哪：`software/<软件包>/tools/<工具>.json`。
fn spec_file(package: &str, name: &str) -> String {
    format!("software/{package}/tools/{name}.json")
}

/// 工具 `tool` 的一句 `name` 在哪：`software/<软件包>/<工具>/<名字>.txt`。
fn text_file(package: &str, tool: &str, name: &str) -> String {
    format!("software/{package}/{tool}/{name}.txt")
}

/// 编进程序的一份。
fn read(file: &str) -> Result<&'static str, LoadError> {
    super::shipped::get(file).ok_or_else(|| LoadError {
        file: PathBuf::from(file),
        why: "not shipped with this program".to_string(),
    })
}

#[cfg(test)]
mod tests;
