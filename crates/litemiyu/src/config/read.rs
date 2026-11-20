//! 界面字表的读法（瘦身，界面线第四趟）：先照 `serde_json` 读成一棵 [`Value`]，再一格一格照名字取。
//!
//! 原来每个字表的结构 `#[derive(Deserialize)]`：serde 给每个结构展开一整套读法（认字段名、每格一个槽、查重、缺了报错），
//! 一个五十来格的结构就是 40 kB 代码，界面的字表加起来两百多 kB（界面线第四趟的账）。这里每种格子（字、两个
//! 字、表……）只有一份读法；第七趟再把「查」和「造」分开（[`Read`]）：查照每个结构写成常量的样子走一份代码，造的那一步
//! 不会错，一格十几个字节。读法照旧严：缺了的、多出来的、类型不对的都报错，说清是哪一格。
//!
//! 结构用 [`texts!`](crate::config::texts) 写：和平常写结构一样，宏顺手给它实现 [`Read`]。

use std::collections::{BTreeMap, HashMap};

use serde_json::{Map, Value};

/// 一格该是什么样（[`check`] 照它查）。每个字表的结构有一份写成常量的，查 JSON 的只有 [`check`] 这一份代码。
pub enum Kind {
    /// 字。
    Text,
    /// 不小于 0、不大于 `max` 的整数。
    Whole {
        /// 这种整数放得下的最大的。
        max: u64,
    },
    /// 数。
    Number,
    /// `true` 或 `false`。
    Switch,
    /// 一列，每个都是 `item`。
    List {
        /// 每个是什么样。
        item: &'static Kind,
    },
    /// 正好 `len` 个的一列。
    Fixed {
        /// 每个是什么样。
        item: &'static Kind,
        /// 几个。
        len: usize,
    },
    /// 一张表（对象），每格的值都是 `item`。
    Table {
        /// 每格的值是什么样。
        item: &'static Kind,
    },
    /// 一个结构：照名字的格；`shared` 的不管多出来的格。
    Fields {
        /// 每格的名字和样子，照结构里的先后。
        names: &'static [(&'static str, &'static Kind)],
        /// 同一份 JSON 分两个结构读的（`texts!` 的 `@shared`）。
        shared: bool,
    },
}

/// 照一棵 JSON 读出来的。
///
/// 读分两步（瘦身，界面线第七趟）：先照 [`Read::KIND`] 整棵查一遍（[`check`]，不带泛型、只有一份），查过了再照查过的
/// 造（[`Read::build`]）。原来每格边读边查，每格都要展开「取出来、读、错了丢掉已经读好的几格、报错」那一套，一格两百多
/// 字节；现在造的那一步不会错，每格只是取出来交给那种格子的造法。
pub trait Read: Sized {
    /// 这种格子该是什么样。
    const KIND: Kind;

    /// 照查过的 `value` 造。只在 [`check`] 过了以后调：对不上的地方造成空的（不会走到）。
    fn build(value: Value) -> Self;

    /// 读 `value`；`at` 是它在哪一格（报错用）。
    ///
    /// # Errors
    ///
    /// 类型不对、缺了格、多了格：说清是哪一格。
    fn read(value: Value, at: &str) -> Result<Self, String> {
        check(&value, &Self::KIND, at)?;
        Ok(Self::build(value))
    }
}

/// 查 `value` 是不是 `kind` 那样；`at` 是它在哪一格。先后和报的话同原来边读边查的：结构先看是不是对象，再照先后一格格
/// 查（缺了的当 `null`），最后看多出来的格；一列、一张表照里面的先后查，正好几个的先查每个再数个数。
///
/// # Errors
///
/// 头一处不对的：说清是哪一格。
pub fn check(value: &Value, kind: &Kind, at: &str) -> Result<(), String> {
    match kind {
        Kind::Text => match value {
            Value::String(_) => Ok(()),
            Value::Null => Err(format!("缺了 {at}")),
            _ => Err(format!("{at} 应该是字")),
        },
        Kind::Whole { max } => match value.as_u64() {
            Some(n) if n <= *max => Ok(()),
            _ => Err(format!("{at} 应该是不小于 0 的整数")),
        },
        Kind::Number => match value.as_f64() {
            Some(_) => Ok(()),
            None => Err(format!("{at} 应该是数")),
        },
        Kind::Switch => match value.as_bool() {
            Some(_) => Ok(()),
            None => Err(format!("{at} 应该是 true 或 false")),
        },
        Kind::List { item } => list(value, at)?
            .iter()
            .try_for_each(|one| check(one, item, at)),
        Kind::Fixed { item, len } => {
            let items = list(value, at)?;
            items.iter().try_for_each(|one| check(one, item, at))?;
            if items.len() == *len {
                Ok(())
            } else {
                Err(format!("{at} 应该正好 {len} 个"))
            }
        }
        Kind::Table { item } => object(value, at)?
            .values()
            .try_for_each(|one| check(one, item, at)),
        Kind::Fields { names, shared } => {
            let map = object(value, at)?;
            for (name, kind) in *names {
                check(map.get(*name).unwrap_or(&Value::Null), kind, name)?;
            }
            let extra = map
                .keys()
                .find(|key| !names.iter().any(|(name, _)| name == key));
            match extra {
                Some(extra) if !shared => Err(format!("{at} 里多了 {extra}")),
                _ => Ok(()),
            }
        }
    }
}

/// 一列：不是一列的报错。
fn list<'a>(value: &'a Value, at: &str) -> Result<&'a Vec<Value>, String> {
    match value {
        Value::Array(items) => Ok(items),
        Value::Null => Err(format!("缺了 {at}")),
        _ => Err(format!("{at} 应该是一列")),
    }
}

/// 一个对象：不是对象的报错。
fn object<'a>(value: &'a Value, at: &str) -> Result<&'a Map<String, Value>, String> {
    match value {
        Value::Object(map) => Ok(map),
        Value::Null => Err(format!("缺了 {at}")),
        _ => Err(format!("{at} 应该是一个对象")),
    }
}

impl Read for String {
    const KIND: Kind = Kind::Text;

    fn build(value: Value) -> Self {
        match value {
            Value::String(text) => text,
            _ => Self::new(),
        }
    }
}

/// 数：照 JSON 的数读（查过了放得下）。
macro_rules! number {
    ($($ty:ty),*) => {$(
        impl Read for $ty {
            const KIND: Kind = Kind::Whole { max: <$ty>::MAX as u64 };

            fn build(value: Value) -> Self {
                value.as_u64().and_then(|n| <$ty>::try_from(n).ok()).unwrap_or_default()
            }
        }
    )*};
}
number!(u16, u32, u64, usize);

impl Read for f64 {
    const KIND: Kind = Kind::Number;

    fn build(value: Value) -> Self {
        value.as_f64().unwrap_or_default()
    }
}

impl Read for bool {
    const KIND: Kind = Kind::Switch;

    fn build(value: Value) -> Self {
        value.as_bool().unwrap_or_default()
    }
}

impl<T: Read> Read for Vec<T> {
    const KIND: Kind = Kind::List { item: &T::KIND };

    fn build(value: Value) -> Self {
        match value {
            Value::Array(items) => items.into_iter().map(T::build).collect(),
            _ => Self::new(),
        }
    }
}

impl<T: Read + Default, const N: usize> Read for [T; N] {
    const KIND: Kind = Kind::Fixed {
        item: &T::KIND,
        len: N,
    };

    fn build(value: Value) -> Self {
        let mut items = Vec::<T>::build(value).into_iter();
        std::array::from_fn(|_| items.next().unwrap_or_default())
    }
}

impl<T: Read> Read for HashMap<String, T> {
    const KIND: Kind = Kind::Table { item: &T::KIND };

    fn build(value: Value) -> Self {
        entries(value)
            .map(|(key, item)| (key, T::build(item)))
            .collect()
    }
}

impl<T: Read> Read for BTreeMap<String, T> {
    const KIND: Kind = Kind::Table { item: &T::KIND };

    fn build(value: Value) -> Self {
        // 一格格放进去，不 `collect`：JSON 的对象本来就照名字排好、不重名，`collect` 还要另展开一份稳定排序。
        let mut out = Self::new();
        for (key, item) in entries(value) {
            out.insert(key, T::build(item));
        }
        out
    }
}

/// 一个对象里的每一格（不是对象的当空的）。
fn entries(value: Value) -> serde_json::map::IntoIter {
    match value {
        Value::Object(map) => map,
        _ => Map::new(),
    }
    .into_iter()
}

/// 一个结构查过的格，照名字一格格取走（[`texts!`](crate::config::texts) 造结构用）。
pub struct Fields(Map<String, Value>);

impl Fields {
    /// 照查过的 `value`（不是对象的当空的）。
    #[must_use]
    pub fn new(value: Value) -> Self {
        match value {
            Value::Object(map) => Self(map),
            _ => Self(Map::new()),
        }
    }

    /// 取走叫 `name` 的那一格造成 `T`。
    pub fn take<T: Read>(&mut self, name: &str) -> T {
        T::build(self.value(name))
    }

    /// 取走叫 `name` 的那一格（没有的是 `null`）：不带泛型，每格只调它一下。
    #[inline(never)]
    fn value(&mut self, name: &str) -> Value {
        self.0.remove(name).unwrap_or(Value::Null)
    }
}

/// 读一份 JSON 原文成 `T`；`name` 是哪一份（报错用）。
///
/// # Errors
///
/// JSON 写坏了、读不成 `T`。
pub fn parse<T: Read>(name: &str, json: &str) -> Result<T, String> {
    let value: Value = lm_kernel::kernel::json::from_str(json)
        .map_err(|e| format!("resources/{name} 读不懂：{e}"))?;
    T::read(value, name).map_err(|e| format!("resources/{name} 读不懂：{e}"))
}

/// 写一个字表的结构，顺手实现 [`Read`]：每格照字段名取，缺了报错、多了也报错。前面写 `@shared` 的不管多出来的格：
/// 同一份 JSON 分两个结构读的（第一屏要的字和别的字，`config/mod.rs`）。
macro_rules! texts {
    (
        $(@$shared:ident)?
        $(#[$meta:meta])*
        pub struct $name:ident {
            $(
                $(#[$field_meta:meta])*
                pub $field:ident : $ty:ty
            ),* $(,)?
        }
    ) => {
        $(#[$meta])*
        pub struct $name {
            $(
                $(#[$field_meta])*
                pub $field: $ty,
            )*
        }

        impl $crate::config::read::Read for $name {
            const KIND: $crate::config::read::Kind = $crate::config::read::Kind::Fields {
                names: &[$( (stringify!($field), &<$ty as $crate::config::read::Read>::KIND) ),*],
                shared: $crate::config::read::texts!(@shared $($shared)?),
            };

            fn build(value: ::serde_json::Value) -> Self {
                let mut fields = $crate::config::read::Fields::new(value);
                Self {
                    $( $field: fields.take(stringify!($field)), )*
                }
            }
        }
    };
    (@shared shared) => {
        true
    };
    (@shared) => {
        false
    };
}
pub(crate) use texts;

#[cfg(test)]
mod tests {
    use super::{Read, parse};

    texts! {
        /// 测试用的。
        #[derive(Debug, PartialEq)]
        pub struct Sample {
            /// 一句。
            pub one: String,
            /// 两句。
            pub pair: [String; 2],
            /// 一张表。
            pub map: std::collections::HashMap<String, String>,
        }
    }

    #[test]
    fn a_table_reads_by_name_and_says_which_field_is_wrong() {
        let sample: Sample =
            parse("t.json", r#"{"one":"a","pair":["b","c"],"map":{"k":"v"}}"#).expect("读得懂");
        assert_eq!(sample.one, "a");
        assert_eq!(sample.pair, ["b".to_string(), "c".to_string()]);
        assert_eq!(sample.map["k"], "v");
        let missing = parse::<Sample>("t.json", r#"{"pair":["b","c"],"map":{}}"#);
        assert!(missing.unwrap_err().contains("缺了 one"));
        let extra = parse::<Sample>("t.json", r#"{"one":"a","pair":["b","c"],"map":{},"x":1}"#);
        assert!(extra.unwrap_err().contains("多了 x"));
        let short = parse::<Sample>("t.json", r#"{"one":"a","pair":["b"],"map":{}}"#);
        assert!(short.unwrap_err().contains("pair 应该正好 2 个"));
        let wrong = Sample::read(serde_json::json!({"one": 1}), "t");
        assert!(wrong.unwrap_err().contains("one 应该是字"));
    }

    texts! {
        /// 测试用的：套着一层、带数的。
        #[derive(Debug, PartialEq)]
        pub struct Outer {
            /// 套着的一层。
            pub inner: Sample,
            /// 一个小整数。
            pub small: u16,
            /// 一列数。
            pub list: Vec<u64>,
        }
    }

    texts! {
        @shared
        /// 测试用的：同一份里只要一格。
        #[derive(Debug, PartialEq)]
        pub struct Part {
            /// 一个小整数。
            pub small: u16,
        }
    }

    /// 先查完、再造（第七趟）：报的是头一处不对的，说法同原来边读边查的。
    #[test]
    fn the_first_wrong_field_is_reported_the_same_way_as_reading_field_by_field() {
        let inner = r#"{"one":"a","pair":["b","c"],"map":{"k":"v"}}"#;
        let good = format!(r#"{{"inner":{inner},"small":7,"list":[1,2]}}"#);
        let outer: Outer = parse("o.json", &good).expect("读得懂");
        assert_eq!(outer.inner.map["k"], "v");
        assert_eq!((outer.small, outer.list), (7, vec![1, 2]));
        let said = |json: String| parse::<Outer>("o.json", &json).unwrap_err();
        assert_eq!(
            said(r#"{"inner":{"one":"a","pair":["b","c"],"map":{},"x":1},"small":1}"#.into()),
            "resources/o.json 读不懂：inner 里多了 x"
        );
        assert_eq!(
            said(format!(r#"{{"inner":{inner},"small":70000,"list":[]}}"#)),
            "resources/o.json 读不懂：small 应该是不小于 0 的整数"
        );
        assert_eq!(
            said(format!(r#"{{"inner":{inner},"small":1,"list":[1,"2"]}}"#)),
            "resources/o.json 读不懂：list 应该是不小于 0 的整数"
        );
        assert_eq!(
            said(r#"{"inner":{"one":"a","pair":["b","c"],"map":{"k":1}},"small":1}"#.into()),
            "resources/o.json 读不懂：map 应该是字"
        );
        assert_eq!(
            said(r#"{"inner":[],"small":"x"}"#.into()),
            "resources/o.json 读不懂：inner 应该是一个对象"
        );
        assert_eq!(
            said("[]".into()),
            "resources/o.json 读不懂：o.json 应该是一个对象"
        );
        let part: Part = parse("o.json", &good).expect("同一份里别的格不管");
        assert_eq!(part.small, 7);
    }
}
