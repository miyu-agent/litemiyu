//! 框下面那一行用量那一格：照 Miyu 换成环以前的写法（2026-11-14 项目主人），卡住、正在压缩的排在后面。

use super::{Facts, Hit, Piece, pieces};
use crate::config::Config;
use crate::core::{CallError, Compaction, Limits, Push, Update, Usage};
use crate::transcript::Transcript;

fn text(pieces: &[Piece]) -> Vec<String> {
    pieces
        .iter()
        .map(|p| p.spans.iter().map(|s| s.content.as_ref()).collect())
        .collect()
}

fn lay(t: &Transcript, total: Usage, config: &Config) -> Vec<String> {
    lay_spent(t, total, None, config)
}

fn lay_spent(t: &Transcript, total: Usage, spent: Option<&str>, config: &Config) -> Vec<String> {
    let facts = Facts {
        t,
        total,
        spent: spent.map(str::to_string),
        texts: &config.front.usage_cell,
        spinner: &config.timeline.spinner,
    };
    text(&pieces(&facts, 0, None))
}

fn push(t: &mut Transcript, push: Push, config: &Config) {
    t.update(Update::Push(push), config.text());
}

fn used(tokens: u64) -> Usage {
    Usage {
        uncached: tokens,
        ..Usage::default()
    }
}

#[test]
fn context_and_total_are_written_like_the_old_footer() {
    let config = Config::builtin().unwrap();
    let mut t = Transcript::default();
    assert!(lay(&t, Usage::default(), &config).is_empty(), "全是 0 不写");
    t.context = 5_600;
    t.limits = Limits {
        window: Some(1_000_000),
        compaction_line: Some(800_000),
    };
    assert_eq!(
        lay(&t, Usage::default(), &config),
        ["5.6k/1M(0.6%)"],
        "累计还是 0 不写"
    );
    // 命中率照主请求的缓存读 ÷ 输入（没命中 + 读 + 写）；辅助请求只算进总数。
    let total = Usage {
        uncached: 1_000_000,
        cache_read: 118_000_000,
        cache_write: 500_000,
        output: 400_000,
        aux: 100_000,
    };
    assert_eq!(
        lay(&t, total, &config),
        ["5.6k/1M(0.6%)", "Σ120M(C99%)"],
        "主人给的例子：5.6k/1M(1%) · Σ120M(C99%)（占比照 Miyu 旧版保留一位小数）"
    );
    // 不知道窗口的只写用了多少；上下文还是 0 的只写累计。
    t.limits.window = None;
    assert_eq!(lay(&t, used(1), &config), ["5.6k", "Σ1(C0%)"]);
    t.context = 0;
    assert_eq!(lay(&t, used(1), &config), ["Σ1(C0%)"]);
}

#[test]
fn the_money_spent_follows_the_total_and_goes_first_when_it_does_not_fit() {
    let config = Config::builtin().unwrap();
    let mut t = Transcript::default();
    t.context = 5_600;
    t.limits.window = Some(1_000_000);
    let total = Usage {
        cache_read: 99,
        uncached: 1,
        ..Usage::default()
    };
    let facts = Facts {
        t: &t,
        total,
        spent: Some("$0.42".to_string()),
        texts: &config.front.usage_cell,
        spinner: &config.timeline.spinner,
    };
    let laid = pieces(&facts, 0, None);
    assert_eq!(text(&laid), ["5.6k/1M(0.6%)", "Σ100(C99%)", "$0.42"]);
    // 先丢花费、再丢累计（`footer.rs` 照 `keep` 从小到大丢）；点哪一截都开 `/status`。
    let keeps: Vec<u8> = laid.iter().map(|p| p.keep).collect();
    assert!(keeps[2] < keeps[1] && keeps[1] < keeps[0], "{keeps:?}");
    assert!(laid.iter().all(|p| p.hit == Some(Hit::Usage) && !p.tight));
    // 没有数（模型没价格）：不写那一段。
    assert_eq!(
        lay_spent(&t, total, None, &config),
        ["5.6k/1M(0.6%)", "Σ100(C99%)"]
    );
}

#[test]
fn a_failed_compaction_past_the_line_turns_red_with_a_compact_button_until_it_compacts() {
    let config = Config::builtin().unwrap();
    let mut t = Transcript::default();
    t.limits = Limits {
        window: Some(256_000),
        compaction_line: Some(190_000),
    };
    t.context = 203_000;
    assert_eq!(
        lay(&t, used(1), &config),
        ["203k/256k(79.3%)", "Σ1(C0%)"],
        "失败以前只是到了线，不算卡住"
    );
    let failed = CallError {
        class: "timeout".into(),
        message: "timed out".into(),
        status: None,
        retry_after_ms: None,
    };
    push(
        &mut t,
        Push::Compaction(Compaction::Failed(failed)),
        &config,
    );
    let facts = Facts {
        t: &t,
        total: used(1),
        spent: None,
        texts: &config.front.usage_cell,
        spinner: &config.timeline.spinner,
    };
    let laid = pieces(&facts, 0, None);
    assert_eq!(
        text(&laid),
        ["203k/256k(79.3%)", "Σ1(C0%)", "上下文压缩失败", "压缩"]
    );
    assert_eq!(laid[3].hit, Some(Hit::Compact));
    assert!(
        !laid[2].tight && laid[3].tight,
        "红字前面 · 、「压缩」前面一个空格"
    );
    assert_eq!(
        laid[0].spans[0].style.fg,
        crate::theme::error().fg,
        "上下文那一段红字"
    );
    assert_eq!(laid[2].spans[0].style.fg, crate::theme::error().fg, "红字");
    assert!(laid[2].keep > laid[1].keep, "红字和「压缩」比累计晚丢");
    // 压好了：回到平常。
    let done = Compaction::Done {
        before: 203_000,
        after: 31_000,
        prepared: false,
    };
    push(&mut t, Push::Compaction(done), &config);
    t.context = 31_000;
    assert_eq!(lay(&t, used(1), &config), ["31k/256k(12.1%)", "Σ1(C0%)"]);
}

#[test]
fn a_paused_compaction_below_the_line_is_not_stuck_and_over_the_window_is() {
    let config = Config::builtin().unwrap();
    let mut t = Transcript::default();
    t.limits = Limits {
        window: Some(128_000),
        compaction_line: Some(100_000),
    };
    t.context = 50_000;
    let paused = Compaction::Paused {
        reason: "failures".into(),
        failures: Some(3),
        entry: None,
    };
    push(&mut t, Push::Compaction(paused), &config);
    assert_eq!(
        lay(&t, used(1), &config),
        ["50k/128k(39.1%)", "Σ1(C0%)"],
        "没到线不算卡住"
    );
    // 换到了窗口小的模型：用量大于窗口。暂停着、也到了线的照压缩失败写。
    t.context = 203_000;
    assert_eq!(lay(&t, used(1), &config)[2], "上下文压缩失败");
    push(&mut t, Push::Compacted { clear: true }, &config);
    t.context = 203_000;
    assert_eq!(
        lay(&t, used(1), &config),
        ["203k/128k(158.6%)", "Σ1(C0%)", "上下文超出窗口", "压缩"],
        "清空过不再算压缩失败，超出窗口照写"
    );
}

#[test]
fn compacting_shows_a_spinner_after_the_numbers() {
    let config = Config::builtin().unwrap();
    let mut t = Transcript::default();
    t.context = 203_000;
    let progress = Compaction::Progress {
        written: 10,
        expected: None,
        manual: Some(true),
    };
    push(&mut t, Push::Compaction(progress), &config);
    let laid = lay(&t, used(1), &config);
    assert_eq!(laid.len(), 3, "{laid:?}");
    assert_eq!(laid[..2], ["203k", "Σ1(C0%)"]);
    assert!(laid[2].ends_with(" 正在压缩上下文"), "{laid:?}");
    assert!(laid[2].starts_with(&config.timeline.spinner[0]));
}
