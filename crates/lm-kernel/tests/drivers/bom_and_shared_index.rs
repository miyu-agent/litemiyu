//! LM-36（同 Miyu AU-020 4ce29c6fe，审计 #020）：三家驱动各喂两种流。流开头带一个 BOM（U+FEFF）时，第一条事件照样收到；
//! 并行的两次工具调用都写同一个块的编号（线上的 `index`、`output_index` 都是 0）时，照各自的 `id` 分成两次，参数不拼在一起。

use std::collections::BTreeMap;

use lm_kernel::drivers::{anthropic, openai_chat, openai_responses};
use lm_kernel::kernel::accumulate::{Delta, Kind};

/// BOM 的三个字节。
const BOM: &[u8] = b"\xEF\xBB\xBF";

/// 增量里的块：编号到种类（工具调用写工具名）和接起来的字，照编号排。
fn blocks(deltas: &[Delta]) -> Vec<(String, String)> {
    let mut found: BTreeMap<usize, (String, String)> = BTreeMap::new();
    for delta in deltas {
        match delta {
            Delta::Start { index, kind } => {
                let kind = match kind {
                    Kind::Text => "text".to_string(),
                    Kind::Reasoning => "reasoning".to_string(),
                    Kind::ToolCall { name } => format!("call {name}"),
                };
                found.insert(*index, (kind, String::new()));
            }
            Delta::Text { index, text } => {
                if let Some((_, said)) = found.get_mut(index) {
                    said.push_str(text);
                }
            }
            _ => {}
        }
    }
    found.into_values().collect()
}

/// 每一块都收了（`End`）：收了几块。
fn ended(deltas: &[Delta]) -> usize {
    deltas
        .iter()
        .filter(|delta| matches!(delta, Delta::End { .. }))
        .count()
}

/// OpenAI 兼容对话接口：整段喂进去，连同收尾交出来的。
fn chat(stream: &[u8]) -> Vec<Delta> {
    let mut decoder = openai_chat::Decoder::new();
    let mut deltas = decoder.feed(stream);
    deltas.extend(decoder.finish().deltas);
    deltas
}

/// Anthropic：同上。
fn claude(stream: &[u8]) -> Vec<Delta> {
    let mut decoder = anthropic::Decoder::new();
    let mut deltas = decoder.feed(stream);
    deltas.extend(decoder.finish().deltas);
    deltas
}

/// OpenAI Responses：同上。
fn responses(stream: &[u8]) -> Vec<Delta> {
    let mut decoder = openai_responses::Decoder::new();
    let mut deltas = decoder.feed(stream);
    deltas.extend(decoder.finish().deltas);
    deltas
}

/// 前面接上 BOM。
fn with_bom(stream: &str) -> Vec<u8> {
    [BOM, stream.as_bytes()].concat()
}

const CHAT_TEXT: &str = concat!(
    "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"你好\"}}]}\n\n",
    "data: {\"choices\":[{\"index\":0,\"delta\":{\"content\":\"！\"},\"finish_reason\":\"stop\"}]}\n\n",
    "data: [DONE]\n\n",
);

const CLAUDE_TEXT: &str = concat!(
    "event: content_block_start\n",
    "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
    "event: content_block_delta\n",
    "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"你好！\"}}\n\n",
    "event: message_delta\n",
    "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":3}}\n\n",
    "event: message_stop\n",
    "data: {\"type\":\"message_stop\"}\n\n",
);

const RESPONSES_TEXT: &str = concat!(
    "event: response.output_item.added\n",
    "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"id\":\"msg_1\",\"type\":\"message\",\"content\":[]}}\n\n",
    "event: response.output_text.delta\n",
    "data: {\"type\":\"response.output_text.delta\",\"output_index\":0,\"delta\":\"你好！\"}\n\n",
    "event: response.completed\n",
    "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"usage\":{\"input_tokens\":3,\"output_tokens\":3}}}\n\n",
);

#[test]
fn a_leading_bom_loses_nothing_in_any_driver() {
    let said = vec![("text".to_string(), "你好！".to_string())];
    assert_eq!(blocks(&chat(CHAT_TEXT.as_bytes())), said, "对照");
    assert_eq!(blocks(&chat(&with_bom(CHAT_TEXT))), said, "OpenAI 对话");
    assert_eq!(blocks(&claude(CLAUDE_TEXT.as_bytes())), said, "对照");
    assert_eq!(blocks(&claude(&with_bom(CLAUDE_TEXT))), said, "Anthropic");
    assert_eq!(blocks(&responses(RESPONSES_TEXT.as_bytes())), said, "对照");
    assert_eq!(
        blocks(&responses(&with_bom(RESPONSES_TEXT))),
        said,
        "OpenAI Responses"
    );
}

/// 两次调用：`read` 读 a.txt、`shell` 跑 ls。
fn two_calls() -> Vec<(String, String)> {
    vec![
        ("call read".to_string(), r#"{"path":"a.txt"}"#.to_string()),
        ("call shell".to_string(), r#"{"command":"ls"}"#.to_string()),
    ]
}

/// OpenAI 兼容对话接口：有的兼容层并行的几次调用都写 `index: 0`，靠 `id` 分。
#[test]
fn parallel_calls_on_index_zero_stay_apart_in_openai_chat() {
    let stream = concat!(
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_a\",\"type\":\"function\",\"function\":{\"name\":\"read\",\"arguments\":\"{\\\"path\\\":\"}}]}}]}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"\\\"a.txt\\\"}\"}}]}}]}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"id\":\"call_b\",\"type\":\"function\",\"function\":{\"name\":\"shell\",\"arguments\":\"{\\\"command\\\":\"}}]}}]}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"\\\"ls\\\"}\"}}]}}]}\n\n",
        "data: {\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"tool_calls\"}]}\n\n",
        "data: [DONE]\n\n",
    );
    let deltas = chat(stream.as_bytes());
    assert_eq!(blocks(&deltas), two_calls());
    assert_eq!(ended(&deltas), 2, "两块都收了");
}

/// Anthropic：照协议每块的 `index` 不一样；照样喂一份两块都写 0 的（一块收了再开下一块），靠 `id` 分。
#[test]
fn parallel_calls_on_index_zero_stay_apart_in_anthropic() {
    let stream = concat!(
        "event: content_block_start\n",
        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_a\",\"name\":\"read\",\"input\":{}}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"path\\\":\\\"a.txt\\\"}\"}}\n\n",
        "event: content_block_stop\n",
        "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
        "event: content_block_start\n",
        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_b\",\"name\":\"shell\",\"input\":{}}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"command\\\":\\\"ls\\\"}\"}}\n\n",
        "event: content_block_stop\n",
        "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
        "event: message_delta\n",
        "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"},\"usage\":{\"output_tokens\":9}}\n\n",
        "event: message_stop\n",
        "data: {\"type\":\"message_stop\"}\n\n",
    );
    let deltas = claude(stream.as_bytes());
    assert_eq!(blocks(&deltas), two_calls());
    assert_eq!(ended(&deltas), 2, "两块都收了");
}

/// OpenAI Responses：照协议每项的 `output_index` 不一样；照样喂一份两项都写 0 的（一项完了再加下一项），靠项的 `id` 分。
#[test]
fn parallel_calls_on_index_zero_stay_apart_in_openai_responses() {
    let stream = concat!(
        "event: response.output_item.added\n",
        "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"id\":\"fc_a\",\"type\":\"function_call\",\"arguments\":\"\",\"call_id\":\"call_a\",\"name\":\"read\"}}\n\n",
        "event: response.function_call_arguments.delta\n",
        "data: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"fc_a\",\"output_index\":0,\"delta\":\"{\\\"path\\\":\\\"a.txt\\\"}\"}\n\n",
        "event: response.output_item.done\n",
        "data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"id\":\"fc_a\",\"type\":\"function_call\",\"arguments\":\"{\\\"path\\\":\\\"a.txt\\\"}\",\"call_id\":\"call_a\",\"name\":\"read\"}}\n\n",
        "event: response.output_item.added\n",
        "data: {\"type\":\"response.output_item.added\",\"output_index\":0,\"item\":{\"id\":\"fc_b\",\"type\":\"function_call\",\"arguments\":\"\",\"call_id\":\"call_b\",\"name\":\"shell\"}}\n\n",
        "event: response.function_call_arguments.delta\n",
        "data: {\"type\":\"response.function_call_arguments.delta\",\"item_id\":\"fc_b\",\"output_index\":0,\"delta\":\"{\\\"command\\\":\\\"ls\\\"}\"}\n\n",
        "event: response.output_item.done\n",
        "data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"id\":\"fc_b\",\"type\":\"function_call\",\"arguments\":\"{\\\"command\\\":\\\"ls\\\"}\",\"call_id\":\"call_b\",\"name\":\"shell\"}}\n\n",
        "event: response.completed\n",
        "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"usage\":{\"input_tokens\":3,\"output_tokens\":9}}}\n\n",
    );
    let deltas = responses(stream.as_bytes());
    assert_eq!(blocks(&deltas), two_calls());
    assert_eq!(ended(&deltas), 2, "两块都收了");
}
