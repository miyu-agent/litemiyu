//! `session_usage`（`docs/blueprint/tools/session_usage.md`，施工 8-15）：零参数、访问类别读；输出逐字节比：用量总有，上下文
//! 有窗口的写几成和压缩线（没有压缩线的不写那一句），没窗口的写大约多少，算不出的不写；参数写了什么都不认；没有端口的照什么
//! 都没用答；叫停。Miyu 的金额、没价格的几次、查不了的出错不搬（金额不算了，用量照内存里的累计，查不会失败）。

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::json;

use lm_engine::tool::{ContextUse, Done, Spent, Stop, UsagePort};
use lm_kernel::kernel::block::{Block, Text};
use lm_kernel::kernel::tool::Access;

use crate::support::{Site, human, said, tool};

/// 假的查用量端口：交回给的那一份，数着被问了几次。
struct Port {
    spent: Spent,
    context: Option<ContextUse>,
    asked: AtomicUsize,
    busy: Option<u64>,
}

impl UsagePort for Port {
    fn context(&self) -> Option<ContextUse> {
        self.context
    }

    fn busy_ms(&self) -> Option<u64> {
        self.busy
    }

    fn spent(&self) -> Spent {
        self.asked.fetch_add(1, Ordering::Relaxed);
        self.spent
    }
}

/// 用了的：12 次请求。
fn spent() -> Spent {
    Spent {
        requests: 12,
        input: 48_210,
        cached: 40_122,
        output: 3_120,
    }
}

fn port(spent: Spent, context: Option<ContextUse>) -> Arc<Port> {
    Arc::new(Port {
        spent,
        context,
        asked: AtomicUsize::new(0),
        busy: None,
    })
}

fn context(used: u64, window: Option<u64>, line: Option<u64>) -> Option<ContextUse> {
    Some(ContextUse { used, window, line })
}

/// 交回的那一段字。
fn text(done: &Done) -> &str {
    match done.blocks.as_slice() {
        [Block::Text(Text { text })] => text,
        other => panic!("只有一段字：{other:?}"),
    }
}

async fn usage(port: &Arc<Port>, args: serde_json::Value) -> Done {
    let port = Arc::clone(port) as Arc<dyn UsagePort>;
    Site::new()
        .done_with_usage("session_usage", args, Some(port), Stop::default())
        .await
}

#[test]
fn it_reads_and_takes_no_parameters() {
    let spec = tool("session_usage").spec().clone();
    assert_eq!(spec.access, Access::Read);
    assert_eq!(
        spec.parameters.get(),
        r#"{"type":"object","properties":{}}"#
    );
}

#[tokio::test]
async fn everything_it_knows_one_sentence_a_line() {
    let port = Arc::new(Port {
        busy: Some(3_723_400),
        ..Arc::into_inner(port(spent(), context(23_110, Some(128_000), Some(95_000))))
            .expect("只有一份")
    });
    let done = usage(&port, json!({"whatever": 1})).await;
    assert!(!done.error);
    assert_eq!(
        text(&done),
        "Usage so far: 12 requests, 48210 input tokens (40122 from cache), 3120 output tokens.\n\
         Cache hit rate: 83%.\n\
         Context: 23110 of 128000 tokens.\n\
         Compaction starts at 95000.\n\
         Working time so far: 1h 2m 3s, not counting waits for the user.\n"
    );
    assert_eq!(port.asked.load(Ordering::Relaxed), 1);
    assert_eq!(human(done), said("session_usage/shown"));
}

#[tokio::test]
async fn only_what_there_is() {
    let plain = spent();
    let cases = [
        (
            context(900, Some(32_000), None),
            "Context: 900 of 32000 tokens.\n",
        ),
        (
            context(900, None, None),
            "Context: about 900 tokens. This model reports no window.\n",
        ),
        (None, ""),
    ];
    for (context, tail) in cases {
        let done = usage(&port(plain, context), json!({})).await;
        assert_eq!(
            text(&done),
            format!(
                "Usage so far: 12 requests, 48210 input tokens (40122 from cache), 3120 output tokens.\nCache hit rate: 83%.\n{tail}"
            )
        );
    }
}

#[tokio::test]
async fn without_a_port_nothing_was_spent() {
    let done = Site::new()
        .done_with_usage("session_usage", json!({}), None, Stop::default())
        .await;
    assert!(!done.error);
    assert_eq!(
        text(&done),
        "Usage so far: 0 requests, 0 input tokens (0 from cache), 0 output tokens.\n"
    );
}

#[tokio::test]
async fn stopping() {
    let stop = Stop::default();
    stop.raise();
    let port = port(spent(), None) as Arc<dyn UsagePort>;
    let done = Site::new()
        .done_with_usage("session_usage", json!({}), Some(port), stop)
        .await;
    assert!(done.stopped, "叫停了的停下");
}
