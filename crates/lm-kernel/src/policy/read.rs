//! 快照里几组字的读法（引擎线第七趟瘦身）：先照 serde 读成一棵 [`Value`]，再按名字一格格取。
//!
//! 照 serde 派生的读法，每个结构展开一份「认格子名、读这一格、出错丢掉已读的」，一格一千多字节，几十格的结构二三十 kB。这里
//! 每个结构只剩几行按名字取的调用（[`Fields`]），读字串、判缺了的都是一份共用的代码。读出来的值和派生的读法一样：缺了必填的、
//! 类型不对的照样报错（话照 serde 的样子），可缺的缺了、写成 `null` 的读成没有，不认识的格子不管。只一处不同：同一个格子写了两遍，
//! 派生的读法报错，这里照最后一遍（快照是程序自己写的，不会写两遍）。写出去的照旧是派生的 `Serialize`，一字不变。

use std::borrow::Cow;

use serde_json::{Map, Value};

/// 读一个结构的结果：错的是说给人看的一句话（交给 serde 包成它的错）。
pub(crate) type Read<T> = Result<T, String>;

/// 一个 JSON 对象，按名字取格子。
pub(crate) struct Fields<'a> {
    map: Cow<'a, Map<String, Value>>,
}

impl<'a> Fields<'a> {
    /// `value` 得是对象；`what` 是报错时怎么称呼它（同 serde 的「expected struct X」）。
    pub(crate) fn of(value: &'a Value, what: &str) -> Read<Fields<'a>> {
        match value {
            Value::Object(map) => Ok(Fields {
                map: Cow::Borrowed(map),
            }),
            other => Err(format!(
                "invalid type: {}, expected struct {what}",
                kind(other)
            )),
        }
    }

    /// 同 [`Fields::of`]，另认写成数组的（serde 派生的读法也认）：照 `names`（结构里格子的先后）一个个对上，比格子多的报错，
    /// 少的当后面几格缺了。
    pub(crate) fn of_struct(value: &'a Value, what: &str, names: &[&str]) -> Read<Fields<'a>> {
        match value {
            Value::Array(items) if items.len() <= names.len() => {
                // 一格格插：`collect` 成 `Map` 要另展开一份排序。
                let mut map = Map::new();
                for (name, item) in names.iter().zip(items) {
                    map.insert((*name).to_string(), item.clone());
                }
                Ok(Fields {
                    map: Cow::Owned(map),
                })
            }
            Value::Array(items) => Err(format!(
                "invalid length {}, expected struct {what} with {} elements",
                items.len(),
                names.len()
            )),
            other => Fields::of(other, what),
        }
    }

    /// 必填的一格字串：缺了的报 ``missing field `key` ``。
    pub(crate) fn text(&self, key: &str) -> Read<String> {
        match self.map.get(key) {
            Some(value) => string(key, value),
            None => Err(format!("missing field `{key}`")),
        }
    }

    /// 缺了读成空的一格字串（`#[serde(default)]`）：写了的照样得是字串，`null` 也不行。
    pub(crate) fn text_or_empty(&self, key: &str) -> Read<String> {
        self.map
            .get(key)
            .map_or_else(|| Ok(String::new()), |value| string(key, value))
    }

    /// 可缺的一格字串（`Option<String>`）：缺了、写成 `null` 的没有。
    pub(crate) fn opt_text(&self, key: &str) -> Read<Option<String>> {
        match self.map.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(value) => string(key, value).map(Some),
        }
    }

    /// 必填的一组（嵌着的结构），照 `read` 读：缺了的报 ``missing field `key` ``。
    pub(crate) fn nested<T>(&self, key: &str, read: fn(&Value) -> Read<T>) -> Read<T> {
        match self.map.get(key) {
            Some(value) => read(value),
            None => Err(format!("missing field `{key}`")),
        }
    }

    /// 缺了照默认的一组（`#[serde(default)]` 的嵌着的结构）：写了的照 `read` 读，`null` 也照它读（读不出就报错）。
    pub(crate) fn nested_or_default<T: Default>(
        &self,
        key: &str,
        read: fn(&Value) -> Read<T>,
    ) -> Read<T> {
        self.map.get(key).map_or_else(|| Ok(T::default()), read)
    }

    /// 可缺的一格（`Option<T>`）：缺了、写成 `null` 的没有，别的照 `read` 读。
    pub(crate) fn opt<T>(&self, key: &str, read: fn(&Value) -> Read<T>) -> Read<Option<T>> {
        match self.map.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(value) => read(value).map(Some),
        }
    }

    /// 必填的一格数（`u32`、`u64`、`usize`）：照 serde 的规矩，不是数的、负数、小数、超了这一种的范围的都报错。
    pub(crate) fn number<T: TryFrom<u64>>(&self, key: &str) -> Read<T> {
        match self.map.get(key) {
            Some(value) => number(key, value),
            None => Err(format!("missing field `{key}`")),
        }
    }

    /// 缺了照 `default` 的一格数（`#[serde(default = "…")]`）：写了的照 [`Fields::number`] 读，`null` 也不行。
    pub(crate) fn number_or<T: TryFrom<u64>>(&self, key: &str, default: T) -> Read<T> {
        self.map
            .get(key)
            .map_or_else(|| Ok(default), |value| number(key, value))
    }

    /// 可缺的一格数（`Option<u64>` 这些）：缺了、写成 `null` 的没有。
    pub(crate) fn opt_number<T: TryFrom<u64>>(&self, key: &str) -> Read<Option<T>> {
        match self.map.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(value) => number(key, value).map(Some),
        }
    }

    /// 这几格都在、都是字串：交回它们（照 `keys` 的先后）；缺了哪一格、哪一格不是字串的没有。照 serde 的 `flatten` 加
    /// `Option`：读不出那一组就当没有，不报错。
    pub(crate) fn all_texts<const N: usize>(&self, keys: [&str; N]) -> Option<[String; N]> {
        let mut found = [const { String::new() }; N];
        for (slot, key) in found.iter_mut().zip(keys) {
            match self.map.get(key) {
                Some(Value::String(text)) => slot.clone_from(text),
                _ => return None,
            }
        }
        Some(found)
    }
}

/// 一格数：只认非负的整数，再看放不放得进 `T`（报错的话照 serde 的说法，加上是哪一格）。
fn number<T: TryFrom<u64>>(key: &str, value: &Value) -> Read<T> {
    let what = std::any::type_name::<T>();
    let Value::Number(n) = value else {
        return Err(format!(
            "invalid type: {} at `{key}`, expected {what}",
            kind(value)
        ));
    };
    if let Some(whole) = n.as_u64() {
        T::try_from(whole)
            .map_err(|_| format!("invalid value: integer `{whole}` at `{key}`, expected {what}"))
    } else if let Some(negative) = n.as_i64() {
        Err(format!(
            "invalid value: integer `{negative}` at `{key}`, expected {what}"
        ))
    } else {
        Err(format!(
            "invalid type: floating point `{n}` at `{key}`, expected {what}"
        ))
    }
}

/// 一格字串。
fn string(key: &str, value: &Value) -> Read<String> {
    match value {
        Value::String(text) => Ok(text.clone()),
        other => Err(format!(
            "invalid type: {} at `{key}`, expected a string",
            kind(other)
        )),
    }
}

/// 报错时怎么说这一格的类型（同 serde 的 `Unexpected` 的说法）。
fn kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "sequence",
        Value::Object(_) => "map",
    }
}

/// 给类型 `$ty` 接上照 `$read`（`fn(&Value) -> Read<$ty>`）读的 `Deserialize`：先读成一棵 [`Value`]，再交给它。
macro_rules! read_by_name {
    ($ty:ty, $read:path) => {
        impl<'de> serde::Deserialize<'de> for $ty {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let value = <serde_json::Value as serde::Deserialize>::deserialize(d)?;
                $read(&value).map_err(<D::Error as serde::de::Error>::custom)
            }
        }
    };
}

pub(crate) use read_by_name;

#[cfg(test)]
mod numbers_tests;
#[cfg(test)]
mod tests;
