//! `schedule_wakeup`（Miyu 施工 J-2）：间隔收边界、原话覆盖、停、没给间隔、没开过循环、没有端口、给人看的说法带理由。端口是假的：
//! 真的在 `lm_engine::session` 里测（`tests/session/looping.rs`）。从 Miyu `miyu-schedule/tests/wakeup.rs` 搬来（L5 四），连同那边
//! `support.rs` 里用到的、`budget.rs` 里循环的那一份说明预算（最后一个）。端口不再带会话编号（`tool/schedules.rs`），记下的只有
//! 她要做的。

use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};

use serde_json::json;

use lm_engine::tool::{
    Done, Progress, ScheduleAnswer, ScheduleAnswering, ScheduleAsk, SchedulePort, ScheduleRefused,
    Tool,
};
use lm_kernel::kernel::block::Block;
use lm_kernel::kernel::time::Timestamp;

/// 一个假端口：记下她要做的，照预先定好的交回。
struct Fake {
    asked: Mutex<Vec<ScheduleAsk>>,
    answer: Result<ScheduleAnswer, ScheduleRefused>,
}

impl Fake {
    fn answering(answer: Result<ScheduleAnswer, ScheduleRefused>) -> Arc<Fake> {
        Arc::new(Fake {
            asked: Mutex::default(),
            answer,
        })
    }

    fn asked(&self) -> Vec<ScheduleAsk> {
        self.asked
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl SchedulePort for Fake {
    fn schedule(&self, ask: ScheduleAsk) -> ScheduleAnswering<'_> {
        self.asked
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(ask);
        let answer = self.answer.clone();
        Box::pin(async move { answer })
    }
}

/// 用假端口 `port`（没有的是这个会话用不了）调一次。
async fn run(port: Option<&Arc<Fake>>, args: serde_json::Value) -> Done {
    let mut call = lm_engine::tool::testkit::call(&args.to_string());
    call.schedules = port.map(|port| Arc::clone(port) as Arc<dyn SchedulePort>);
    let tool: Arc<dyn Tool> = lm_engine::schedule::tools().expect("读得出").remove(0);
    tool.run(call, Progress::new(|_| {})).await
}

/// 动态循环改好了：下一次在 2026-10-27 09:00（UTC）。
fn woken() -> ScheduleAnswer {
    ScheduleAnswer::Woken {
        next: Timestamp::parse("2026-10-27T09:00:00.000Z").expect("合写法"),
    }
}

/// 给她看的字。
fn text(done: &Done) -> String {
    done.blocks
        .iter()
        .map(|block| match block {
            Block::Text(text) => text.text.clone(),
            other => panic!("只该有字：{other:?}"),
        })
        .collect()
}

/// 给人看的说法：编号和字段。
fn said(done: &Done) -> (String, Vec<(String, String)>) {
    let said = done.human.as_ref().expect("有给人看的说法");
    let fields = said
        .fields
        .iter()
        .map(|(name, value)| (name.clone(), value.clone()))
        .collect();
    (said.key.clone(), fields)
}

#[tokio::test]
async fn the_delay_is_kept_within_bounds() {
    for (given, kept) in [
        (5, 60),
        (-3, 60),
        (600, 600),
        (99_999, 99_999),
        (i64::MAX, i64::MAX as u64),
    ] {
        let port = Fake::answering(Ok(woken()));
        run(Some(&port), json!({"delay_seconds": given, "reason": "x"})).await;
        assert_eq!(
            port.asked(),
            [ScheduleAsk::Wakeup {
                delay: kept,
                prompt: None
            }],
            "{given} 秒"
        );
    }
}

#[tokio::test]
async fn a_wakeup_says_when_and_shows_the_reason() {
    let port = Fake::answering(Ok(woken()));
    let done = run(
        Some(&port),
        json!({"delay_seconds": 1200, "prompt": "盯 CI", "reason": "CI 大约二十分钟"}),
    )
    .await;
    assert!(!done.error);
    assert_eq!(text(&done), "Next iteration at 2026-10-27 09:00.");
    assert_eq!(
        port.asked()[0],
        ScheduleAsk::Wakeup {
            delay: 1_200,
            prompt: Some("盯 CI".to_string()),
        }
    );
    assert_eq!(
        said(&done),
        (
            "software/loop/wakeup/set".to_string(),
            vec![
                ("next".to_string(), "2026-10-27 09:00".to_string()),
                ("reason".to_string(), "CI 大约二十分钟".to_string()),
            ]
        )
    );
}

#[tokio::test]
async fn a_blank_prompt_keeps_the_old_one() {
    let port = Fake::answering(Ok(woken()));
    run(Some(&port), json!({"delay_seconds": 60, "prompt": " "})).await;
    assert_eq!(
        port.asked()[0],
        ScheduleAsk::Wakeup {
            delay: 60,
            prompt: None
        }
    );
}

#[tokio::test]
async fn stop_ends_the_loop_and_ignores_the_rest() {
    let port = Fake::answering(Ok(ScheduleAnswer::LoopStopped));
    let done = run(Some(&port), json!({"stop": true, "delay_seconds": 5})).await;
    assert!(!done.error);
    assert_eq!(text(&done), "Loop stopped.");
    assert_eq!(port.asked(), [ScheduleAsk::StopLoop]);
}

#[tokio::test]
async fn without_a_delay_it_asks_for_one() {
    let port = Fake::answering(Ok(woken()));
    let done = run(Some(&port), json!({"reason": "x"})).await;
    assert!(done.error);
    assert_eq!(
        text(&done),
        "delay_seconds is required unless stop is true."
    );
    assert!(port.asked().is_empty());
}

#[tokio::test]
async fn with_no_loop_running_it_says_so() {
    let port = Fake::answering(Err(ScheduleRefused::NoLoop));
    let done = run(Some(&port), json!({"delay_seconds": 60})).await;
    assert!(done.error);
    assert_eq!(text(&done), "No loop is running in this session.");
    assert_eq!(said(&done).0, "software/loop/wakeup/no-loop");
}

#[tokio::test]
async fn without_the_port_it_is_not_available() {
    let done = run(None, json!({"delay_seconds": 60})).await;
    assert!(done.error);
    assert_eq!(text(&done), "Loops are not available in this session.");
}

/// 循环的工具面预算（Miyu `miyu-schedule/tests/budget.rs` 里 `loop` 的那一份）：`resources/software/loop/tools/` 下的一份说明
/// 不超过 [`BUDGET`] 字节。预算：字节，回车 `\r` 不算（Windows 上检出的可能多出回车）。
const BUDGET: usize = 588;

#[test]
fn the_loop_tool_face_stays_within_its_budget() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../resources/software/loop/tools");
    let mut total = 0;
    let mut files = 0;
    for entry in std::fs::read_dir(&dir).expect("读得了工具说明的目录") {
        let path = entry.expect("读得了目录项").path();
        let bytes = std::fs::read(&path).expect("读得了说明");
        total += bytes.iter().filter(|byte| **byte != b'\r').count();
        files += 1;
    }
    assert_eq!(files, 1, "一件工具一份说明");
    assert!(total <= BUDGET, "说明 {total} 字节，超过预算 {BUDGET}");
}
