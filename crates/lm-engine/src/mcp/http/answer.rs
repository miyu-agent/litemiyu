//! 读服务的回应（施工 X-32 上、PT-1 上、PT-1 下）：整段读完、SSE 拆成一条条（边读边拆：[`Stream`]）、回错的回应体是不是一条
//! JSON-RPC 错误。

use serde_json::{Value, json};

use crate::mcp::wire::LINE_LIMIT;

/// 回错的回应体 `body`（`sse` 的照事件拆，拆不出的照整段读：有的服务出错时类型还写着 SSE、正文却是 JSON）是一条 JSON-RPC 错误的：编号换成这次请求的 `id`，交回要写回去的那一行（通知没有编号，
/// 什么都不写）和记日志的一句（码和原话）。不是的没有。
pub(super) fn rpc_error(
    body: &str,
    sse: bool,
    id: Option<&Value>,
) -> Option<(Option<String>, String)> {
    let text = sse
        .then(|| events(body).into_iter().next())
        .flatten()
        .unwrap_or_else(|| body.trim().to_string());
    let mut message: Value = lm_kernel::kernel::json::from_str(&text).ok()?;
    let code = message["error"]["code"].as_i64()?;
    let said = format!(
        "{code} {}",
        message["error"]["message"]
            .as_str()
            .unwrap_or_default()
            .chars()
            .take(200)
            .collect::<String>()
    );
    let line = id.map(|id| {
        message["jsonrpc"] = json!("2.0");
        message["id"] = id.clone();
        if let Some(object) = message.as_object_mut() {
            object.remove("result");
        }
        message.to_string()
    });
    Some((line, said))
}

/// 读完一段回应：读断了的、超过 [`LINE_LIMIT`] 的交回原因（再长的一条客户端也收不下，不往内存里一直读）。
pub(super) async fn body(mut response: crate::http::Response) -> Result<String, String> {
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|error| format!("the answer broke off: {}", error.without_url()))?
    {
        if bytes.len() + chunk.len() > LINE_LIMIT {
            return Err(format!("the answer is longer than {LINE_LIMIT} bytes"));
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

/// SSE 的正文拆成一条条消息（照 SSE 的规矩，WHATWG HTML「Server-sent events」解析一节）：一行以 `\r\n`、`\n` 或单独的 `\r`
/// 结尾；空行分开一个事件；一个事件里的几行 `data` 照先后用 `\n` 接起来（`data:` 后面的一个空格不算，只写 `data` 没有冒号的是
/// 空的一行）；`:` 起头的是注释，别的字段（`event`、`id`、`retry`）这里用不上。一个 `data` 都没有的事件不算。
pub(super) fn events(body: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut data: Option<String> = None;
    for line in sse_lines(body).chain(std::iter::once("")) {
        if line.is_empty() {
            out.extend(data.take());
            continue;
        }
        let (field, value) = match line.split_once(':') {
            Some((field, value)) => (field, value.strip_prefix(' ').unwrap_or(value)),
            None => (line, ""),
        };
        if field == "data" {
            match &mut data {
                Some(data) => {
                    data.push('\n');
                    data.push_str(value);
                }
                None => data = Some(value.to_string()),
            }
        }
    }
    out
}

/// SSE 正文的一行行：`\r\n`、`\n`、单独的 `\r` 都是一行的结尾；最后一行没有结尾的也算。
fn sse_lines(body: &str) -> impl Iterator<Item = &str> {
    let mut rest = body;
    std::iter::from_fn(move || {
        if rest.is_empty() {
            return None;
        }
        let Some(end) = rest.find(['\r', '\n']) else {
            return Some(std::mem::take(&mut rest));
        };
        let line = &rest[..end];
        let skip = if rest[end..].starts_with("\r\n") {
            2
        } else {
            1
        };
        rest = &rest[end + skip..];
        Some(line)
    })
}

/// SSE 回应边读边拆（施工 PT-1 下）：攒着收到的字节，到空行就是凑齐了一个事件，马上交出去，进度、订阅推来的通知不用等整段读完。
/// 一个没凑齐的事件超过 [`LINE_LIMIT`] 的当对方回错（同整段读的上限）。
#[derive(Default)]
pub(super) struct Stream {
    pending: Vec<u8>,
    /// 攒着的前面这些已经找过空行了：下一块从这里往回两个字节接着找，长事件分很多块到的不用每次从头找。
    scanned: usize,
}

impl Stream {
    /// 收一块 `chunk`，交回这一块凑齐的事件（照先后）。
    ///
    /// 出错：攒着没凑齐的超过上限，交回原因（英文）。
    pub(super) fn push(&mut self, chunk: &[u8]) -> Result<Vec<String>, String> {
        self.pending.extend_from_slice(chunk);
        let mut end = None;
        let from = self.scanned.saturating_sub(2);
        for at in from..self.pending.len() {
            // 空行：`\n\n`、`\n\r\n`（`\r\n` 结尾的两行），只用 `\r` 结尾的 `\r\r`。
            match (
                self.pending[at],
                self.pending.get(at + 1),
                self.pending.get(at + 2),
            ) {
                (b'\n', Some(b'\n'), _) | (b'\r', Some(b'\r'), _) => end = Some(at + 2),
                (b'\n', Some(b'\r'), Some(b'\n')) => end = Some(at + 3),
                _ => {}
            }
        }
        let done: Vec<u8> = match end {
            Some(end) => self.pending.drain(..end).collect(),
            None => Vec::new(),
        };
        self.scanned = self.pending.len();
        if self.pending.len() > LINE_LIMIT {
            return Err(format!("the answer is longer than {LINE_LIMIT} bytes"));
        }
        Ok(events(&String::from_utf8_lossy(&done)))
    }

    /// 读完了：最后一个没以空行收尾的事件也算。
    pub(super) fn finish(self) -> Vec<String> {
        events(&String::from_utf8_lossy(&self.pending))
    }
}
