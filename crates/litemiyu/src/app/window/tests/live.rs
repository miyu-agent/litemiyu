//! 只留几屏和全留逐格对照，第十一趟没覆盖的几样（第十二趟补）：回合进行中（推送一块块流出来）往上翻、排着的话撤回以后、
//! 点开收起的一块滚走再滚回。

use crossterm::event::KeyCode;
use lm_kernel::kernel::event::Event;

use super::{Ui, at, both, next, replayed, session};
use crate::core::{Push, Update};
use crate::ui::rows::Target;

/// 接在 `events` 后面的一轮：先排进一句话、马上撤回（`withdraw`），回答十几段。交回这一轮的事件。
pub(super) fn one_more(events: &[Event], withdraw: bool) -> Vec<Event> {
    let mut seq = events.last().map_or(0, |e| e.seq.get());
    let n = 999;
    let mut lines = Vec::new();
    let said = next(&mut seq);
    lines.push(format!(
        r#"{{"seq":{said},"at":"{}","kind":"message.user","by":{{"kind":"person"}},"cause":"cmd-{n}","body":{{"blocks":[{{"type":"text","text":"最后一轮的问话：链接错误又出来了。"}}]}}}}"#,
        at(said)
    ));
    let started = next(&mut seq);
    lines.push(format!(
        r#"{{"seq":{started},"at":"{}","kind":"turn.started","turn":{started},"by":{{"kind":"kernel"}},"cause":"cmd-{n}","body":{{"trigger":{said},"cwd":"/tmp"}}}}"#,
        at(started)
    ));
    if withdraw {
        let queued = next(&mut seq);
        lines.push(format!(
            r#"{{"seq":{queued},"at":"{}","kind":"message.user","by":{{"kind":"person"}},"cause":"cmd-q","body":{{"blocks":[{{"type":"text","text":"排着的一句，马上撤回"}}]}}}}"#,
            at(queued)
        ));
        let back = next(&mut seq);
        lines.push(format!(
            r#"{{"seq":{back},"at":"{}","kind":"message.withdrawn","by":{{"kind":"person"}},"cause":"cmd-w","body":{{"messages":[{queued}]}}}}"#,
            at(back)
        ));
    }
    let seen = seq;
    let reply = next(&mut seq);
    let text = (0..14)
        .map(|k| format!("最后一轮回答的第 {k} 段：链接顺序照依赖倒过来排，静态库放在用它的后面。"))
        .collect::<Vec<_>>()
        .join("\\n\\n");
    lines.push(format!(
        r#"{{"seq":{reply},"at":"{}","kind":"message.assistant","turn":{started},"by":{{"kind":"model","endpoint":"fake","model":"m1"}},"cause":"cmd-{n}","body":{{"blocks":[{{"type":"reasoning","text":"再想一想"}},{{"type":"text","text":"{text}"}}],"seen":{seen}}}}}"#,
        at(reply)
    ));
    let called = next(&mut seq);
    lines.push(format!(
        r#"{{"seq":{called},"at":"{}","kind":"model.called","turn":{started},"by":{{"kind":"kernel"}},"cause":"cmd-{n}","body":{{"seen":{seen},"endpoint":"fake","model":"m1","messages":3,"usage":{{"uncached":60,"cache_read":147,"cache_write":0,"output":10}},"first_token_ms":200,"duration_ms":3000,"blocks":[{{"start_ms":200,"end_ms":900}},{{"start_ms":1000,"end_ms":2900}}],"result":"ok"}}}}"#,
        at(called)
    ));
    let ended = next(&mut seq);
    lines.push(format!(
        r#"{{"seq":{ended},"at":"{}","kind":"turn.ended","turn":{started},"by":{{"kind":"kernel"}},"cause":"cmd-{n}","body":{{"reason":"completed"}}}}"#,
        at(ended)
    ));
    lines
        .iter()
        .map(|line| serde_json::from_str(line).expect("事件的写法"))
        .collect()
}

/// 推送拆成流出来的样子：一段字拆成几截到。
fn streamed(pushes: Vec<Push>) -> Vec<Push> {
    let mut out = Vec::new();
    for push in pushes {
        let Push::Delta { index, text } = push else {
            out.push(push);
            continue;
        };
        let chars: Vec<char> = text.chars().collect();
        for part in chars.chunks(37) {
            out.push(Push::Delta {
                index,
                text: part.iter().collect(),
            });
        }
    }
    out
}

/// 回合进行中画出来的不比的那几行：等第一个字时转圈的那一行（转到第几格看时刻）和它以下（运行状态行的词随机、用时照钟）。
pub(super) fn steady(screen: Vec<String>) -> Vec<String> {
    let spinner = |line: &String| {
        line.trim_start()
            .chars()
            .next()
            .is_some_and(|c| ('\u{2800}'..='\u{28ff}').contains(&c))
    };
    let status = |line: &String| {
        line.split_whitespace().last().is_some_and(|last| {
            last.strip_suffix('s')
                .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
        })
    };
    let end = screen
        .iter()
        .position(|line| spinner(line) || status(line))
        .unwrap_or(screen.len());
    screen[..end].to_vec()
}

/// 两个界面一起收这一轮的推送：每收一条画稳、比（转圈、运行状态行不比），每收四条一起按一下 `key`。
fn stream_both(full: &mut Ui, kept: &mut Ui, extra: &[Event], key: KeyCode) {
    for ui in [&mut *full, &mut *kept] {
        ui.events.extend_from_slice(extra);
    }
    for (k, push) in streamed(replayed(extra)).into_iter().enumerate() {
        full.app.core(Update::Push(push.clone()));
        kept.app.core(Update::Push(push));
        let (a, b) = (steady(full.settle()), steady(kept.settle()));
        assert_eq!(a, b, "流到第 {k} 条推送");
        if k % 4 == 3 {
            let (a, b) = (steady(full.key(key)), steady(kept.key(key)));
            assert_eq!(a, b, "流到第 {k} 条时按一下");
        }
    }
    assert_eq!(full.settle(), kept.settle(), "这一轮完了，整屏一样");
}

/// 翻回底，每一屏都比。
pub(super) fn back_down(full: &mut Ui, kept: &mut Ui, at: &str) {
    let mut down = 0;
    while full.app.screen.view.top.is_some() {
        down += 1;
        both(
            full,
            kept,
            KeyCode::PageDown,
            &format!("{at}：下翻第 {down} 下"),
        );
        assert!(down < 4000, "翻不回底");
    }
}

#[test]
fn paging_up_while_a_turn_streams_in_matches() {
    let events = session(80);
    let extra = one_more(&events, false);
    let mut full = Ui::open(&events, usize::MAX, 72, 20);
    let mut kept = Ui::open(&events, super::super::SCREENS, 72, 20);
    stream_both(&mut full, &mut kept, &extra, KeyCode::PageUp);
    assert!(full.app.screen.view.top.is_some(), "翻上去了");
    back_down(&mut full, &mut kept, "这一轮流完");
    assert!(kept.resident() * 4 < full.resident(), "回到底以后又放掉了");
}

#[test]
fn a_withdrawn_message_then_paging_matches() {
    let events = session(80);
    let extra = one_more(&events, true);
    let mut full = Ui::open(&events, usize::MAX, 72, 20);
    let mut kept = Ui::open(&events, super::super::SCREENS, 72, 20);
    // 跟着最新的看这一轮流完（排着的那句撤回了），再翻到顶、翻回底。
    stream_both(&mut full, &mut kept, &extra, KeyCode::Null);
    let mut pages = 0;
    while full.first() > 0 {
        pages += 1;
        both(
            &mut full,
            &mut kept,
            KeyCode::PageUp,
            &format!("撤回以后上翻第 {pages} 下"),
        );
        assert!(pages < 2000, "翻不到顶");
    }
    back_down(&mut full, &mut kept, "撤回以后");
}

/// 视口里第一个能点开收起的时间线（一段或者一步，各自的下标）。
fn fold_on_screen(ui: &Ui) -> Option<Target> {
    let view = &ui.app.screen.view;
    let height = usize::from(view.area.height);
    (view.first..view.first + height).find_map(|r| match view.rows.get(r)?.target {
        Some(t @ (Target::Segment(_) | Target::Step(..))) => Some(t),
        _ => None,
    })
}

#[test]
fn a_toggled_fold_scrolled_away_and_back_matches() {
    let events = session(90);
    let mut full = Ui::open(&events, usize::MAX, 72, 20);
    let mut kept = Ui::open(&events, super::super::SCREENS, 72, 20);
    // 往上翻到视口里有一段时间线。
    let mut k = 0;
    let (a, b) = loop {
        both(
            &mut full,
            &mut kept,
            KeyCode::PageUp,
            &format!("上翻第 {k} 下"),
        );
        k += 1;
        if let (Some(a), Some(b)) = (fold_on_screen(&full), fold_on_screen(&kept))
            && k > 6
        {
            break (a, b);
        }
        assert!(k < 200, "翻不到一段时间线");
    };
    full.app.toggle(a);
    kept.app.toggle(b);
    assert_eq!(full.settle(), kept.settle(), "点开了");
    // 滚走：再往上翻二十屏、翻回底，点开的那一段照旧点开着。
    for j in 0..20 {
        both(
            &mut full,
            &mut kept,
            KeyCode::PageUp,
            &format!("点开以后上翻第 {j} 下"),
        );
    }
    back_down(&mut full, &mut kept, "点开以后");
    for j in 0..k {
        both(
            &mut full,
            &mut kept,
            KeyCode::PageUp,
            &format!("回到点开的那一屏，第 {j} 下"),
        );
    }
    back_down(&mut full, &mut kept, "再翻回底");
}

#[test]
fn reading_back_laid_out_over_several_frames_still_matches() {
    // 真程序里读回的段照预算分几帧排完（整份重排的预算，`relayout_budget_ms`）：视口顶在空行上的照样不挪（第十二趟，伪终端
    // 160 列时撞上过差一行）。只留几屏的那一份预算调成 0：一帧只排视口附近必要的。
    let events = session(120);
    let mut full = Ui::open(&events, usize::MAX, 72, 20);
    let mut kept = Ui::open(&events, super::super::SCREENS, 72, 20);
    kept.app.config.layout.relayout_budget_ms = 0;
    let mut pages = 0;
    while full.first() > 0 {
        pages += 1;
        both(
            &mut full,
            &mut kept,
            KeyCode::PageUp,
            &format!("上翻第 {pages} 下"),
        );
        assert!(pages < 2000, "翻不到顶");
    }
    back_down(&mut full, &mut kept, "分帧排");
}

#[test]
fn read_backs_arriving_after_a_new_turn_still_match() {
    // 真程序里读回是另一条线程交回来的，可能在人说了新的一句、这一轮都流完以后才到：翻上去要的那几段晚到，再往上翻、翻回底，
    // 照样一样。
    let events = session(80);
    let extra = one_more(&events, false);
    let mut full = Ui::open(&events, usize::MAX, 72, 20);
    let mut kept = Ui::open(&events, super::super::SCREENS, 72, 20);
    for k in 0..15 {
        both(
            &mut full,
            &mut kept,
            KeyCode::PageUp,
            &format!("上翻第 {k} 下"),
        );
    }
    // 再往上翻，翻到要了读回为止，要的先不交。
    let mut late = Vec::new();
    for _ in 0..60 {
        for ui in [&mut full, &mut kept] {
            let page = crossterm::event::KeyEvent::new(
                KeyCode::PageUp,
                crossterm::event::KeyModifiers::NONE,
            );
            ui.app.key(page);
            ui.draw();
        }
        late.append(&mut kept.app.talk.wanted);
        if !late.is_empty() {
            break;
        }
    }
    assert!(!late.is_empty(), "翻上去要了读回");
    // 说了新的一句：回到底，这一轮流完（每条推送画一帧，不交读回）。
    for ui in [&mut full, &mut kept] {
        ui.app.screen.view.follow();
        ui.events.extend_from_slice(&extra);
        for push in streamed(replayed(&extra)) {
            ui.app.core(Update::Push(push));
            ui.draw();
        }
    }
    let pending = std::mem::take(&mut kept.app.talk.wanted);
    kept.app.talk.wanted = late.into_iter().chain(pending).collect();
    assert_eq!(full.settle(), kept.settle(), "晚到的读回交了以后，底下一样");
    for k in 0..8 {
        both(
            &mut full,
            &mut kept,
            KeyCode::PageUp,
            &format!("说完以后上翻第 {k} 下"),
        );
    }
    back_down(&mut full, &mut kept, "晚到的读回以后");
}

/// 接在 `events` 后面的一轮：调一次工具，工具回来以后、回答以前来了一条「没开着」的 `peer.idle`（一轮进行中的回报，第二十二趟
/// A4），再回答十几段。交回这一轮的事件。
fn with_report(events: &[Event]) -> Vec<Event> {
    let mut seq = events.last().map_or(0, |e| e.seq.get());
    let n = 998;
    let said = next(&mut seq);
    let started = next(&mut seq);
    let call = next(&mut seq);
    let called = next(&mut seq);
    let result = next(&mut seq);
    let idle = next(&mut seq);
    let seen = seq;
    let reply = next(&mut seq);
    let called_again = next(&mut seq);
    let ended = next(&mut seq);
    let text = (0..14)
        .map(|k| format!("回报以后回答的第 {k} 段：链接顺序照依赖倒过来排，静态库放在用它的后面。"))
        .collect::<Vec<_>>()
        .join("\\n\\n");
    let lines = [
        format!(
            r#"{{"seq":{said},"at":"{}","kind":"message.user","by":{{"kind":"person"}},"cause":"cmd-{n}","body":{{"blocks":[{{"type":"text","text":"盯住另一个窗口，再接着查链接错误。"}}]}}}}"#,
            at(said)
        ),
        format!(
            r#"{{"seq":{started},"at":"{}","kind":"turn.started","turn":{started},"by":{{"kind":"kernel"}},"cause":"cmd-{n}","body":{{"trigger":{said},"cwd":"/tmp"}}}}"#,
            at(started)
        ),
        format!(
            r#"{{"seq":{call},"at":"{}","kind":"message.assistant","turn":{started},"by":{{"kind":"model","endpoint":"fake","model":"m1"}},"cause":"cmd-{n}","body":{{"blocks":[{{"type":"tool_call","call_id":"call_{call}_1","name":"echo","args":"{{\"n\":{n}}}"}}],"seen":{started}}}}}"#,
            at(call)
        ),
        format!(
            r#"{{"seq":{called},"at":"{}","kind":"model.called","turn":{started},"by":{{"kind":"kernel"}},"cause":"cmd-{n}","body":{{"seen":{started},"endpoint":"fake","model":"m1","messages":1,"usage":{{"uncached":60,"cache_read":56,"cache_write":0,"output":10}},"first_token_ms":300,"duration_ms":2000,"blocks":[{{"start_ms":300,"end_ms":1900}}],"result":"ok"}}}}"#,
            at(called)
        ),
        format!(
            r#"{{"seq":{result},"at":"{}","kind":"tool.result","turn":{started},"by":{{"kind":"tool","call_id":"call_{call}_1"}},"cause":"cmd-{n}","body":{{"call_id":"call_{call}_1","status":"ok","blocks":[{{"type":"text","text":"echo {n}"}}]}}}}"#,
            at(result)
        ),
        format!(
            r#"{{"seq":{idle},"at":"{}","kind":"peer.idle","by":{{"kind":"kernel"}},"cause":"cmd-{n}","body":{{"session":"01a1d25d-b0db-733a-a362-186ba5869999","reason":"closed"}}}}"#,
            at(idle)
        ),
        format!(
            r#"{{"seq":{reply},"at":"{}","kind":"message.assistant","turn":{started},"by":{{"kind":"model","endpoint":"fake","model":"m1"}},"cause":"cmd-{n}","body":{{"blocks":[{{"type":"text","text":"{text}"}}],"seen":{seen}}}}}"#,
            at(reply)
        ),
        format!(
            r#"{{"seq":{called_again},"at":"{}","kind":"model.called","turn":{started},"by":{{"kind":"kernel"}},"cause":"cmd-{n}","body":{{"seen":{seen},"endpoint":"fake","model":"m1","messages":3,"usage":{{"uncached":60,"cache_read":147,"cache_write":0,"output":10}},"first_token_ms":200,"duration_ms":3000,"blocks":[{{"start_ms":200,"end_ms":2900}}],"result":"ok"}}}}"#,
            at(called_again)
        ),
        format!(
            r#"{{"seq":{ended},"at":"{}","kind":"turn.ended","turn":{started},"by":{{"kind":"kernel"}},"cause":"cmd-{n}","body":{{"reason":"completed"}}}}"#,
            at(ended)
        ),
    ];
    lines
        .iter()
        .map(|line| serde_json::from_str(line).expect("事件的写法"))
        .collect()
}

/// A4 的钉住（`Transcript::pin_last`）在会放掉再读回的长会话里：回报进了时间线的那一段钉住、翻到顶时照旧常驻，别的段照常
/// 放掉（钉住不让占用跟着会话长）；翻回底、点开那一段，回报那一步还在，和全留的逐格一样。
#[test]
fn a_report_inside_a_turn_stays_while_the_rest_is_let_go() {
    let events = session(80);
    let extra = with_report(&events);
    let mut full = Ui::open(&events, usize::MAX, 72, 20);
    let mut kept = Ui::open(&events, super::super::SCREENS, 72, 20);
    stream_both(&mut full, &mut kept, &extra, KeyCode::Null);
    // 回报进了时间线：在进行的那一段里多了回报那一步，正文里没有单独的那一行。
    let in_steps = |ui: &Ui| {
        ui.app.talk.transcript.entries.iter().any(|entry| {
            entry.segment.iter().flat_map(|s| s.steps.iter()).any(|step| {
                matches!(&step.kind, crate::transcript::StepKind::Report { text, .. } if text.contains("未打开"))
            })
        })
    };
    assert!(in_steps(&kept) && in_steps(&full), "回报是时间线里的一步");
    assert!(
        !kept
            .app
            .talk
            .transcript
            .entries
            .iter()
            .any(|entry| entry.text.contains("未打开")),
        "一轮进行中的回报不单独一行"
    );
    let pieces = kept.app.talk.transcript.pieces();
    let last = pieces.len() - 1;
    assert!(
        pieces.flag(last, crate::transcript::pieces::PINNED),
        "回报那一段钉住了"
    );
    let mut pages = 0;
    while full.first() > 0 {
        pages += 1;
        both(
            &mut full,
            &mut kept,
            KeyCode::PageUp,
            &format!("回报以后上翻第 {pages} 下"),
        );
        assert!(pages < 2000, "翻不到顶");
    }
    let pieces = kept.app.talk.transcript.pieces();
    assert!(pieces.resident(last), "翻到顶，钉住的那一段照旧常驻");
    let resident = (0..pieces.len()).filter(|p| pieces.resident(*p)).count();
    assert!(
        resident * 4 < pieces.len(),
        "别的段照常放掉：{} 段里常驻 {resident} 段",
        pieces.len()
    );
    assert!(
        kept.resident() * 4 < full.resident(),
        "别的段照常放掉：常驻 {} 条，全留的 {} 条",
        kept.resident(),
        full.resident()
    );
    back_down(&mut full, &mut kept, "回报以后");
    assert!(in_steps(&kept), "翻回底，回报那一步还在、字没变");
    // 点开回报那一段：两边逐格一样。
    let fold = |ui: &Ui| {
        let view = &ui.app.screen.view;
        view.rows
            .iter()
            .filter_map(|row| match row.target {
                Some(t @ Target::Segment(_)) => Some(t),
                _ => None,
            })
            .last()
    };
    let (Some(a), Some(b)) = (fold(&full), fold(&kept)) else {
        panic!("回报那一段的收起行在");
    };
    full.app.toggle(a);
    kept.app.toggle(b);
    let (a, b) = (full.settle(), kept.settle());
    assert_eq!(a, b, "点开以后一样");
}
