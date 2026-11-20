//! 编辑窗存（Miyu 终端 `settings/forms/apply.rs`，蓝图「配置页」第 16 到 19 条、第 24 条）：照窗里的值算出要改的几项。只记
//! 动了的项。不收的交回出错的名字（`errors` 里的键）和要换进 `{name}` 的字。Miyu 先记进草稿再发 `config.set`；LiteMiyu 没有
//! 核心，算出来的交给配置服务一次改完（`keys.rs`）。新填的 key 另交出来：先存进密钥文件，配置里写引用。

use std::borrow::Cow;

use lm_engine::settings::Edit;
use lm_kernel::config::key::fill;
use lm_kernel::config::secret::Reference;
use lm_kernel::config::{Number, Value};

use super::data::Data;
use super::forms::{Field, Form, Target, Val, Written, name_ok, window_value};

/// 不收：出错的名字、`{name}` 换成什么。
pub type Refused = (&'static str, String);

/// 算好的一次存。
#[derive(Debug, Default)]
pub struct Applied {
    /// 要改的几项。
    pub edits: Vec<Edit>,
    /// 新填的 key：哪一家（存成后照它起密钥的名字）、明文。
    pub secret: Option<(String, String)>,
    /// 存成了选中哪一样：供应商编号、`供应商/模型`。
    pub select: String,
}

/// 照窗里的值算出要改的几项。
///
/// # Errors
///
/// 编号、名字不合写法，撞了已有的，数字写得不对。
pub fn apply(form: &Form, view: &Data, written: &Written) -> Result<Applied, Refused> {
    match &form.target {
        Target::Provider(id) => provider(form, id.as_deref(), view, written),
        Target::Model(p, m) => model(form, p, m.as_deref(), view),
    }
}

fn text(value: &str) -> Value {
    Value::Text(Cow::Owned(value.to_string()))
}

fn provider(
    form: &Form,
    id: Option<&str>,
    view: &Data,
    written: &Written,
) -> Result<Applied, Refused> {
    let typed = form.text(Field::Id).trim().to_string();
    let renamed = id.filter(|id| *id != typed);
    if id.is_none() || renamed.is_some() {
        if !name_ok(&typed) {
            return Err(("bad_id", typed));
        }
        if view.provider(&typed).is_some() {
            return Err(("taken", typed));
        }
    }
    let mut out = Applied::default();
    // 改了编号：照旧编号写着的每一项搬到新编号下，指着它的引用（默认用途、池的成员）跟着换（Miyu 交给核心 `provider.rename`）。
    if let Some(old) = renamed {
        rename(&mut out.edits, written, old, &typed);
    }
    let id = typed;
    let key = |item: &str| fill(&format!("providers.<id>.{item}"), &[&id]);
    let new = id_new(form);
    let auth = form.choice(Field::Auth).unwrap_or("secret");
    let pasted = form.text(Field::Key).trim().to_string();
    if new && auth == "env" && pasted.is_empty() {
        return Err(("need_key", String::new()));
    }
    let mut set = |key: String, value: Option<Value>| {
        out.edits.retain(|edit| edit.key() != key);
        out.edits.push(match value {
            Some(value) => Edit::Set(key, value),
            None => Edit::Unset(key),
        });
    };
    // 照目录加的：编号和目录的不一样（改成了合写法的、错开的、自己改了的）才写 `catalog`。
    if let Some(catalog) = form.catalog.as_deref().filter(|c| new && *c != id) {
        set(key("catalog"), Some(text(catalog)));
    }
    if new || form.moved(Field::Name) {
        let name = form.text(Field::Name).trim();
        set(key("name"), (!name.is_empty()).then(|| text(name)));
    }
    if new || form.moved(Field::BaseUrl) {
        let url = form.text(Field::BaseUrl).trim();
        let value = match url.strip_prefix("env:") {
            Some(env) => Some(Value::Secret(Reference::Env(env.trim().to_string()))),
            None => (!url.is_empty()).then(|| text(url)),
        };
        set(key("base_url"), value);
    }
    if form.moved(Field::Driver) {
        let driver = form.choice(Field::Driver).filter(|d| !d.is_empty());
        set(key("driver"), driver.map(text));
    }
    if !pasted.is_empty() {
        if auth == "env" {
            set(key("key"), Some(Value::Secret(Reference::Env(pasted))));
        } else {
            out.secret = Some((id.clone(), pasted));
        }
    }
    // 新加的一家什么都没写的（只填了 key、key 又没填）也要在配置里留下它：写对应目录里的哪一家。
    if new && out.edits.is_empty() && out.secret.is_none() {
        let vendor = form.catalog.clone().unwrap_or_else(|| id.clone());
        out.edits.push(Edit::Set(key("catalog"), text(&vendor)));
    }
    out.select = id;
    Ok(out)
}

/// 编号 `old` 改成 `new`：搬表、换引用。
fn rename(edits: &mut Vec<Edit>, written: &Written, old: &str, new: &str) {
    let from = format!("{}.", fill("providers.<id>", &[old]));
    let to = format!("{}.", fill("providers.<id>", &[new]));
    let head = format!("{old}/");
    let retarget = |reference: &str| {
        reference
            .strip_prefix(&head)
            .map(|model| format!("{new}/{model}"))
    };
    for (key, value) in written {
        if let Some(rest) = key.strip_prefix(&from) {
            edits.push(Edit::Unset(key.clone()));
            edits.push(Edit::Set(format!("{to}{rest}"), value.clone()));
            continue;
        }
        match value {
            Value::Text(reference) if key.starts_with("models.") => {
                if let Some(moved) = retarget(reference) {
                    edits.push(Edit::Set(key.clone(), text(&moved)));
                }
            }
            _ => {}
        }
    }
}

fn id_new(form: &Form) -> bool {
    form.target == Target::Provider(None)
}

fn model(form: &Form, provider: &str, name: Option<&str>, view: &Data) -> Result<Applied, Refused> {
    let name = match name {
        Some(n) => n.to_string(),
        None => {
            let n = form.text(Field::Model).trim().to_string();
            if n.is_empty() || n.len() > 128 || n.chars().any(char::is_control) {
                return Err(("need_name", n));
            }
            if view.model(&format!("{provider}/{n}")).is_some() {
                return Err(("taken", n));
            }
            n
        }
    };
    let new = form.target == Target::Model(provider.to_string(), None);
    let key = |item: &str| {
        fill(
            &format!("providers.<id>.models.<model>.{item}"),
            &[provider, &name],
        )
    };
    // 先查完再写：有一项不收，什么都不改。
    let tokens = |field: Field| -> Result<Option<u64>, Refused> {
        match form.text(field).trim() {
            "" => Ok(None),
            t => window_value(t)
                .map(Some)
                .ok_or(("bad_number", field.name().to_string())),
        }
    };
    let window = tokens(Field::Window)?;
    let max_output = tokens(Field::MaxOutput)?;
    let temperature = match form.text(Field::Temperature).trim() {
        "" => None,
        t => Some(
            t.parse::<f64>()
                .ok()
                .filter(|n| n.is_finite() && *n >= 0.0)
                .ok_or(("bad_number", "temperature".to_string()))?,
        ),
    };
    let mut out = Applied::default();
    let mut set = |key: String, value: Option<Value>| {
        out.edits.push(match value {
            Some(value) => Edit::Set(key, value),
            None => Edit::Unset(key),
        });
    };
    let int = |n: u64| Value::Int(i64::try_from(n).unwrap_or(i64::MAX));
    if new || form.moved(Field::Inputs) {
        let row = form.rows.iter().find(|r| r.field == Field::Inputs);
        let picked = match row.map(|r| &r.val) {
            Some(Val::Multi {
                options,
                on: Some(on),
            }) if on.iter().any(|b| *b) => Some(
                options
                    .iter()
                    .zip(on)
                    .filter(|(_, b)| **b)
                    .map(|(o, _)| text(o))
                    .collect::<Vec<_>>(),
            ),
            _ => None,
        };
        set(
            key("inputs"),
            picked.map(|list| Value::List(Cow::Owned(list))),
        );
    }
    if form.moved(Field::Window) {
        set(key("window"), window.map(int));
    }
    if form.moved(Field::MaxOutput) {
        set(key("max_output"), max_output.map(int));
    }
    if form.moved(Field::Effort) {
        let effort = form.choice(Field::Effort).filter(|e| !e.is_empty());
        set(key("effort"), effort.map(text));
    }
    if form.moved(Field::Temperature) {
        set(
            key("temperature"),
            temperature.map(|t| Value::Float(Number::new(t))),
        );
    }
    // 新加的模型一格都没写的：写下它能收文字，配置里才有它（Miyu 新加的模型默认勾着文字）。
    if new && out.edits.iter().all(|edit| matches!(edit, Edit::Unset(_))) {
        out.edits = vec![Edit::Set(
            key("inputs"),
            Value::List(Cow::Owned(vec![text("text")])),
        )];
    }
    out.select = format!("{provider}/{name}");
    Ok(out)
}
