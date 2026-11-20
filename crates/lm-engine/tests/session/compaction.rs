//! 压缩接在会话上整条走通（执行计划第六节 L3 六）：窗口设得很小，会话说几轮以后到线自动压一次，压完接着答，落盘以后载得回；
//! 供应商报超长的被动压了再发；摘要请求自己超长的截掉最老的再发；自动压缩连着失败的熔断，明知放不下的请求不发。
//!
//! 剧本的用量一次比一次多 200（`Script::grows`），回了摘要的那一次从头涨。窗口 21700、没报最大输出：压缩线 = 21700 − 20000 − 1085
//! （余量是窗口的 5%）= 615，尾巴至多线的四分之一 153。人说的一句约 70 token（字节数除以 4）：主请求报 110、310、510、710，
//! 第四轮 510 + 70 不到线，第四轮答完锚是 710，第五轮一开头就过线；尾巴留得下最近两轮上下，更早的压掉。

use lm_engine::session::testkit::{Play, Script};
use lm_kernel::kernel::block::Block;
use lm_kernel::kernel::event::{Body, CompactTrigger, ContextCompacted, ErrorClass};
use lm_kernel::kernel::request::{Message, Request};

use crate::support::routing::configs;
use crate::support::*;

/// 窗口 21700，用量一次涨 200。
fn script(plays: impl IntoIterator<Item = Play>) -> Script {
    Script::new(plays).window(21_700).grows(200)
}

/// 关了提前压好的（`[compaction] prepare = false`）：到线当场压，请求的先后一定（提前压好的另有 `prepare_log` 守着）。
fn at_the_line() -> Home {
    let mut home = Home::new();
    home.configs = configs("[compaction]\nprepare = false\n", &[]);
    home
}

/// 这一次请求是不是摘要请求：最后一块是出厂的摘要指令。
fn is_summary(request: &Request) -> bool {
    matches!(
        request.messages.last(),
        Some(Message::User { blocks })
            if matches!(blocks.last(), Some(Block::Text(text)) if text.text.starts_with("Respond with text only."))
    )
}

/// 请求里人说的话，照先后。
fn said(request: &Request) -> Vec<String> {
    request
        .messages
        .iter()
        .filter_map(|message| match message {
            Message::User { blocks } => Some(blocks),
            _ => None,
        })
        .flatten()
        .filter_map(|block| match block {
            Block::Text(text) if text.text.starts_with("第") => {
                text.text.split('。').next().map(str::to_string)
            }
            _ => None,
        })
        .collect()
}

/// 日志里的每一次压缩。
fn compactions(home: &Home, id: &lm_kernel::kernel::id::SessionId) -> Vec<ContextCompacted> {
    home.log(id)
        .into_iter()
        .filter_map(|event| match event.body {
            Body::ContextCompacted(compacted) => Some(*compacted),
            _ => None,
        })
        .collect()
}

/// 说第 `n` 轮、等这一轮说完：「第n轮。」后面接一段约 280 字节的话。
async fn turn(handle: &lm_engine::session::Handle, n: usize) {
    let text = format!("第{n}轮。{}", "lorem ipsum dolor sit amet, ".repeat(10));
    let mut pushes = watch(handle).await;
    ask(handle, &format!("cmd-{n}"), say(&text))
        .await
        .expect("会话在跑");
    until_turn_ends(&mut pushes).await;
}

#[tokio::test]
async fn a_small_window_compacts_after_a_few_turns_answers_on_and_loads_back() {
    let script = script([
        Play::Says("一。"),
        Play::Says("二。"),
        Play::Says("三。"),
        Play::Says("四。"),
        Play::Says("<analysis>a</analysis><summary>用户数了四轮。</summary>"),
        Play::Says("五。"),
        Play::Says("六。"),
    ]);
    let home = at_the_line();
    let handle = home.create(&script).await;
    for n in 1..=5 {
        turn(&handle, n).await;
    }
    let id = handle.id().clone();

    let requests = script.requests();
    let summaries: Vec<usize> = (0..requests.len())
        .filter(|&at| is_summary(&requests[at].1))
        .collect();
    assert_eq!(summaries, [4], "前四轮不压，第五轮一开头压了一次");
    let compacted = compactions(&home, &id);
    assert_eq!(compacted.len(), 1, "{compacted:?}");
    assert_eq!(compacted[0].trigger, Some(CompactTrigger::Auto));
    // 压完接着答：这一次请求里有检查点，没有前四轮的原话。
    let after = &requests[5].1;
    assert!(!is_summary(after));
    let kept = said(after);
    assert_eq!(kept.last().map(String::as_str), Some("第5轮"), "{kept:?}");
    assert!(
        !kept.contains(&"第1轮".to_string()),
        "前面的压掉了：{kept:?}"
    );
    let words = format!("{after:?}");
    assert!(words.contains("用户数了四轮。"), "{words}");
    let log = home.log(&id);
    assert!(
        log.iter()
            .any(|event| matches!(&event.body, Body::TurnEnded(_))),
        "{:?}",
        kinds(&log)
    );
    stop(&handle).await;

    // 落盘的载得回：日志一样，接着说的请求从检查点往下拼。
    let loaded = home.load(&id, &script).await;
    assert_eq!(home.log(&id)[..log.len()], log[..]);
    turn(&loaded, 6).await;
    let requests = script.requests();
    let reloaded = &requests.last().expect("有请求").1;
    assert!(!is_summary(reloaded));
    let kept = said(reloaded);
    assert!(
        kept.ends_with(&["第5轮".to_string(), "第6轮".to_string()]),
        "{kept:?}"
    );
    assert!(
        !kept.contains(&"第1轮".to_string()),
        "从检查点往下拼：{kept:?}"
    );
    assert!(format!("{reloaded:?}").contains("用户数了四轮。"));
    assert_eq!(compactions(&home, &id).len(), 1, "载入以后没再压");
    stop(&loaded).await;
}

#[tokio::test]
async fn a_window_overflow_compacts_then_sends_again() {
    // 用量不涨：到不了线，第三轮供应商报超长
    let script = Script::new([
        Play::Says("一。"),
        Play::Says("二。"),
        Play::Fails {
            class: ErrorClass::ContextTooLong,
            wait_ms: None,
        },
        Play::Says("<summary>用户说了两轮。</summary>"),
        Play::Says("三。"),
    ])
    .window(21_700);
    let home = Home::new();
    let handle = home.create(&script).await;
    for n in 1..=3 {
        turn(&handle, n).await;
    }
    let id = handle.id().clone();
    stop(&handle).await;

    let compacted = compactions(&home, &id);
    assert_eq!(compacted.len(), 1, "{compacted:?}");
    assert_eq!(compacted[0].trigger, Some(CompactTrigger::Overflow));
    let requests = script.requests();
    assert_eq!(requests.len(), 5, "两轮、超长、摘要、再发");
    assert!(is_summary(&requests[3].1));
    let again = &requests[4].1;
    assert!(!is_summary(again));
    assert!(format!("{again:?}").contains("用户说了两轮。"));
}

#[tokio::test]
async fn a_summary_request_too_long_is_cut_and_sent_again() {
    let script = script([
        Play::Says("一。"),
        Play::Says("二。"),
        Play::Says("三。"),
        Play::Says("四。"),
        // 摘要请求自己报超长：截掉最老的几组再发一次
        Play::Fails {
            class: ErrorClass::ContextTooLong,
            wait_ms: None,
        },
        Play::Says("<summary>后几轮。</summary>"),
        Play::Says("五。"),
    ]);
    let home = at_the_line();
    let handle = home.create(&script).await;
    for n in 1..=5 {
        turn(&handle, n).await;
    }
    let id = handle.id().clone();
    stop(&handle).await;

    let requests = script.requests();
    assert_eq!(requests.len(), 7, "{}", requests.len());
    let (first, second) = (&requests[4].1, &requests[5].1);
    assert!(is_summary(first) && is_summary(second));
    assert!(
        said(second).len() < said(first).len(),
        "再发的截掉了最老的：{:?} → {:?}",
        said(first),
        said(second)
    );
    let compacted = compactions(&home, &id);
    assert_eq!(compacted.len(), 1, "截短以后压成了：{compacted:?}");
    assert!(format!("{:?}", requests[6].1).contains("后几轮。"));
}

#[tokio::test]
async fn auto_compaction_failing_again_and_again_pauses() {
    // 第五轮起每一轮都到线：摘要请求被拦下（压缩失败），这一轮就停在这里；连着三轮失败就暂停自动压缩，第八轮照发。
    let mut plays = vec![
        Play::Says("一。"),
        Play::Says("二。"),
        Play::Says("三。"),
        Play::Says("四。"),
    ];
    for _ in 0..3 {
        plays.push(Play::Fails {
            class: ErrorClass::ContentPolicy,
            wait_ms: None,
        });
    }
    plays.push(Play::Says("暂停着照答。"));
    let script = script(plays);
    let home = at_the_line();
    let handle = home.create(&script).await;
    for n in 1..=8 {
        turn(&handle, n).await;
    }
    let id = handle.id().clone();
    stop(&handle).await;

    assert!(compactions(&home, &id).is_empty(), "一次都没压成");
    let requests = script.requests();
    let summaries = requests
        .iter()
        .filter(|(_, request)| is_summary(request))
        .count();
    assert_eq!(summaries, 3, "连着失败三次就不再试");
    let log = home.log(&id);
    let paused = log
        .iter()
        .any(|event| matches!(&event.body, Body::CompactionPaused(_)));
    assert!(paused, "自动压缩暂停了：{:?}", kinds(&log));
    let last = &requests.last().expect("有请求").1;
    assert!(!is_summary(last), "暂停着，第八轮不压、照发");
    assert_eq!(said(last).last().map(String::as_str), Some("第8轮"));
}
