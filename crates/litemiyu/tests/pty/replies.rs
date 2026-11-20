//! 假服务器照 OpenAI Chat 的流式写法回的几种：调一件工具、说一句话。

/// 一次调 `name`，参数是 `arguments`（JSON 的原文）。
pub fn calls(name: &str, arguments: &str) -> String {
    let delta = serde_json::json!({
        "role": "assistant",
        "tool_calls": [{
            "index": 0,
            "id": "call_1",
            "type": "function",
            "function": { "name": name, "arguments": arguments }
        }]
    });
    let first = serde_json::json!({
        "choices": [{ "index": 0, "delta": delta, "finish_reason": null }]
    })
    .to_string();
    sse(&[
        &first,
        r#"{"choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}"#,
    ])
}

/// 先说一句 `text`，再调 `name`（参数是 JSON 的原文）。
pub fn says_then_calls(text: &str, name: &str, arguments: &str) -> String {
    let said = serde_json::json!({
        "choices": [{ "index": 0, "delta": { "role": "assistant", "content": text }, "finish_reason": null }]
    })
    .to_string();
    let call = calls(name, arguments);
    format!("data: {said}\n\n{call}")
}

/// 说一句话，分几段到。
pub fn says(pieces: &[&str]) -> String {
    let mut events: Vec<String> = pieces
        .iter()
        .map(|piece| {
            format!(
                r#"{{"choices":[{{"index":0,"delta":{{"content":{}}},"finish_reason":null}}]}}"#,
                serde_json::json!(piece)
            )
        })
        .collect();
    events.push(r#"{"choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":120,"completion_tokens":12,"total_tokens":132}}"#.into());
    let refs: Vec<&str> = events.iter().map(String::as_str).collect();
    sse(&refs)
}

/// 一串 SSE 事件，末尾 `[DONE]`。
fn sse(events: &[&str]) -> String {
    let mut text: String = events
        .iter()
        .map(|event| format!("data: {event}\n\n"))
        .collect();
    text.push_str("data: [DONE]\n\n");
    text
}
