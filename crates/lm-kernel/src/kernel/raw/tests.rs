//! 认标签的快路（施工 V-10）：标签在开头、不带转义的直接认出来；别的写法（不在开头、带转义、不是字、缺了）交回没有，
//! 照整表认。两条路读出来的块、`by` 一样；不认识的种类照旧原样留着。

use super::*;
use crate::kernel::block::Block;
use crate::kernel::origin::By;

#[test]
fn the_leading_tag_is_read_without_parsing_the_whole_block() {
    assert_eq!(
        leading_tag(r#"{"type":"text","text":"hi"}"#, "type"),
        Some("text")
    );
    assert_eq!(
        leading_tag(" { \"type\" : \"image\" }", "type"),
        Some("image")
    );
    assert_eq!(
        leading_tag(r#"{"text":"hi","type":"text"}"#, "type"),
        None,
        "不在开头"
    );
    assert_eq!(
        leading_tag(r#"{"type":"te\u0078t"}"#, "type"),
        None,
        "带转义"
    );
    assert_eq!(leading_tag(r#"{"typed":"text"}"#, "type"), None, "别的键");
    assert_eq!(leading_tag(r#"{"type":1}"#, "type"), None, "不是字");
    assert_eq!(leading_tag(r#"{"kind":"kernel"}"#, "kind"), Some("kernel"));
    assert_eq!(leading_tag("[]", "type"), None);
}

#[test]
fn both_ways_read_the_same() {
    let fast: Block = serde_json::from_str(r#"{"type":"text","text":"a \"b\""}"#).unwrap();
    let slow: Block = serde_json::from_str(r#"{"text":"a \"b\"","type":"text"}"#).unwrap();
    assert_eq!(fast, slow);
    let escaped: Block = serde_json::from_str(r#"{"type":"te\u0078t","text":"x"}"#).unwrap();
    assert!(matches!(escaped, Block::Text(_)), "带转义的照整表认");
    let unknown: Block = serde_json::from_str(r#"{"type":"hologram","depth":3}"#).unwrap();
    assert!(
        matches!(&unknown, Block::Unknown(raw) if raw.get() == r#"{"type":"hologram","depth":3}"#),
        "不认识的原样留着"
    );
    let by: By = serde_json::from_str(r#"{"kind":"kernel"}"#).unwrap();
    let late: By = serde_json::from_str(r#"{ "kind" : "kernel" }"#).unwrap();
    assert_eq!(by, late);
    assert!(
        serde_json::from_str::<Block>(r#"{"text":"x"}"#).is_err(),
        "缺了标签照旧报错"
    );
}

/// 按标签分派的三种（块、`by`、效果）认识的种类内容坏了的：原话就是照那一种单读这一块时 serde_json 的原话，连位置
/// （LM-10 改成只起一个读法、读完查一次后面时守着；改前的代码上同样过）。
#[test]
fn a_broken_known_kind_says_what_its_own_type_says() {
    use crate::kernel::block::{Block, Image, Text};
    use crate::kernel::event::{Effect, FileRead};
    use crate::kernel::origin::{By, Model};

    fn same<T: serde::de::DeserializeOwned, U: serde::de::DeserializeOwned>(bad: &str) {
        let tagged = serde_json::from_str::<T>(bad).err().map(|e| e.to_string());
        let alone = serde_json::from_str::<U>(bad).err().map(|e| e.to_string());
        assert!(alone.is_some(), "{bad}");
        assert_eq!(tagged, alone, "{bad}");
    }
    same::<Block, Text>(r#"{"type":"text"}"#);
    same::<Block, Text>(r#"{"type":"text","text":7}"#);
    same::<Block, Text>(r#"{"text":"a","type":"text","text":"b"}"#);
    same::<Block, Image>(
        r#"{"type":"image","blob":"x","media_type":"image/png","width":1,"height":1}"#,
    );
    same::<By, Model>(r#"{"kind":"model","endpoint":7,"model":"m"}"#);
    same::<Effect, FileRead>(r#"{"kind":"file.read","path":1}"#);
    assert_eq!(
        serde_json::from_str::<By>(r#"{"kind":"kernel","extra":[1,{"a":null}]}"#).unwrap(),
        By::Kernel
    );
}
