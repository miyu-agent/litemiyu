//! JSON-RPC 2.0 的那一层：标准输入上一行一条读进来，标准输出上一行一条写出去。规范的错误码在这里。

use std::io::{BufRead, Write};
use std::sync::mpsc;

use serde_json::{Value, json};

use super::Event;

/// 不是合法的 JSON。
pub const PARSE: i64 = -32700;
/// 不是一条请求；握手以前来了别的请求。
pub const INVALID_REQUEST: i64 = -32600;
/// 没有这个方法。
pub const NO_METHOD: i64 = -32601;
/// 参数不对，或者写了不支持的值。
pub const INVALID_PARAMS: i64 = -32602;
/// 办的时候出了错。
pub const INTERNAL: i64 = -32603;
/// 说的那个会话不在。
pub const NOT_FOUND: i64 = -32002;

/// 另起一条线程读标准输入：一行交一次，读到头（编辑器关了管道）交 [`Event::Closed`]。起不来的当成读到头。
pub fn read_stdin(tx: mpsc::Sender<Event>) {
    let closed = tx.clone();
    let spawned = std::thread::Builder::new()
        .name("acp-stdin".into())
        .spawn(move || {
            for line in std::io::stdin().lock().lines() {
                let Ok(line) = line else { break };
                if tx.send(Event::Line(line)).is_err() {
                    return;
                }
            }
            drop(tx.send(Event::Closed));
        });
    if spawned.is_err() {
        drop(closed.send(Event::Closed));
    }
}

/// 写一行到标准输出、冲掉（那头关了的不崩）。
fn line(value: &Value) {
    let mut out = std::io::stdout().lock();
    drop(writeln!(out, "{value}").and_then(|()| out.flush()));
}

/// 回一个请求：成了。
pub fn result(id: &Value, result: Value) {
    line(&json!({"jsonrpc": "2.0", "id": id, "result": result}));
}

/// 回一个请求：没成。`data` 有的带上。
pub fn error(id: &Value, code: i64, message: &str, data: Option<Value>) {
    let mut error = json!({"code": code, "message": message});
    if let (Some(data), Some(map)) = (data, error.as_object_mut()) {
        map.insert("data".into(), data);
    }
    line(&json!({"jsonrpc": "2.0", "id": id, "error": error}));
}

/// 发一条通知。
pub fn notify(method: &str, params: Value) {
    line(&json!({"jsonrpc": "2.0", "method": method, "params": params}));
}

/// 向编辑器发一个请求。
pub fn request(id: u64, method: &str, params: Value) {
    line(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}));
}

/// 一个会话的一条更新（`session/update`）。
pub fn update(session: &str, update: Value) {
    notify(
        "session/update",
        json!({"sessionId": session, "update": update}),
    );
}
