//! 按名字取的读法和原来派生的读法读出来一样（引擎线第七趟瘦身）：原来的结构照原样（格子、serde 的标注）抄在这里，派生
//! `Deserialize`；同一份字两种读法各读一遍，要么都读得出、写回去一字不差，要么都报错。字照出厂的那一份，再逐格改：删掉、写成
//! `null`、写成数、多一个不认识的格子。

use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;

/// 原来的读法：结构和 serde 的标注照搬原来的（文档注释略去）。带 `flatten` 的 `PeerTexts` 第八趟换了，原来的抄在 `numbers_tests.rs`。
mod old {
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize)]
    pub struct CoreTexts {
        pub checkpoint_open: String,
        pub checkpoint_close: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        pub checkpoint_end: String,
        pub turn_ended: TurnEndedTexts,
        pub facts: FactTexts,
        pub tool_results: ToolResultTexts,
        pub drivers: DriverPlaceholders,
        #[serde(default)]
        pub permissions: PermissionTexts,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub compaction: Option<CompactionTexts>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub jobs: Option<JobTexts>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub peers: Option<crate::policy::read::numbers_tests::old::PeerTexts>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub schedule: Option<ScheduleTexts>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub recap: Option<RecapTexts>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub title: Option<TitleTexts>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub vision: Option<VisionTexts>,
    }

    #[derive(Serialize, Deserialize)]
    pub struct ToolResultTexts {
        pub unknown: String,
        pub not_an_object: String,
        pub cancelled_before: String,
        pub cancelled_running: String,
        pub skipped: String,
        pub denied: String,
        pub denied_with_reason: String,
        pub unattended: String,
        pub question_interrupted: String,
        pub question_voided: String,
        pub question_unattended: String,
        pub restarted: String,
        #[serde(default)]
        pub unavailable: String,
        #[serde(default)]
        pub crashed: String,
        #[serde(default)]
        pub uninstalled: String,
    }

    #[derive(Serialize, Deserialize)]
    pub struct CompactionTexts {
        pub summarize_task: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        pub summarize_instructions: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        pub summarize_end: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub rebuild: Option<RebuildTexts>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub shorten: Option<ShortenTexts>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub summarize_system: Option<String>,
    }

    #[derive(Serialize, Deserialize, Default)]
    pub struct PermissionTexts {
        pub forbidden: String,
        pub unresolvable: String,
    }

    #[derive(Serialize, Deserialize)]
    pub struct TurnEndedTexts {
        pub interrupted: String,
        pub error: String,
        pub step_limit: String,
        pub aborted: String,
        pub restarted: String,
    }

    #[derive(Serialize, Deserialize)]
    pub struct JobTexts {
        pub command_open: String,
        pub command_exit: String,
        pub command_signal: String,
        pub command_duration: String,
        pub command_output: String,
        pub command_close: String,
        pub subagent_open: String,
        pub subagent_person: String,
        pub subagent_truncated: String,
        pub subagent_silent: String,
        pub subagent_close: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        pub subagent_omitted: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        pub stopped_by_user: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        pub subagent_message_open: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        pub subagent_message_close: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        pub command_tail: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        pub command_tail_cut: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        pub user_message_open: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        pub user_message_close: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        pub lines_open: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        pub lines_close: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        pub shell_open: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        pub shell_command: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        pub shell_close: String,
    }

    #[derive(Serialize, Deserialize)]
    pub struct DriverPlaceholders {
        pub image_omitted: String,
        pub file_omitted: String,
        pub no_output: String,
        pub tool_attachments: String,
        pub tool_attachments_only: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub text_file: Option<TextFileTexts>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub image_name: Option<ImageNameTexts>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub image_description: Option<ImageDescriptionTexts>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub attached_path: Option<AttachedPathTexts>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub open_path: Option<OpenPathTexts>,
    }

    #[derive(Serialize, Deserialize)]
    pub struct TextFileTexts {
        pub file_open: String,
        pub file_cut: String,
        pub file_close: String,
    }

    #[derive(Serialize, Deserialize)]
    pub struct ImageNameTexts {
        pub image_open: String,
        pub image_close: String,
        pub image_omitted_named: String,
    }

    #[derive(Serialize, Deserialize)]
    pub struct ImageDescriptionTexts {
        pub image_description_open: String,
        pub image_description_open_named: String,
        pub image_description_close: String,
    }

    #[derive(Serialize, Deserialize)]
    pub struct VisionTexts {
        pub instruction: String,
        pub question: String,
    }

    #[derive(Serialize, Deserialize)]
    pub struct AttachedPathTexts {
        pub image_omitted_path: String,
        pub file_omitted_path: String,
    }

    #[derive(Serialize, Deserialize)]
    pub struct OpenPathTexts {
        pub image_open_path: String,
        pub file_open_path: String,
    }

    #[derive(Serialize, Deserialize)]
    pub struct FactTexts {
        pub env: String,
        pub reply_cut: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub session: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub output_limit: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pub model: Option<String>,
    }

    #[derive(Serialize, Deserialize)]
    pub struct RebuildTexts {
        pub notes_files: String,
        pub notes_files_more: String,
        pub notes_retrieve: String,
        pub notes_too_large: String,
        pub restored_open: String,
        pub restored_close: String,
    }

    #[derive(Serialize, Deserialize)]
    pub struct RecapTexts {
        pub user: String,
        pub assistant: String,
        pub omitted: String,
        pub excerpted: String,
    }

    #[derive(Serialize, Deserialize)]
    pub struct ScheduleTexts {
        pub schedule_open: String,
        pub schedule_close: String,
        pub loop_open: String,
        pub loop_close: String,
    }

    #[derive(Serialize, Deserialize)]
    pub struct ShortenTexts {
        pub truncated: String,
        pub notes_uncovered: String,
    }

    #[derive(Serialize, Deserialize)]
    pub struct TitleTexts {
        pub instruction: String,
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

/// `base` 照原样，再逐格改：删掉、写成 `null`、写成数、写成空字串；另加一个不认识的格子；整个换成数组、`null`。嵌着的对象
/// 里面照样逐格改（一层层往下），连同外面整份比。
fn changes(base: &Value) -> Vec<Value> {
    let Value::Object(map) = base else {
        panic!("是对象：{base}");
    };
    let mut all = vec![base.clone()];
    for (key, value) in map {
        for change in [
            None,
            Some(Value::Null),
            Some(Value::from(1)),
            Some(Value::from("")),
        ] {
            let mut changed = map.clone();
            match change {
                None => {
                    changed.remove(key);
                }
                Some(value) => {
                    changed.insert(key.clone(), value);
                }
            }
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

/// 出厂的那一份写成的 JSON。
fn shipped<T: Serialize>(texts: &T) -> Value {
    serde_json::to_value(texts).expect("写得出")
}

/// 出厂的那几份。
fn core() -> crate::policy::CoreTexts {
    crate::policy::shipped::core()
}

/// 整份出厂的字（连同嵌着的每一组）。
#[test]
fn core_texts_read_the_same() {
    every_change::<crate::policy::CoreTexts, old::CoreTexts>(&shipped(&core()));
}

/// 嵌着的每一组单独读（别处也单读它们）。
#[test]
fn each_group_reads_the_same() {
    let whole = shipped(&core());
    let group = |key: &str| {
        whole
            .get(key)
            .cloned()
            .unwrap_or_else(|| panic!("出厂的有 {key}"))
    };
    let drivers = group("drivers");
    let compaction = group("compaction");
    every_change::<crate::policy::ToolResultTexts, old::ToolResultTexts>(&group("tool_results"));
    every_change::<crate::policy::JobTexts, old::JobTexts>(&group("jobs"));
    every_change::<crate::policy::DriverPlaceholders, old::DriverPlaceholders>(&drivers);
    every_change::<crate::policy::TextFileTexts, old::TextFileTexts>(&drivers["text_file"]);
    every_change::<crate::policy::ImageNameTexts, old::ImageNameTexts>(&drivers["image_name"]);
    every_change::<crate::policy::ImageDescriptionTexts, old::ImageDescriptionTexts>(
        &drivers["image_description"],
    );
    every_change::<crate::policy::AttachedPathTexts, old::AttachedPathTexts>(
        &drivers["attached_path"],
    );
    every_change::<crate::policy::OpenPathTexts, old::OpenPathTexts>(&drivers["open_path"]);
    every_change::<crate::policy::CompactionTexts, old::CompactionTexts>(&compaction);
    every_change::<crate::policy::RebuildTexts, old::RebuildTexts>(&compaction["rebuild"]);
    every_change::<crate::policy::ShortenTexts, old::ShortenTexts>(&compaction["shorten"]);
    every_change::<crate::policy::PermissionTexts, old::PermissionTexts>(&group("permissions"));
    every_change::<crate::policy::TurnEndedTexts, old::TurnEndedTexts>(&group("turn_ended"));
    every_change::<crate::policy::FactTexts, old::FactTexts>(&group("facts"));
    every_change::<crate::policy::RecapTexts, old::RecapTexts>(&group("recap"));
    every_change::<crate::policy::ScheduleTexts, old::ScheduleTexts>(&group("schedule"));
    every_change::<crate::policy::TitleTexts, old::TitleTexts>(&group("title"));
    every_change::<crate::policy::VisionTexts, old::VisionTexts>(&group("vision"));
}

/// 整份快照：新旧两种读法拼起来的读回来，写回去和原来的字节一样（快照的哈希照它算）。
#[test]
fn a_whole_snapshot_reads_back_byte_for_byte() {
    let snapshot = crate::policy::test_support::engineer();
    let bytes = snapshot.to_bytes();
    let back = crate::policy::Snapshot::from_bytes(&bytes).expect("读得回来");
    assert_eq!(back.to_bytes(), bytes);
    assert_eq!(back, snapshot);
}

/// 报错的话：缺了必填的、类型不对的，说是哪一格。
#[test]
fn errors_say_which_field() {
    let mut base = shipped(&core().tool_results);
    base.as_object_mut().expect("是对象").remove("unknown");
    let error = serde_json::from_value::<crate::policy::ToolResultTexts>(base.clone())
        .expect_err("缺了必填的");
    assert!(
        error.to_string().contains("missing field `unknown`"),
        "{error}"
    );
    base["unknown"] = Value::from(1);
    let error =
        serde_json::from_value::<crate::policy::ToolResultTexts>(base).expect_err("类型不对");
    assert!(
        error
            .to_string()
            .contains("invalid type: number at `unknown`, expected a string"),
        "{error}"
    );
}
