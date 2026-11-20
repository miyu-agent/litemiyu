//! 量 CPU 的一把尺：试会影响速度的发行配置（比如 `opt-level = "z"`）时，看一段长 Markdown
//! 排版慢没慢。平时不跑；要量时照发行配置编：`cargo test --release -p litemiyu markdown::speed -- --ignored --nocapture`。

use std::time::{Duration, Instant};

use super::{Kit, render};
use crate::config::Config;

/// 一节：标题、段落里的强调和行内代码、列表、引用、带着色的代码块、表格。
const SECTION: &str = "## Section\n\nThe **build** failed because `cargo` could not find the *linker*; see [the guide](https://example.com/guide) \
for the details, and remember that 中文和 English 混排 also has to wrap at the right column without splitting a word.\n\n\
- first item with `inline code`\n- second item\n  1. nested one\n  2. nested two\n\n\
> A quoted line that is long enough to be folded onto the next line when the terminal is narrow.\n\n\
```rust\nfn main() {\n    let total: u64 = (1..=10).map(|k| k * k).sum();\n    println!(\"{total}\");\n}\n```\n\n\
| name | size | note |\n|:-----|-----:|------|\n| text | 7102 | code |\n| rodata | 1788 | data |\n\n";

#[test]
#[ignore = "量速度用的，不判对错"]
fn a_long_answer_renders() {
    let config = Config::builtin().expect("出厂的配置读得进来");
    let kit = Kit {
        languages: config.languages(),
        math: config.math(),
        labels: &config.text().markdown,
        tab: config.layout.tab,
    };
    let text = SECTION.repeat(100);
    let mut times: Vec<Duration> = Vec::new();
    let mut lines = 0;
    for _ in 0..21 {
        let began = Instant::now();
        lines = render(&text, 100, &kit, &[]).len();
        times.push(began.elapsed());
    }
    times.sort();
    println!(
        "Markdown 排版：{} kB，{lines} 行，21 次的中位数 {:.2} ms",
        text.len() / 1024,
        times[10].as_secs_f64() * 1000.0
    );
}
