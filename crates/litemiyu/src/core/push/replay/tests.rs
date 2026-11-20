use lm_kernel::kernel::event::Event;

use super::{Block, Push, Replay};

fn event(json: &str) -> Event {
    serde_json::from_str(json).expect("事件的写法")
}

/// 一条回答：先想、再说。
fn answer() -> Event {
    event(
        r#"{"seq":5,"at":"2026-11-12T08:00:10.000Z","kind":"message.assistant","by":{"kind":"model","endpoint":"fake","model":"m1"},"body":{"seen":4,"blocks":[{"type":"reasoning","text":"想想"},{"type":"text","text":"CI 还在跑。"}]}}"#,
    )
}

/// 它的 `model.called`：08:00:10.500 落盘，用了 10 秒（08:00:00.500 发出去），想 1–3.5 秒、说 3.6–9.8 秒。
fn called(seen: u64) -> Event {
    event(&format!(
        r#"{{"seq":6,"at":"2026-11-12T08:00:10.500Z","kind":"model.called","turn":2,"by":{{"kind":"kernel"}},"body":{{"seen":{seen},"messages":2,"duration_ms":10000,"blocks":[{{"start_ms":1000,"end_ms":3500}},{{"start_ms":3600,"end_ms":9800}}],"result":"ok"}}}}"#
    ))
}

fn at(text: &str) -> Push {
    Push::Clock(Some(text.parse().expect("时刻")))
}

#[test]
fn a_stored_answer_reads_as_the_blocks_it_streamed() {
    let mut replay = Replay::default();
    assert!(
        replay.read(answer()).is_empty(),
        "回答先记着，等它的 model.called"
    );
    let pushes = replay.finish();
    let blocks: Vec<&Push> = pushes
        .iter()
        .filter(|p| !matches!(p, Push::Clock(_)))
        .collect();
    assert_eq!(
        blocks[..6],
        [
            &Push::BlockStart {
                index: 0,
                block: Block::Reasoning
            },
            &Push::Delta {
                index: 0,
                text: "想想".into()
            },
            &Push::BlockEnd(0),
            &Push::BlockStart {
                index: 1,
                block: Block::Text
            },
            &Push::Delta {
                index: 1,
                text: "CI 还在跑。".into()
            },
            &Push::BlockEnd(1),
        ]
    );
    assert!(
        blocks[6..].contains(&&Push::Calls(Vec::new())),
        "接着照平常读：{pushes:?}"
    );
    assert_eq!(pushes.last(), Some(&Push::Clock(None)), "补完了钟回到现在");
}

#[test]
fn each_block_starts_and_ends_at_the_times_its_model_called_recorded() {
    let mut replay = Replay::default();
    assert!(replay.read(answer()).is_empty());
    let pushes = replay.read(called(4));
    let start = pushes
        .iter()
        .position(|p| matches!(p, Push::BlockStart { index: 0, .. }))
        .expect("开了第一块");
    assert_eq!(
        pushes[start - 1],
        at("2026-11-12T08:00:01.500Z"),
        "想从第 1 秒起"
    );
    let end = pushes
        .iter()
        .position(|p| *p == Push::BlockEnd(0))
        .expect("收了第一块");
    assert_eq!(
        pushes[end - 1],
        at("2026-11-12T08:00:04.000Z"),
        "想到第 3.5 秒"
    );
    let start = pushes
        .iter()
        .position(|p| matches!(p, Push::BlockStart { index: 1, .. }))
        .expect("开了第二块");
    assert_eq!(pushes[start - 1], at("2026-11-12T08:00:04.100Z"));
    let end = pushes
        .iter()
        .position(|p| *p == Push::BlockEnd(1))
        .expect("收了第二块");
    assert_eq!(pushes[end - 1], at("2026-11-12T08:00:10.300Z"));
    assert!(
        pushes[end..].contains(&at("2026-11-12T08:00:10.500Z")),
        "接着照 model.called 自己的时刻读它：{pushes:?}"
    );
    assert!(
        pushes
            .iter()
            .any(|p| matches!(p, Push::Sent { .. }) || matches!(p, Push::CallOk)),
        "model.called 照平常读：{pushes:?}"
    );
}

#[test]
fn an_answer_without_its_model_called_reads_at_the_moment_it_was_stored() {
    let mut replay = Replay::default();
    assert!(replay.read(answer()).is_empty());
    // 隔着别的请求的 model.called（`seen` 对不上）：回答照自己落盘的时刻读，那一条照平常读。
    let pushes = replay.read(called(9));
    let clocks: Vec<&Push> = pushes
        .iter()
        .take_while(|p| !matches!(p, Push::Calls(_)))
        .filter(|p| matches!(p, Push::Clock(_)))
        .collect();
    assert!(
        clocks.iter().all(|c| **c == at("2026-11-12T08:00:10.000Z")),
        "{pushes:?}"
    );
}

#[test]
fn a_replayed_thought_takes_as_long_as_it_streamed() {
    use crate::config::Config;
    use crate::transcript::{StepKind, Transcript};
    let mut replay = Replay::default();
    let mut pushes = replay.read(answer());
    pushes.extend(replay.read(called(4)));
    pushes.extend(replay.finish());
    let texts = Config::builtin().expect("自带的配置").text().clone();
    let mut t = Transcript::default();
    for push in pushes {
        t.update(crate::core::Update::Push(push), &texts);
    }
    let thought = t
        .entries
        .iter()
        .filter_map(|e| e.segment.as_ref())
        .flat_map(|s| s.steps.iter())
        .find(|s| matches!(s.kind, StepKind::Thought { .. }))
        .expect("有一步思考");
    assert_eq!(
        thought.took,
        Some(std::time::Duration::from_millis(2500)),
        "已思考的用时照 model.called 记的起止算，不是 0"
    );
}
