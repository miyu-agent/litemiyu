//! 快照里的数、别的会话的标签（引擎线第八趟瘦身）换成按名字取以后，和原来派生的读法读出来一样：原来的结构照原样（格子、serde
//! 的标注）抄在这里，同一份字两种读法各读一遍，要么都读得出、写回去一字不差，要么都报错。字照出厂的快照，再逐格改：删掉、写成
//! `null`、写成字串、开关、负数、小数、超了 `u32` 的、`u64` 最大的、`-0`，多一个不认识的格子，整个换成数组（长短不一）、`null`。

use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;

/// 原来的读法：结构和 serde 的标注照搬原来的（文档注释略去）。
pub(crate) mod old {
    use serde::{Deserialize, Serialize};

    fn default_tail() -> u64 {
        16_000
    }
    fn default_lead() -> u64 {
        16_000
    }
    fn default_line_percent() -> u64 {
        85
    }
    fn default_margin_percent() -> u64 {
        5
    }

    #[derive(Serialize, Deserialize)]
    pub struct CompactionNumbers {
        pub reserve_cap: u64,
        pub margin: u64,
        #[serde(default = "default_line_percent")]
        pub line_percent: u64,
        #[serde(default = "default_margin_percent")]
        pub margin_percent: u64,
        pub image: u64,
        pub file: u64,
        #[serde(default = "default_tail")]
        pub tail: u64,
        #[serde(default = "default_lead")]
        pub lead: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub rebuild: Option<RebuildNumbers>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub pause: Option<PauseNumbers>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub shorten: Option<ShortenNumbers>,
    }

    #[derive(Serialize, Deserialize)]
    pub struct RebuildNumbers {
        pub files: usize,
        pub file_tokens: u64,
        pub total: u64,
        pub min_window: u64,
        pub candidates: usize,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub skills: Option<SkillNumbers>,
    }

    #[derive(Serialize, Deserialize)]
    pub struct SkillNumbers {
        pub tokens: u64,
        pub total: u64,
    }

    #[derive(Serialize, Deserialize)]
    pub struct PauseNumbers {
        pub failures: u32,
        pub turns: u32,
        pub refills: u32,
    }

    #[derive(Serialize, Deserialize)]
    pub struct ShortenNumbers {
        pub tries: u32,
        pub percent: u32,
    }

    #[derive(Serialize, Deserialize)]
    pub struct JobNumbers {
        pub report_chars: u64,
    }

    #[derive(Serialize, Deserialize)]
    pub struct RecapNumbers {
        pub turns: u32,
        pub tokens: u64,
    }

    #[derive(Serialize, Deserialize)]
    pub struct TitleNumbers {
        pub tokens: u64,
        pub chars: u32,
    }

    #[derive(Serialize, Deserialize)]
    pub struct PeerNumbers {
        pub burst: u64,
        pub window: u64,
        pub unread: u64,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub watch_hours: Option<u64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub status_chars: Option<u64>,
    }

    #[derive(Serialize, Deserialize)]
    pub struct PeerTexts {
        pub message_open: String,
        pub message_close: String,
        #[serde(flatten, default, skip_serializing_if = "Option::is_none")]
        pub idle: Option<PeerIdleTexts>,
    }

    #[derive(Serialize, Deserialize)]
    pub struct PeerIdleTexts {
        pub idle_open: String,
        pub idle_silent: String,
        pub idle_expired: String,
        pub idle_gone: String,
        pub idle_close: String,
    }
}

/// 两种读法读 `json`：都读得出的写回去要一样，读不出的要都读不出。
fn same<New: DeserializeOwned + Serialize, Old: DeserializeOwned + Serialize>(json: &Value) {
    let text = json.to_string();
    let new = serde_json::from_str::<New>(&text);
    let old = serde_json::from_str::<Old>(&text);
    match (new, old) {
        (Ok(new), Ok(old)) => assert_eq!(
            serde_json::to_value(&new).expect("写得出"),
            serde_json::to_value(&old).expect("写得出"),
            "{text}"
        ),
        (Err(_), Err(_)) => {}
        (new, old) => panic!(
            "一个读得出一个读不出：{text}\n新：{:?}\n旧：{:?}",
            new.map(|_| ()),
            old.map(|_| ())
        ),
    }
}

/// 换进一格的那些值：数的各种边上的、别的类型的。
fn replacements() -> Vec<Value> {
    [
        "null",
        "\"\"",
        "\"3\"",
        "true",
        "0",
        "-0",
        "-1",
        "1.5",
        "2.0",
        "1e3",
        "4294967295",
        "4294967296",
        "18446744073709551615",
        "18446744073709551616",
        "[]",
        "{}",
    ]
    .iter()
    .map(|text| serde_json::from_str(text).expect("写得对"))
    .collect()
}

/// `base` 照原样，再逐格改：删掉、换成 [`replacements`] 的每一个；嵌着的对象里面照样逐格改（一层层往下）；另加一个不认识的
/// 格子；整个换成数组、`null`。
fn changes(base: &Value) -> Vec<Value> {
    let Value::Object(map) = base else {
        panic!("是对象：{base}");
    };
    let mut all = vec![base.clone()];
    for (key, value) in map {
        let mut removed = map.clone();
        removed.remove(key);
        all.push(Value::Object(removed));
        for replacement in replacements() {
            let mut changed = map.clone();
            changed.insert(key.clone(), replacement);
            all.push(Value::Object(changed));
        }
        if value.is_object() {
            for inner in changes(value) {
                let mut changed = map.clone();
                changed.insert(key.clone(), inner);
                all.push(Value::Object(changed));
            }
        }
    }
    let mut extra = map.clone();
    extra.insert(
        "not_a_field".to_string(),
        serde_json::json!({"x": [1, null]}),
    );
    all.push(Value::Object(extra));
    all.push(serde_json::json!([1, 2]));
    all.push(Value::Null);
    // 整份写成数组（serde 派生的读法照格子的先后认）：少一格、正好、多一格，全是 1 的、带 `null` 的。
    for len in [map.len() - 1, map.len(), map.len() + 1] {
        all.push(Value::Array(vec![Value::from(1); len]));
        if len > 0 {
            let mut with_null = vec![Value::from(1); len];
            with_null[len - 1] = Value::Null;
            all.push(Value::Array(with_null));
        }
    }
    all
}

/// [`changes`] 的每一份两种读法比一遍。
fn every_change<New: DeserializeOwned + Serialize, Old: DeserializeOwned + Serialize>(
    base: &Value,
) {
    for json in changes(base) {
        same::<New, Old>(&json);
    }
}

/// 出厂的快照写成的 JSON 里的一格。
fn shipped(key: &str) -> Value {
    let snapshot = serde_json::to_value(crate::policy::test_support::engineer()).expect("写得出");
    snapshot
        .get(key)
        .cloned()
        .unwrap_or_else(|| panic!("出厂的快照里有 {key}"))
}

#[test]
fn numbers_read_the_same() {
    let compaction = shipped("compaction");
    every_change::<crate::policy::CompactionNumbers, old::CompactionNumbers>(&compaction);
    every_change::<crate::policy::RebuildNumbers, old::RebuildNumbers>(&compaction["rebuild"]);
    every_change::<crate::policy::SkillNumbers, old::SkillNumbers>(
        &compaction["rebuild"]["skills"],
    );
    every_change::<crate::policy::PauseNumbers, old::PauseNumbers>(&compaction["pause"]);
    every_change::<crate::policy::ShortenNumbers, old::ShortenNumbers>(&compaction["shorten"]);
    every_change::<crate::policy::JobNumbers, old::JobNumbers>(&shipped("jobs"));
    every_change::<crate::policy::RecapNumbers, old::RecapNumbers>(&shipped("recap"));
    every_change::<crate::policy::TitleNumbers, old::TitleNumbers>(&shipped("title"));
    every_change::<crate::policy::PeerNumbers, old::PeerNumbers>(&shipped("peers"));
}

#[test]
fn peer_texts_read_the_same() {
    let core = shipped("core");
    let peers = &core["peers"];
    assert!(
        peers.get("idle_open").is_some(),
        "出厂的带通知的字：{peers}"
    );
    every_change::<crate::policy::PeerTexts, old::PeerTexts>(peers);
    // C-2 时造的：没有通知的五格。
    let mut bare = peers.clone();
    let map = bare.as_object_mut().expect("是对象");
    map.retain(|key, _| !key.starts_with("idle_"));
    every_change::<crate::policy::PeerTexts, old::PeerTexts>(&bare);
}

/// 报错的话：缺了必填的、类型不对的、放不下的，说是哪一格。
#[test]
fn number_errors_say_which_field() {
    let read = |json: Value| {
        serde_json::from_value::<crate::policy::PauseNumbers>(json)
            .expect_err("读不出")
            .to_string()
    };
    let base = shipped("compaction")["pause"].clone();
    let mut missing = base.clone();
    missing.as_object_mut().expect("是对象").remove("turns");
    assert!(read(missing).contains("missing field `turns`"));
    for (value, said) in [
        ("-1", "invalid value: integer `-1` at `turns`, expected u32"),
        (
            "4294967296",
            "invalid value: integer `4294967296` at `turns`, expected u32",
        ),
        (
            "1.5",
            "invalid type: floating point `1.5` at `turns`, expected u32",
        ),
        ("\"3\"", "invalid type: string at `turns`, expected u32"),
    ] {
        let mut changed = base.clone();
        changed["turns"] = serde_json::from_str(value).expect("写得对");
        let error = read(changed);
        assert!(error.contains(said), "{error}");
    }
}
