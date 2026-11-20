//! 一封信的读法（引擎线第八趟瘦身）：先照 serde 读成一棵 [`Value`]，再看 `kind` 一格格取。
//!
//! 原来照 serde 派生（`#[serde(tag = "kind")]`）：内部带标签的几选一要先把整封读进 serde 的中间值、再按每一种各展一份读法，
//! 一封信的读法 12 kB 代码。这里读出来的和原来一样：`kind` 得是四种之一的字串（缺了、不认识的报错）；每一种照它的格子取，缺了
//! 必填的、类型不对的报错，不认识的格子不管，`status`、`wall`（LM-12）缺了、写成 `null` 的没有；整封写成数组的（serde 也认）头一个是 `kind`，
//! 后面照格子的先后，少了必填的、多了的报错。只一处不同：同一格写了两遍，派生的读法报错，这里照最后一遍（信是程序自己写的）。
//! 写出去照旧是派生的 `Serialize`，一字不变。

use serde_json::{Map, Value};

use lm_kernel::kernel::id::{CallId, SessionId};

use super::Letter;

/// 四种的名字，连同各自的格子（照结构里的先后，数组写法照它对上）。
const KINDS: [(&str, &[&str]); 4] = [
    ("message", &["from", "call", "text"]),
    ("watch", &["from", "since", "wall"]),
    ("idle", &["from", "since", "status"]),
    ("closed", &["from", "since"]),
];

/// 读一封的字节。错的是英文的一句（进运行日志）。
pub(super) fn letter(body: &[u8]) -> Result<Letter, String> {
    let value: Value =
        lm_kernel::kernel::json::from_slice(body).map_err(|error| error.to_string())?;
    let (kind, fields) = match value {
        Value::Object(mut map) => {
            let kind = match map.remove("kind") {
                Some(Value::String(kind)) => kind,
                Some(other) => return Err(format!("invalid type: {} at `kind`", what(&other))),
                None => return Err("missing field `kind`".to_string()),
            };
            (kind, map)
        }
        Value::Array(items) => {
            let mut items = items.into_iter();
            let kind = match items.next() {
                Some(Value::String(kind)) => kind,
                Some(other) => return Err(format!("invalid type: {} at `kind`", what(&other))),
                None => return Err("invalid length 0, expected a letter".to_string()),
            };
            let names = KINDS
                .iter()
                .find(|(name, _)| *name == kind)
                .map_or(&[][..], |(_, names)| *names);
            let rest: Vec<Value> = items.collect();
            if rest.len() > names.len() {
                return Err(format!(
                    "invalid length {}, expected {} fields for `{kind}`",
                    rest.len(),
                    names.len()
                ));
            }
            let mut map = Map::new();
            for (name, item) in names.iter().zip(rest) {
                map.insert((*name).to_string(), item);
            }
            (kind, map)
        }
        other => {
            return Err(format!("invalid type: {}, expected a letter", what(&other)));
        }
    };
    let from = || id(&fields, "from", SessionId::parse);
    match kind.as_str() {
        "message" => Ok(Letter::Message {
            from: from()?,
            call: id(&fields, "call", CallId::parse)?,
            text: text(&fields, "text")?,
        }),
        "watch" => Ok(Letter::Watch {
            from: from()?,
            since: since(&fields)?,
            wall: match fields.get("wall") {
                None | Some(Value::Null) => None,
                Some(_) => Some(millis(&fields, "wall")?),
            },
        }),
        "idle" => Ok(Letter::Idle {
            from: from()?,
            since: since(&fields)?,
            status: match fields.get("status") {
                None | Some(Value::Null) => None,
                Some(_) => Some(text(&fields, "status")?),
            },
        }),
        "closed" => Ok(Letter::Closed {
            from: from()?,
            since: since(&fields)?,
        }),
        other => Err(format!(
            "unknown variant `{other}`, expected one of `message`, `watch`, `idle`, `closed`"
        )),
    }
}

/// 必填的一格字串。
fn text(fields: &Map<String, Value>, key: &str) -> Result<String, String> {
    match fields.get(key) {
        Some(Value::String(text)) => Ok(text.clone()),
        Some(other) => Err(format!(
            "invalid type: {} at `{key}`, expected a string",
            what(other)
        )),
        None => Err(format!("missing field `{key}`")),
    }
}

/// 必填的一格编号：字串，照 `parse` 认得出。
fn id<T, E: std::fmt::Display>(
    fields: &Map<String, Value>,
    key: &str,
    parse: fn(&str) -> Result<T, E>,
) -> Result<T, String> {
    parse(&text(fields, key)?).map_err(|error| format!("{error} at `{key}`"))
}

/// `since`：必填的毫秒数。
fn since(fields: &Map<String, Value>) -> Result<i64, String> {
    millis(fields, "since")
}

/// 一格毫秒数：放得进 `i64` 的整数（照 serde：小数、超了范围的报错）。
fn millis(fields: &Map<String, Value>, key: &str) -> Result<i64, String> {
    match fields.get(key) {
        Some(Value::Number(n)) => n
            .as_i64()
            .ok_or_else(|| format!("invalid value: `{n}` at `{key}`, expected i64")),
        Some(other) => Err(format!(
            "invalid type: {} at `{key}`, expected i64",
            what(other)
        )),
        None => Err(format!("missing field `{key}`")),
    }
}

/// 报错时怎么说一格的类型（同 serde 的说法）。
fn what(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "sequence",
        Value::Object(_) => "map",
    }
}
