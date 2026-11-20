//! 照清单画的一页：几行怎么排，值怎么写给人看、人敲的字怎么读回来。

use lm_engine::settings::schema::schema;
use lm_kernel::config::Value;

use super::{Row, editable, read_input, rows, shown_value};
use crate::settings::texts::Texts;

#[test]
fn pattern_items_are_not_rows_and_groups_head_their_items() {
    let schema = schema(&lm_engine::configured::items(), "zh").unwrap();
    let general = rows(&schema, "general");
    assert!(matches!(general[0], Row::Group(_)));
    let models: Vec<&str> = rows(&schema, "models")
        .into_iter()
        .filter_map(|row| match row {
            Row::Item(i) => Some(schema.items[i].item.key),
            _ => None,
        })
        .collect();
    assert_eq!(
        models,
        ["models.chat", "models.vision"],
        "常用的在前；供应商、池那几项键里有人起的名字，归「供应商和模型」的栏"
    );
}

#[test]
fn values_are_written_for_people() {
    let schema = schema(&lm_engine::configured::items(), "zh").unwrap();
    let texts = Texts::load("zh").unwrap();
    let item = |key: &str| schema.item(key).unwrap();
    let on = Value::Bool(true);
    assert_eq!(
        shown_value(item("compaction.prepare"), Some(&on), &texts),
        "开"
    );
    let en = Value::Text("en".into());
    assert_eq!(
        shown_value(item("ui.language"), Some(&en), &texts),
        "English"
    );
    assert_eq!(shown_value(item("models.chat"), None, &texts), "—");
}

#[test]
fn input_is_read_by_kind_and_empty_means_default() {
    let schema = schema(&lm_engine::configured::items(), "zh").unwrap();
    let chat = schema.item("models.chat").unwrap();
    assert_eq!(read_input(chat, "  "), Ok(None));
    assert_eq!(read_input(chat, "a/b"), Ok(Some(Value::Text("a/b".into()))));
    assert_eq!(read_input(chat, "not a reference"), Err(()));
    assert_eq!(editable(Some(&Value::Text("a/b".into()))), "a/b");
    assert_eq!(editable(Some(&Value::Bool(true))), "true");
}

/// 画出来的行里没有密钥项：值照原样写，密钥项要是画出来就露出引用了（LM-3 删了「只写设没设」那一支）。哪天加了一个画出来的
/// 密钥项，这里红，得把那一支加回来。
#[test]
fn no_drawn_row_is_a_secret() {
    let schema = schema(&lm_engine::configured::items(), "zh").unwrap();
    for page in &schema.pages {
        for row in rows(&schema, page.id) {
            if let Row::Item(i) = row {
                assert_ne!(
                    schema.items[i].item.kind,
                    lm_kernel::config::Kind::Secret,
                    "{} 画在 {} 页",
                    schema.items[i].item.key,
                    page.id
                );
            }
        }
    }
}
