//! 量 CPU 的一把尺：试会影响速度的发行配置（比如 `opt-level = "z"`）时，看一个长会话组装成
//! 请求、编成三家的字节慢没慢。平时不跑；要量时照发行配置编：
//! `cargo test --release -p lm-kernel --test assemble speed -- --ignored --nocapture`。

use std::time::{Duration, Instant};

use crate::support::{anthropic_wire, policy, responses_wire, stage, wire};
use lm_kernel::kernel::history::History;
use lm_kernel::kernel::testkit::{Line, Play};

/// 读回来的一页：约 7 kB 的代码。
fn page() -> String {
    "fn main() {\n    let total: u64 = (1..=10).map(|k| k * k).sum();\n    println!(\"{total}\");\n}\n".repeat(80)
}

#[test]
#[ignore = "量速度用的，不判对错"]
fn a_long_session_assembles_and_encodes() {
    let mut s = stage();
    let page = page();
    for k in 0..150 {
        s.model([
            Line::calls("Let me read it.", &[("read", r#"{"path":"a"}"#)]),
            Line::says(&format!(
                "Round {k}: the loop sums the squares and prints the total once."
            )),
        ]);
        s.tools([Play::done(&page)]);
        s.say("Please read the file again and tell me what the main function prints.");
    }
    let mut history = History::default();
    for event in s.log() {
        history.append(event.clone());
    }
    let assembler = policy().assembler;
    let mut times: Vec<Duration> = Vec::new();
    let mut bytes = 0;
    for _ in 0..21 {
        let began = Instant::now();
        let request = assembler.assemble(&history);
        bytes = wire(&request).body.len()
            + anthropic_wire(&request).body.len()
            + responses_wire(&request).body.len();
        times.push(began.elapsed());
    }
    times.sort();
    println!(
        "组装加编码：{} 条事件，三家合计 {} kB，21 次的中位数 {:.2} ms",
        s.log().len(),
        bytes / 1024,
        times[10].as_secs_f64() * 1000.0
    );
}
