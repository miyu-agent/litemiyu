//! 起标题（施工 3-8 五补、TT-1，`docs/blueprint/kernel/session.md`「起标题」）的这一层：回合进行中人要求重起的，`turn.ended`
//! 落了盘才起、照落了盘的有效历史组装；对不上在路上的那一次的回报不理（名字、用途都要对得上）；标题记下以前不回什么（要它的
//! 命令收下时就回过了）。整轮怎么走在 `scenario/retitle.rs`，收回报的共用规矩在 `recap.rs` 测。

use super::executor::*;
use super::*;
use crate::kernel::accumulate::{Delta, Kind};
use crate::kernel::event::{MetaChanged, Purpose};
use crate::kernel::id::ContentHash;

/// 标题最多 50 个字的会话：说了一句，回合进行中人要求重起标题（命令 2，这时组装不出来，等这一轮完），她答完了，
/// `turn.ended`（8）还没落盘。
fn answered() -> Session {
    let mut session = session_with(Policy {
        titles: Some(Titles { chars: 50 }),
        ..policy()
    });
    session.handle(send(1, "hi"));
    session.handle(stored(5));
    session.handle(hooks_done(turn3(), Vec::new()));
    let asked = session.handle(Input::Command(Received {
        id: id(2),
        by: person(),
        at: at(30),
        command: Command::Retitle,
    }));
    assert!(
        !asked
            .iter()
            .any(|action| matches!(action, Action::Aside { .. })),
        "还没有回答，等这一轮完：{asked:?}"
    );
    let done = answer(&mut session, 5, "好。");
    assert_eq!(appended(&done), seqs(&[6, 7, 8]));
    session
}

/// 起标题的一样回报：用途 `purpose`，名字 `upto`。
fn aside_sent(purpose: Purpose, upto: u64) -> Input {
    Input::AsideSent {
        at: at(40),
        purpose,
        upto: seq(upto),
        model: deepseek(),
        request: ContentHash::of(b"title"),
        fell_back: false,
    }
}

/// 一段正文增量。
fn aside_delta(purpose: Purpose, upto: u64, delta: Delta) -> Input {
    Input::AsideDelta {
        at: at(41),
        purpose,
        upto: seq(upto),
        delta,
    }
}

/// 说完了。
fn aside_ended(purpose: Purpose, upto: u64) -> Input {
    Input::AsideEnded {
        sent: None,
        at: at(45),
        purpose,
        upto: seq(upto),
        usage: None,
        cost: None,
        error: None,
    }
}

/// 说了 `text` 这一块正文。
fn aside_said(purpose: &Purpose, upto: u64, text: &str) -> Vec<Input> {
    [
        Delta::Start {
            index: 0,
            kind: Kind::Text,
        },
        Delta::Text {
            index: 0,
            text: text.to_string(),
        },
        Delta::End { index: 0 },
    ]
    .into_iter()
    .map(|delta| aside_delta(purpose.clone(), upto, delta))
    .collect()
}

#[test]
fn it_asks_once_the_turn_end_is_stored() {
    let mut session = answered();
    let early = session.handle(stored(7));
    assert!(
        !early
            .iter()
            .any(|action| matches!(action, Action::Aside { .. })),
        "turn.ended 还没落盘，不起：{early:?}"
    );
    let asked = session.handle(stored(8));
    let titles: Vec<(&Purpose, Seq, String)> = asked
        .iter()
        .filter_map(|action| match action {
            Action::Aside {
                purpose,
                upto,
                request,
            } => Some((purpose, *upto, listed_request(request))),
            _ => None,
        })
        .collect();
    assert_eq!(
        titles,
        [(
            &Purpose::Title,
            seq(6),
            "6 message.assistant\nretitle\n".to_string()
        )]
    );
    assert!(
        asked.contains(&Action::RunTurnEndHooks { turn: turn3() }),
        "回合结束的挂接点照跑"
    );
}

#[test]
fn reports_for_another_request_are_ignored() {
    let mut session = answered();
    session.handle(stored(8));
    // 名字对不上、用途对不上（回顾那一路）、不认识的用途：都不理。
    for stale in [
        aside_ended(Purpose::Title, 5),
        aside_ended(Purpose::Recap, 6),
        aside_ended(Purpose::Other("vision".to_string()), 6),
        aside_sent(Purpose::Recap, 6),
    ] {
        assert!(session.handle(stale).is_empty(), "对不上的不理");
    }
    for purpose in [Purpose::Recap, Purpose::Other("vision".to_string())] {
        for stale in aside_said(&purpose, 6, "串进来的") {
            assert!(session.handle(stale).is_empty());
        }
    }
    session.handle(aside_sent(Purpose::Title, 6));
    for input in aside_said(&Purpose::Title, 6, "打招呼") {
        assert!(session.handle(input).is_empty(), "增量不推给头");
    }
    let done = session.handle(aside_ended(Purpose::Title, 6));
    let events = appended_events(&done);
    assert_eq!(appended(&done), seqs(&[9, 10]));
    assert_eq!(
        events[1].body,
        Body::MetaChanged(MetaChanged {
            title: Some("打招呼".to_string()),
            pinned: None,
        }),
        "对不上的增量没收进来"
    );
    assert!(replies(&done).is_empty(), "要它的命令收下时就回过了");
    assert!(
        session.handle(aside_ended(Purpose::Title, 6)).is_empty(),
        "说完了的再报一次不理"
    );
}

/// 扩展提供的驱动报的真发出去的字节的哈希（施工 MD-2）：起标题这类旁路请求也记进 `model.called` 的 `sent`。
#[test]
fn the_title_request_keeps_what_the_driver_sent() {
    let mut session = answered();
    session.handle(stored(8));
    session.handle(aside_sent(Purpose::Title, 6));
    for input in aside_said(&Purpose::Title, 6, "打招呼") {
        session.handle(input);
    }
    let sent = crate::kernel::id::ContentHash::of(b"wire");
    let done = session.handle(Input::AsideEnded {
        sent: Some(sent.clone()),
        at: at(45),
        purpose: Purpose::Title,
        upto: seq(6),
        usage: None,
        cost: None,
        error: None,
    });
    let called = appended_events(&done)
        .into_iter()
        .find_map(|event| match event.body {
            Body::ModelCalled(called) => Some(called),
            _ => None,
        })
        .expect("记了 model.called");
    assert_eq!(called.sent, Some(sent));
}

/// 旁路请求在池里换了人、退到了会话的模型（施工 Q-2）：还没收到增量就报了两次发出去了，记后一次（真发给的那一个）和它退
/// 过没有；收到增量以后再报的不理。
#[test]
fn a_resend_before_any_delta_is_the_one_recorded() {
    use crate::kernel::id::{ModelName, ProviderId};
    use crate::kernel::origin::Model;
    let mut session = answered();
    session.handle(stored(8));
    session.handle(aside_sent(Purpose::Title, 6));
    let fallback = Model {
        endpoint: ProviderId::parse("a").expect("合写法"),
        model: ModelName::parse("m").expect("合写法"),
    };
    let resent = |model: Model, fell_back: bool| Input::AsideSent {
        at: at(42),
        purpose: Purpose::Title,
        upto: seq(6),
        model,
        request: ContentHash::of(b"title"),
        fell_back,
    };
    session.handle(resent(fallback.clone(), true));
    for input in aside_said(&Purpose::Title, 6, "打招呼") {
        session.handle(input);
    }
    session.handle(resent(deepseek(), false));
    let done = session.handle(aside_ended(Purpose::Title, 6));
    let called = appended_events(&done)
        .into_iter()
        .find_map(|event| match event.body {
            Body::ModelCalled(called) => Some(called),
            _ => None,
        })
        .expect("记了 model.called");
    assert_eq!(
        (called.endpoint, called.model, called.fell_back),
        (Some(fallback.endpoint), Some(fallback.model), true)
    );
}

/// 旧日志（会话自己起标题砍掉以前写的）：答完以后内核起过两次，一次出错、一次起成，记着两条 `purpose` 是 `title` 的
/// `model.called` 和一条 `by` 内核的 `session.meta_changed`。照旧载入，不出错、不补记什么，标题照旧；人要求重起照旧起得了，
/// 盖掉内核起的，算人起的。
#[test]
fn an_old_log_with_titles_the_kernel_gave_loads_and_can_be_retitled() {
    let mut logged = super::load::Logged::new();
    let seen = logged.ask(1, "hi");
    logged.say(seen, "好。");
    let answer = logged
        .log
        .iter()
        .rev()
        .find(|event| matches!(event.body, Body::MessageAssistant(_)))
        .map(|event| event.seq.get())
        .unwrap();
    let next = logged.last() + 1;
    for line in [
        format!(
            r#"{{"seq":{next},"at":"2026-09-25T07:00:20.000Z","kind":"model.called","by":{{"kind":"kernel"}},"body":{{"seen":{answer},"messages":1,"result":"error","error":{{"class":"retryable","message":"503"}},"purpose":"title"}}}}"#
        ),
        format!(
            r#"{{"seq":{},"at":"2026-09-25T07:00:21.000Z","kind":"model.called","by":{{"kind":"kernel"}},"body":{{"seen":{answer},"messages":1,"result":"ok","purpose":"title"}}}}"#,
            next + 1
        ),
        format!(
            r#"{{"seq":{},"at":"2026-09-25T07:00:22.000Z","kind":"session.meta_changed","by":{{"kind":"kernel"}},"body":{{"title":"内核起的"}}}}"#,
            next + 2
        ),
    ] {
        logged.log.push(Event::from_line(&line).unwrap());
    }
    let (mut session, actions) = Session::load(
        session_id(),
        logged.log.clone(),
        at(55),
        Policy {
            titles: Some(Titles { chars: 50 }),
            ..policy()
        },
        environment("~/src/miyu"),
    )
    .unwrap();
    assert!(appended_events(&actions).is_empty(), "不补记：{actions:?}");
    assert!(
        !actions
            .iter()
            .any(|action| matches!(action, Action::Aside { .. })),
        "载入不起标题：{actions:?}"
    );
    // 标题照旧：改成一样的，什么都不记。
    let same = session.handle(Input::Command(Received {
        id: id(2),
        by: person(),
        at: at(56),
        command: Command::SetMeta {
            title: Some("内核起的".to_string()),
            pinned: None,
        },
    }));
    assert_eq!(same, [accepted_reply(2, &[])], "标题照旧");
    // 人要求重起：照最近几轮发一次，起好的盖掉内核起的，算人起的。
    let asked = session.handle(Input::Command(Received {
        id: id(3),
        by: person(),
        at: at(57),
        command: Command::Retitle,
    }));
    let [upto] = asked
        .iter()
        .filter_map(|action| match action {
            Action::Aside {
                purpose: Purpose::Title,
                upto,
                request,
            } => {
                assert_eq!(
                    listed_request(request),
                    format!("{answer} message.assistant\nretitle\n")
                );
                Some(upto.get())
            }
            _ => None,
        })
        .collect::<Vec<_>>()[..]
    else {
        panic!("发一次起标题的请求：{asked:?}");
    };
    session.handle(aside_sent(Purpose::Title, upto));
    for input in aside_said(&Purpose::Title, upto, "重起的") {
        session.handle(input);
    }
    let done = session.handle(aside_ended(Purpose::Title, upto));
    let events = appended_events(&done);
    let [called, renamed] = &events[..] else {
        panic!("两条：{events:?}");
    };
    assert!(matches!(called.body, Body::ModelCalled(_)), "{called:?}");
    assert_eq!(
        (&renamed.body, &renamed.by),
        (
            &Body::MetaChanged(MetaChanged {
                title: Some("重起的".to_string()),
                pinned: None,
            }),
            &person()
        )
    );
}
