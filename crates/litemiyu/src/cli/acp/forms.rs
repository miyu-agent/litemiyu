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
