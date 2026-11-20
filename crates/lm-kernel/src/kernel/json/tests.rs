//! 包一层读出来的和 serde_json 原来的一模一样（值、报错的原话），只差写成数组的结构体。

use std::collections::BTreeMap;

use serde::Deserialize;
use serde_json::value::RawValue;

use super::{from_slice, from_str, from_value, from_value_ref};

#[derive(Debug, PartialEq, Deserialize)]
struct Inner {
    a: u32,
    #[serde(default)]
    b: Option<String>,
}

#[derive(Debug, PartialEq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Outer {
    Unit,
    New(Inner),
    Tuple(u8, u8),
    Fields { x: Inner, y: Vec<Inner> },
}

#[derive(Debug, PartialEq, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Tagged {
    One { inner: Inner },
    Two(Inner),
}

#[derive(Debug, PartialEq, Deserialize)]
#[serde(untagged)]
enum Either {
    Word(String),
    Inner(Inner),
}

#[derive(Debug, PartialEq, Deserialize)]
struct Flat {
    #[serde(flatten)]
    inner: Inner,
    c: bool,
}

#[derive(Debug, Deserialize)]
struct Everything {
    inner: Inner,
    list: Vec<Inner>,
    map: BTreeMap<String, Inner>,
    opt: Option<Box<Inner>>,
    outer: Vec<Outer>,
    tagged: Vec<Tagged>,
    either: Vec<Either>,
    flat: Flat,
    raw: Box<RawValue>,
    value: serde_json::Value,
    pair: (u8, String),
}

/// 一份把各种写法都用上的：包一层读的和 serde_json 读的逐格一样，原样的那一块一字不差。
const EVERYTHING: &str = r#"{
  "inner": {"a": 1, "b": "x"},
  "list": [{"a": 2}, {"b": null, "a": 3}],
  "map": {"k": {"a": 4}},
  "opt": {"a": 5},
  "outer": ["unit", {"new": {"a": 6}}, {"tuple": [7, 8]}, {"fields": {"x": {"a": 9}, "y": [{"a": 10}]}}],
  "tagged": [{"kind": "one", "inner": {"a": 11}}, {"a": 12, "kind": "two"}],
  "either": ["w", {"a": 13}],
  "flat": {"a": 14, "c": true, "b": "f"},
  "raw": { "keep" : [1.50, {"as": "is"}] },
  "value": {"z": [1, 2.5, "s", null, true, {"n": {}}]},
  "pair": [15, "p"],
  "unknown": [1, {"ignored": true}]
}"#;

fn same(ours: &Everything, theirs: &Everything) {
    assert_eq!(ours.inner, theirs.inner);
    assert_eq!(ours.list, theirs.list);
    assert_eq!(ours.map, theirs.map);
    assert_eq!(ours.opt, theirs.opt);
    assert_eq!(ours.outer, theirs.outer);
    assert_eq!(ours.tagged, theirs.tagged);
    assert_eq!(ours.either, theirs.either);
    assert_eq!(ours.flat, theirs.flat);
    assert_eq!(ours.raw.get(), theirs.raw.get());
    assert_eq!(ours.value, theirs.value);
    assert_eq!(ours.pair, theirs.pair);
}

#[test]
fn reads_the_same_as_serde_json() {
    let ours: Everything = from_str(EVERYTHING).unwrap();
    let theirs: Everything = serde_json::from_str(EVERYTHING).unwrap();
    same(&ours, &theirs);
    assert_eq!(ours.raw.get(), r#"{ "keep" : [1.50, {"as": "is"}] }"#);
    let ours: Everything = from_slice(EVERYTHING.as_bytes()).unwrap();
    same(&ours, &theirs);
}

/// 坏的报错和 serde_json 原话一样（连位置）：少字段、类型不对、不认识的种类、后面多了字、半截。
#[test]
fn errors_say_the_same_as_serde_json() {
    for bad in [
        r#"{"b": "x"}"#,
        r#"{"a": "x"}"#,
        r#"{"a": 1, "a": 2}"#,
        r#"{"a": 1} x"#,
        r#"{"a": 1"#,
        r#""text""#,
        "null",
        "",
    ] {
        let ours = from_str::<Inner>(bad).unwrap_err().to_string();
        let theirs = serde_json::from_str::<Inner>(bad).unwrap_err().to_string();
        assert_eq!(ours, theirs, "{bad}");
        let ours = from_slice::<Inner>(bad.as_bytes()).unwrap_err().to_string();
        assert_eq!(ours, theirs, "{bad}");
    }
    for bad in [
        r#"{"three": {}}"#,
        r#"{"fields": {"x": {"a": 1}}}"#,
        r#"{"new": {"a": -1}}"#,
        r#"{"tuple": [1]}"#,
    ] {
        let ours = from_str::<Outer>(bad).unwrap_err().to_string();
        let theirs = serde_json::from_str::<Outer>(bad).unwrap_err().to_string();
        assert_eq!(ours, theirs, "{bad}");
    }
    for bad in [r#"{"kind": "three"}"#, r#"{"inner": {"a": 1}}"#] {
        let ours = from_str::<Tagged>(bad).unwrap_err().to_string();
        let theirs = serde_json::from_str::<Tagged>(bad).unwrap_err().to_string();
        assert_eq!(ours, theirs, "{bad}");
    }
}

/// 只差这一处：写成数组的结构体（serde_json 照字段先后读）不认，报类型不对。里面一层的也一样。
#[test]
fn a_struct_written_as_an_array_is_refused() {
    assert_eq!(
        serde_json::from_str::<Inner>(r#"[1, "x"]"#).unwrap(),
        Inner {
            a: 1,
            b: Some("x".to_string())
        }
    );
    let error = from_str::<Inner>(r#"[1, "x"]"#).unwrap_err().to_string();
    assert!(
        error.starts_with("invalid type: sequence, expected struct Inner"),
        "{error}"
    );
    assert!(from_str::<Vec<Inner>>(r#"[[1]]"#).is_err());
    assert!(from_str::<Outer>(r#"{"fields": [{"a": 1}, []]}"#).is_err());
    assert!(from_str::<Outer>(r#"{"new": [1]}"#).is_err());
}

/// 读字节的走读字符串的那一份：合法的读出来一样；带坏的 UTF-8 的（字符串里、字符串外）和 serde_json 一样读不成。
#[test]
fn bytes_read_like_text_and_bad_utf8_is_refused_like_serde_json() {
    for bad in [
        &b"{\"a\": 1, \"b\": \"\xff\"}"[..],
        b"{\"a\": 1, \xff}",
        b"\xef\xbb\xbf{\"a\": 1}",
        b"{\"a\": 1}\xc3",
    ] {
        assert!(serde_json::from_slice::<Inner>(bad).is_err());
        assert!(from_slice::<Inner>(bad).is_err());
    }
    let good = "{\"a\": 1, \"b\": \"字 \\ud83d\\ude00\"}".as_bytes();
    assert_eq!(
        from_slice::<Inner>(good).unwrap(),
        serde_json::from_slice::<Inner>(good).unwrap()
    );
}

/// 照一棵读好的 Value 读（拿走的、借着的）：和 serde_json 读的逐格一样；写成数组的结构体同样不认。
#[test]
fn values_read_the_same_as_serde_json() {
    let tree: serde_json::Value = serde_json::from_str(EVERYTHING).unwrap();
    let theirs: Everything = serde_json::from_value(tree.clone()).unwrap();
    let ours: Everything = from_value(tree.clone()).unwrap();
    same(&ours, &theirs);
    let ours: Everything = from_value_ref(&tree).unwrap();
    same(&ours, &theirs);
    for bad in [
        serde_json::json!({"b": "x"}),
        serde_json::json!({"a": "x"}),
        serde_json::json!("text"),
    ] {
        let ours = from_value::<Inner>(bad.clone()).unwrap_err().to_string();
        let theirs = serde_json::from_value::<Inner>(bad.clone())
            .unwrap_err()
            .to_string();
        assert_eq!(ours, theirs, "{bad}");
        let ours = from_value_ref::<Inner>(&bad).unwrap_err().to_string();
        assert_eq!(ours, theirs, "{bad}");
    }
    let array = serde_json::json!([1, "x"]);
    assert!(serde_json::from_value::<Inner>(array.clone()).is_ok());
    assert!(from_value::<Inner>(array.clone()).is_err());
    assert!(from_value_ref::<Inner>(&array).is_err());
}
