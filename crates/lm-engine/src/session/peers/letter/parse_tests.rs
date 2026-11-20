//! 手写的读法和原来派生的读法读出来一样（引擎线第八趟瘦身）：原来的 `Letter` 照原样（格子、serde 的标注；LM-12 起 `watch` 多一格
//! 可以没有的 `wall`，照同样的标注加在这里）抄在这里，同一份字
//! 两种读法各读一遍，要么都读得出、写回去一字不差，要么都报错。四种各一封，再逐格改：删掉、换成别的类型和边上的数；`kind`
//! 换成不认识的、别的类型；多一个不认识的格子；整封写成数组（长短不一）、别的类型。

use serde_json::{Value, json};

use lm_kernel::kernel::id::{CallId, SessionId};

/// 原来的读法。
mod old {
    use lm_kernel::kernel::id::{CallId, SessionId};
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize)]
    #[serde(tag = "kind", rename_all = "lowercase")]
    pub enum Letter {
        Message {
            from: SessionId,
            call: CallId,
            text: String,
        },
        Watch {
            from: SessionId,
            since: i64,
            #[serde(default, skip_serializing_if = "Option::is_none")]
            wall: Option<i64>,
        },
        Idle {
            from: SessionId,
            since: i64,
            #[serde(default, skip_serializing_if = "Option::is_none")]
            status: Option<String>,
        },
        Closed {
            from: SessionId,
            since: i64,
        },
    }
}

const FROM: &str = "019a0000-0000-7000-8000-000000000001";

/// 两种读法读 `json`：都读得出的写回去要一样，读不出的要都读不出。
fn same(json: &Value) {
    let bytes = json.to_string().into_bytes();
    let new = super::parse::letter(&bytes);
    let old = serde_json::from_slice::<old::Letter>(&bytes);
    match (new, old) {
        (Ok(new), Ok(old)) => assert_eq!(
            serde_json::to_value(&new).expect("写得出"),
            serde_json::to_value(&old).expect("写得出"),
            "{json}"
        ),
        (Err(_), Err(_)) => {}
        (new, old) => panic!(
            "一个读得出一个读不出：{json}\n新：{:?}\n旧：{:?}",
            new.map(|_| ()),
            old.map(|_| ())
        ),
    }
}

/// 换进一格的那些值。
fn replacements() -> Vec<Value> {
    [
        "null",
        "\"\"",
        "\"x\"",
        "\"call_3_1\"",
        "\"019a0000-0000-7000-8000-000000000002\"",
        "true",
        "0",
        "-1",
        "1.5",
        "9223372036854775807",
        "9223372036854775808",
        "-9223372036854775808",
        "[]",
        "{}",
    ]
    .iter()
    .map(|text| serde_json::from_str(text).expect("写得对"))
    .collect()
}

/// 四种各一封，照原来的写法写出来的。
fn letters() -> Vec<Value> {
    let from = SessionId::parse(FROM).expect("写得对");
    let call = CallId::parse("call_7_2").expect("写得对");
    [
        old::Letter::Message {
            from: from.clone(),
            call,
            text: "在吗".to_string(),
        },
        old::Letter::Watch {
            from: from.clone(),
            since: 1_700_000_000_000,
            wall: Some(1_600_000_000_000),
        },
        old::Letter::Watch {
            from: from.clone(),
            since: 1_700_000_000_000,
            wall: None,
        },
        old::Letter::Idle {
            from: from.clone(),
            since: 5,
            status: Some("做完了".to_string()),
        },
        old::Letter::Idle {
            from: from.clone(),
            since: 5,
            status: None,
        },
        old::Letter::Closed { from, since: -3 },
    ]
    .iter()
    .map(|letter| serde_json::to_value(letter).expect("写得出"))
    .collect()
}

#[test]
fn every_change_reads_the_same() {
    for base in letters() {
        let Value::Object(map) = &base else {
            panic!("是对象：{base}");
        };
        same(&base);
        for key in map
            .keys()
            .chain(["status".to_string(), "wall".to_string()].iter())
        {
            let mut removed = map.clone();
            removed.remove(key);
            same(&Value::Object(removed));
            for replacement in replacements() {
                let mut changed = map.clone();
                changed.insert(key.clone(), replacement);
                same(&Value::Object(changed));
            }
        }
        for kind in ["Message", "other", "message", "watch", "idle", "closed"] {
            let mut changed = map.clone();
            changed.insert("kind".to_string(), json!(kind));
            same(&Value::Object(changed));
        }
        let mut extra = map.clone();
        extra.insert("not_a_field".to_string(), json!({"x": [1, null]}));
        same(&Value::Object(extra));
        // 整封写成数组：头一个是 `kind`，后面照格子的先后；少一格、正好、多一格。
        let kind = map["kind"].clone();
        let mut fields: Vec<Value> = ["from", "call", "text", "since", "status", "wall"]
            .iter()
            .filter_map(|key| map.get(*key).cloned())
            .collect();
        for _ in 0..2 {
            let mut array = vec![kind.clone()];
            array.extend(fields.iter().cloned());
            same(&Value::Array(array.clone()));
            array.push(Value::Null);
            same(&Value::Array(array));
            fields.pop();
        }
    }
    for other in ["null", "1", "\"message\"", "[]", "[\"watch\"]", "[1]", "{}"] {
        same(&serde_json::from_str(other).expect("写得对"));
    }
}

#[test]
fn errors_say_which_field() {
    let said = |json: Value| super::parse::letter(json.to_string().as_bytes()).expect_err("读不出");
    assert_eq!(
        said(json!({"from": FROM, "since": 1})),
        "missing field `kind`"
    );
    assert!(said(json!({"kind": "watch", "from": FROM})).contains("missing field `since`"));
    assert!(
        said(json!({"kind": "watch", "from": FROM, "since": 1.5}))
            .contains("at `since`, expected i64")
    );
    assert!(said(json!({"kind": "watch", "from": "x", "since": 1})).contains("at `from`"));
    assert!(
        said(json!({"kind": "watch", "from": FROM, "since": 1, "wall": "x"}))
            .contains("at `wall`, expected i64")
    );
    assert!(said(json!({"kind": "nope"})).contains("unknown variant `nope`"));
}
