//! 模型问人的几道题（`ask_user`）和规范的表单提问（`elicitation/create` 的 form 一种）之间来回换。
//!
//! 表单是一层平的对象：第 N 道题是属性 `qN`；有选项的题另有一个 `qN_other`，给人自己写（内核的规矩是人总能自己写，
//! 不用列成选项）。单选的是带 `enum` 的字符串，多选的是元素带 `enum` 的数组，没有选项的是一个字符串。

use lm_kernel::kernel::event::{Question, Response, fits};
use serde_json::{Map, Value, json};

use crate::cli::words::Words;

/// 第 `index` 道题（从 0 数）的属性名。
fn name(index: usize) -> String {
    format!("q{}", index + 1)
}

/// 有选项的题给人自己写的那一格的属性名。
fn other(index: usize) -> String {
    format!("q{}_other", index + 1)
}

/// 表单上面的那句话：只有一道题的就是那道题，几道的是一句总的。
pub fn message(words: &Words, questions: &[Question]) -> String {
    match questions {
        [only] => only.question.clone(),
        _ => words.get("acp.questions").to_string(),
    }
}

/// 这几道题的表单（`requestedSchema`）。
pub fn schema(words: &Words, questions: &[Question]) -> Value {
    let mut properties = Map::new();
    for (index, question) in questions.iter().enumerate() {
        let title = question
            .header
            .clone()
            .unwrap_or_else(|| question.question.clone());
        let labels: Vec<&str> = question
            .options
            .iter()
            .map(|choice| choice.label.as_str())
            .collect();
        let field = match (labels.is_empty(), question.multiple) {
            (true, _) => json!({"type": "string"}),
            (false, false) => json!({"type": "string", "enum": labels}),
            (false, true) => json!({"type": "array", "items": {"type": "string", "enum": labels}}),
        };
        properties.insert(name(index), described(field, &title, &question.question));
        if !labels.is_empty() {
            let field = json!({"type": "string"});
            let title = words.say("acp.other", &[("title", &title)]);
            properties.insert(other(index), described(field, &title, &question.question));
        }
    }
    json!({"type": "object", "properties": properties})
}

/// 给一格加上标题和说明。
fn described(mut field: Value, title: &str, description: &str) -> Value {
    if let Some(map) = field.as_object_mut() {
        map.insert("title".into(), json!(title));
        map.insert("description".into(), json!(description));
    }
    field
}

/// 一道没答的题。
fn blank() -> Response {
    Response {
        picked: Vec::new(),
        text: None,
        notes: None,
        attachments: Vec::new(),
    }
}

/// 每道题都没答（编辑器不支持表单、人没填就关了、回的读不懂）。
pub fn unanswered(questions: &[Question]) -> Vec<Response> {
    questions.iter().map(|_| blank()).collect()
}

/// 把编辑器交回的表单内容（`content`）读成每道题的回答。选的不是那道题的选项的不算；整组对不上题目的（内核的
/// `fits`：单选的选了几项这一类）照每道都没答交。
pub fn answers(questions: &[Question], content: &Value) -> Vec<Response> {
    let read: Vec<Response> = questions
        .iter()
        .enumerate()
        .map(|(index, question)| {
            let offered = |label: &str| question.options.iter().any(|choice| choice.label == label);
            let mut answer = blank();
            match &content[name(index)] {
                Value::String(written) if question.options.is_empty() => {
                    answer.text = Some(written.clone()).filter(|text| !text.trim().is_empty());
                }
                Value::String(label) if offered(label) => answer.picked.push(label.clone()),
                Value::Array(labels) => {
                    for label in labels.iter().filter_map(Value::as_str) {
                        if offered(label) && !answer.picked.iter().any(|had| had == label) {
                            answer.picked.push(label.to_string());
                        }
                    }
                }
                _ => {}
            }
            if let Some(written) = content[other(index)].as_str()
                && !written.trim().is_empty()
            {
                answer.text = Some(written.to_string());
            }
            answer
        })
        .collect();
    match fits(questions, &read) {
        true => read,
        false => unanswered(questions),
    }
}

#[cfg(test)]
mod tests {
    use lm_kernel::kernel::event::{Choice, Question};
    use serde_json::json;

    use super::{answers, message, schema, unanswered};
    use crate::cli::words::Words;

    fn question(text: &str, labels: &[&str], multiple: bool) -> Question {
        Question {
            header: None,
            question: text.to_string(),
            options: labels
                .iter()
                .map(|label| Choice {
                    label: label.to_string(),
                    description: None,
                    preview: None,
                })
                .collect(),
            multiple,
        }
    }

    #[test]
    fn each_kind_of_question_gets_its_field() {
        let words = Words::load("en");
        let asked = [
            question("Which one?", &["a", "b"], false),
            question("Which ones?", &["x", "y", "z"], true),
            question("Why?", &[], false),
        ];
        let form = schema(&words, &asked);
        let fields = &form["properties"];
        assert_eq!(fields["q1"]["type"], "string");
        assert_eq!(fields["q1"]["enum"], json!(["a", "b"]));
        assert_eq!(fields["q1_other"]["type"], "string");
        assert_eq!(fields["q2"]["type"], "array");
        assert_eq!(fields["q2"]["items"]["enum"], json!(["x", "y", "z"]));
        assert_eq!(fields["q2_other"]["type"], "string");
        assert_eq!(
            fields["q3"],
            json!({"type": "string", "title": "Why?", "description": "Why?"})
        );
        // 没有选项的题本来就是自己写，不另给一格。
        assert!(fields["q3_other"].is_null());
        // 一道题时表单上面就是那道题；几道时是一句总的。
        assert_eq!(message(&words, &asked[..1]), "Which one?");
        assert_eq!(message(&words, &asked), words.get("acp.questions"));
    }

    #[test]
    fn a_filled_form_is_read_question_by_question() {
        let asked = [
            question("Which one?", &["a", "b"], false),
            question("Which ones?", &["x", "y", "z"], true),
            question("Why?", &[], false),
        ];
        // 多选的：不是选项的丢掉，重复的只算一次；自己写的那一格空白的不算。
        let read = answers(
            &asked,
            &json!({"q1": "b", "q1_other": "  ", "q2": ["z", "nope", "x", "z", 7], "q2_other": "and w", "q3": "because"}),
        );
        assert_eq!(read[0].picked, ["b"]);
        assert_eq!(read[0].text, None);
        assert_eq!(read[1].picked, ["z", "x"]);
        assert_eq!(read[1].text.as_deref(), Some("and w"));
        assert!(read[2].picked.is_empty());
        assert_eq!(read[2].text.as_deref(), Some("because"));
        // 没填的那几道就是没答，别的照读。
        let read = answers(&asked, &json!({"q2": "y"}));
        assert!(read[0].picked.is_empty() && read[0].text.is_none());
        assert_eq!(read[1].picked, ["y"]);
        assert!(read[2].picked.is_empty() && read[2].text.is_none());
    }

    #[test]
    fn a_form_that_does_not_fit_counts_as_unanswered() {
        let asked = [
            question("Which one?", &["a", "b"], false),
            question("Why?", &[], false),
        ];
        // 单选的题交来两项：整组照没答交，连答对的那一道也不留（内核只收对得上的一整组）。
        let read = answers(&asked, &json!({"q1": ["a", "b"], "q2": "because"}));
        assert_eq!(read, unanswered(&asked));
        // 交来的不是对象。
        assert_eq!(answers(&asked, &json!("yes")), unanswered(&asked));
        // 单选的选了不是选项的：这一道没答，别的照读。
        let read = answers(&asked, &json!({"q1": "c", "q2": "because"}));
        assert!(read[0].picked.is_empty());
        assert_eq!(read[1].text.as_deref(), Some("because"));
    }
}
