//! 读 JSON 的入口（LM-10）：同 `serde_json::from_str`、`from_slice`，只是结构体只认对象。
//!
//! serde_json 读结构体时 `{…}`、`[…]` 两种都认：数组照字段的先后一个个填（`visit_seq`）。我们读的结构体从来都是对象：日志、
//! 缓存、快照是 serde_json 自己写的（写结构体只写对象），供应商、MCP 的回应、目录也都是对象。可派生出来的每个结构体的读法都
//! 带着这份认数组的代码，整体优化以后内联进各处，量下来整个程序多出约 100 kB（LM-10 量的）。
//!
//! 这里包一层：外面的读法照常交给 serde_json，只有「读一个结构体」改成「读一张表」（serde_json 的 `deserialize_map`：对象
//! 照常读，别的报「invalid type: …, expected struct …」），认数组的那一半就没人调、编不进来。往里一层层的值也经这一层读
//! （访问表、数组、枚举的那几样都包上），所以里面的结构体一样只认对象。表的键、枚举的种类名不包：它们是字，不是结构体。
//!
//! 读出来的和 serde_json 的一模一样，只差一处：本来写成数组的结构体，以前照字段先后读得出，现在报错（我们写的、别人发来的都
//! 没有这样写的）。工具的参数是模型写的：最外一层到得了工具的一定是对象（内核照参数格式修正时不是对象的当场回错，
//! [`crate::kernel::tool::repair`]），里面没有结构体的工具也走这里，读出来的一点不变；里层有结构体、模型可能写成数组的
//! （`ask_user` 的题和选项）和界面照模型原样的参数排差异的（`litemiyu` 的 `diff.rs`）照旧用 serde_json 原来的读法。
//!
//! 读字节的也走读字符串的那一份（[`from_slice`]）：一种读法只展开一份。

use std::fmt;

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde::de::{
    self, DeserializeSeed, Deserializer, EnumAccess, MapAccess, SeqAccess, VariantAccess, Visitor,
};

/// 同 `serde_json::from_str`：读完一个值，后面只许有空白。结构体只认对象（见模块的说明）。
///
/// # Errors
///
/// 同 `serde_json::from_str`，原话一样。
pub fn from_str<'a, T: Deserialize<'a>>(text: &'a str) -> serde_json::Result<T> {
    let mut d = serde_json::Deserializer::from_str(text);
    let value = T::deserialize(Objects(&mut d))?;
    d.end()?;
    Ok(value)
}

/// 同 `serde_json::from_slice`：读完一个值，后面只许有空白。结构体只认对象（见模块的说明）。
///
/// 先查整段是不是 UTF-8，是的照 [`from_str`] 读：同一份读法，serde_json 不用为字节另展开一份（LM-10，程序小 19 kB）。
/// 读得成、读不成的和 `serde_json::from_slice` 一样（JSON 在字符串以外只有 ASCII，字符串里坏了的 UTF-8 它也报错）。
///
/// # Errors
///
/// 同 `serde_json::from_slice`，原话一样；只有带坏的 UTF-8 的，原话换成标准库说的哪个字节起不是 UTF-8。
pub fn from_slice<'a, T: Deserialize<'a>>(bytes: &'a [u8]) -> serde_json::Result<T> {
    match std::str::from_utf8(bytes) {
        Ok(text) => from_str(text),
        Err(error) => Err(<serde_json::Error as de::Error>::custom(error)),
    }
}

/// 同 `serde_json::from_value`：照一棵读好的 [`serde_json::Value`] 读。结构体只认对象（见模块的说明）。
///
/// # Errors
///
/// 同 `serde_json::from_value`，原话一样。
pub fn from_value<T: DeserializeOwned>(value: serde_json::Value) -> serde_json::Result<T> {
    T::deserialize(Objects(value))
}

/// 同 `T::deserialize(&value)`：照一棵读好的 [`serde_json::Value`] 读，不拿走它。结构体只认对象（见模块的说明）。
///
/// # Errors
///
/// 同 `T::deserialize(&value)`，原话一样。
pub fn from_value_ref<'a, T: Deserialize<'a>>(
    value: &'a serde_json::Value,
) -> serde_json::Result<T> {
    T::deserialize(Objects(value))
}

/// 包着一个读法：结构体照表读，里面的值也经这一层（见模块的说明）。
pub struct Objects<D>(pub D);

/// 照样转给里面那一个，访问者包上一层。
macro_rules! forward {
    ($($method:ident),+ $(,)?) => {
        $(
            fn $method<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, D::Error> {
                self.0.$method(Wrap(visitor))
            }
        )+
    };
}

impl<'de, D: Deserializer<'de>> Deserializer<'de> for Objects<D> {
    type Error = D::Error;

    forward!(
        deserialize_any,
        deserialize_bool,
        deserialize_i8,
        deserialize_i16,
        deserialize_i32,
        deserialize_i64,
        deserialize_i128,
        deserialize_u8,
        deserialize_u16,
        deserialize_u32,
        deserialize_u64,
        deserialize_u128,
        deserialize_f32,
        deserialize_f64,
        deserialize_char,
        deserialize_str,
        deserialize_string,
        deserialize_bytes,
        deserialize_byte_buf,
        deserialize_option,
        deserialize_unit,
        deserialize_seq,
        deserialize_map,
        deserialize_identifier,
        deserialize_ignored_any,
    );

    fn deserialize_unit_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        visitor: V,
    ) -> Result<V::Value, D::Error> {
        self.0.deserialize_unit_struct(name, Wrap(visitor))
    }

    /// 名字照传：serde_json 照名字认出原样的 JSON（`RawValue`）。
    fn deserialize_newtype_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        visitor: V,
    ) -> Result<V::Value, D::Error> {
        self.0.deserialize_newtype_struct(name, Wrap(visitor))
    }

    fn deserialize_tuple<V: Visitor<'de>>(
        self,
        len: usize,
        visitor: V,
    ) -> Result<V::Value, D::Error> {
        self.0.deserialize_tuple(len, Wrap(visitor))
    }

    fn deserialize_tuple_struct<V: Visitor<'de>>(
        self,
        name: &'static str,
        len: usize,
        visitor: V,
    ) -> Result<V::Value, D::Error> {
        self.0.deserialize_tuple_struct(name, len, Wrap(visitor))
    }

    /// 结构体照表读：不认数组。
    fn deserialize_struct<V: Visitor<'de>>(
        self,
        _name: &'static str,
        _fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, D::Error> {
        self.0.deserialize_map(Wrap(visitor))
    }

    fn deserialize_enum<V: Visitor<'de>>(
        self,
        name: &'static str,
        variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, D::Error> {
        self.0.deserialize_enum(name, variants, Wrap(visitor))
    }

    fn is_human_readable(&self) -> bool {
        self.0.is_human_readable()
    }
}

/// 包着一个访问者：交给它的表、数组、枚举、里面的值都包上。
struct Wrap<V>(V);

/// 照样转给里面那一个。
macro_rules! visit {
    ($($method:ident($ty:ty)),+ $(,)?) => {
        $(
            fn $method<E: de::Error>(self, value: $ty) -> Result<V::Value, E> {
                self.0.$method(value)
            }
        )+
    };
}

impl<'de, V: Visitor<'de>> Visitor<'de> for Wrap<V> {
    type Value = V::Value;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.expecting(f)
    }

    visit!(
        visit_bool(bool),
        visit_i8(i8),
        visit_i16(i16),
        visit_i32(i32),
        visit_i64(i64),
        visit_i128(i128),
        visit_u8(u8),
        visit_u16(u16),
        visit_u32(u32),
        visit_u64(u64),
        visit_u128(u128),
        visit_f32(f32),
        visit_f64(f64),
        visit_char(char),
        visit_str(&str),
        visit_borrowed_str(&'de str),
        visit_string(String),
        visit_bytes(&[u8]),
        visit_borrowed_bytes(&'de [u8]),
        visit_byte_buf(Vec<u8>),
    );

    fn visit_none<E: de::Error>(self) -> Result<V::Value, E> {
        self.0.visit_none()
    }

    fn visit_unit<E: de::Error>(self) -> Result<V::Value, E> {
        self.0.visit_unit()
    }

    fn visit_some<D: Deserializer<'de>>(self, d: D) -> Result<V::Value, D::Error> {
        self.0.visit_some(Objects(d))
    }

    fn visit_newtype_struct<D: Deserializer<'de>>(self, d: D) -> Result<V::Value, D::Error> {
        self.0.visit_newtype_struct(Objects(d))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, seq: A) -> Result<V::Value, A::Error> {
        self.0.visit_seq(Seq(seq))
    }

    fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<V::Value, A::Error> {
        self.0.visit_map(Map(map))
    }

    fn visit_enum<A: EnumAccess<'de>>(self, data: A) -> Result<V::Value, A::Error> {
        self.0.visit_enum(Enum(data))
    }
}

/// 包着一个值的读法：读的时候包上。
struct Seed<S>(S);

impl<'de, S: DeserializeSeed<'de>> DeserializeSeed<'de> for Seed<S> {
    type Value = S::Value;

    fn deserialize<D: Deserializer<'de>>(self, d: D) -> Result<S::Value, D::Error> {
        self.0.deserialize(Objects(d))
    }
}

/// 数组：每一个元素包上。
struct Seq<A>(A);

impl<'de, A: SeqAccess<'de>> SeqAccess<'de> for Seq<A> {
    type Error = A::Error;

    fn next_element_seed<S: DeserializeSeed<'de>>(
        &mut self,
        seed: S,
    ) -> Result<Option<S::Value>, A::Error> {
        self.0.next_element_seed(Seed(seed))
    }

    fn size_hint(&self) -> Option<usize> {
        self.0.size_hint()
    }
}

/// 表：值包上，键不包（JSON 的键是字）。
struct Map<A>(A);

impl<'de, A: MapAccess<'de>> MapAccess<'de> for Map<A> {
    type Error = A::Error;

    fn next_key_seed<K: DeserializeSeed<'de>>(
        &mut self,
        seed: K,
    ) -> Result<Option<K::Value>, A::Error> {
        self.0.next_key_seed(seed)
    }

    fn next_value_seed<S: DeserializeSeed<'de>>(&mut self, seed: S) -> Result<S::Value, A::Error> {
        self.0.next_value_seed(Seed(seed))
    }

    fn size_hint(&self) -> Option<usize> {
        self.0.size_hint()
    }
}

/// 枚举：种类名不包，内容包上。
struct Enum<A>(A);

impl<'de, A: EnumAccess<'de>> EnumAccess<'de> for Enum<A> {
    type Error = A::Error;
    type Variant = Variant<A::Variant>;

    fn variant_seed<S: DeserializeSeed<'de>>(
        self,
        seed: S,
    ) -> Result<(S::Value, Variant<A::Variant>), A::Error> {
        let (value, variant) = self.0.variant_seed(seed)?;
        Ok((value, Variant(variant)))
    }
}

/// 枚举的内容。
struct Variant<A>(A);

impl<'de, A: VariantAccess<'de>> VariantAccess<'de> for Variant<A> {
    type Error = A::Error;

    fn unit_variant(self) -> Result<(), A::Error> {
        self.0.unit_variant()
    }

    fn newtype_variant_seed<S: DeserializeSeed<'de>>(self, seed: S) -> Result<S::Value, A::Error> {
        self.0.newtype_variant_seed(Seed(seed))
    }

    fn tuple_variant<V: Visitor<'de>>(self, len: usize, visitor: V) -> Result<V::Value, A::Error> {
        self.0.tuple_variant(len, Wrap(visitor))
    }

    /// 带字段的那一种：serde_json 照结构体读，这里只给它表的读法（[`MapOnly`]）。
    fn struct_variant<V: Visitor<'de>>(
        self,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, A::Error> {
        self.0.struct_variant(fields, MapOnly(visitor))
    }
}

/// 只认表的访问者：别的（数组也在内）照 serde 默认的报「invalid type」，不去调里面那一个认数组的读法。
struct MapOnly<V>(V);

impl<'de, V: Visitor<'de>> Visitor<'de> for MapOnly<V> {
    type Value = V::Value;

    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.expecting(f)
    }

    fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<V::Value, A::Error> {
        self.0.visit_map(Map(map))
    }
}

#[cfg(test)]
mod tests;
