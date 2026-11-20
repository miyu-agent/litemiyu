//! 会话的账：主会话照引擎交来的，子代理的照推来的一笔笔加，合起来是底栏、`/status` 的累计和花费（照 Miyu 连同子代理）。

use std::collections::BTreeMap;

use lm_engine::session::Meter;
use lm_kernel::kernel::event::Real;

use crate::app::App;
use crate::config::Config;
use crate::core::{Core, Push, Update, Usage};

fn app() -> App {
    let mut app = App::new(
        Config::builtin().expect("出厂的配置读得懂"),
        Core::new(|_| true),
    );
    app.core(Update::Ready("main".into()));
    app
}

fn usage(uncached: u64, output: u64) -> Usage {
    Usage {
        uncached,
        output,
        ..Usage::default()
    }
}

fn meter(amounts: &[(&str, f64)]) -> Box<Meter> {
    let mut meter = super::empty();
    meter.amounts = amounts
        .iter()
        .map(|(currency, amount)| ((*currency).to_string(), *amount))
        .collect::<BTreeMap<_, _>>();
    Box::new(meter)
}

fn child(app: &mut App, session: &str, push: Push) {
    app.core(Update::Child {
        session: session.into(),
        push,
    });
}

#[test]
fn the_main_meter_and_the_agents_add_up_like_miyu() {
    let mut app = app();
    assert!(app.talk.spent().is_empty(), "还没有账：没有数");
    assert_eq!(app.seen().spent(), None);
    app.core(Update::Meter {
        session: "main".into(),
        meter: meter(&[("USD", 0.4)]),
    });
    assert_eq!(app.seen().spent().as_deref(), Some("$0.40"));
    // 子代理：一次主请求、一次起标题，都有价；孙代理一次人民币。
    child(&mut app, "s-a", Push::Usage(usage(100, 10)));
    child(
        &mut app,
        "s-a",
        Push::Billed(Some(("USD".into(), Real::new(0.02)))),
    );
    child(&mut app, "s-a", Push::AuxUsage(usage(30, 5)));
    child(
        &mut app,
        "s-a",
        Push::Billed(Some(("USD".into(), Real::new(0.001)))),
    );
    child(&mut app, "s-b", Push::Usage(usage(50, 0)));
    child(
        &mut app,
        "s-b",
        Push::Billed(Some(("CNY".into(), Real::new(1.3)))),
    );
    assert_eq!(app.seen().spent().as_deref(), Some("$0.421 + ¥1.30"));
    let total = app.talk.usage_total();
    assert_eq!(
        (total.uncached, total.output, total.aux),
        (150, 10, 35),
        "子代理主请求的四项照主请求加，起标题的算进辅助请求"
    );
    // 没价格的那次不写数（算进 `unpriced`）。
    child(&mut app, "s-b", Push::Usage(usage(1, 0)));
    child(&mut app, "s-b", Push::Billed(None));
    assert_eq!(app.talk.agent_meters["s-b"].unpriced, 1);
}

#[test]
fn a_meter_for_another_session_is_dropped_and_a_new_session_starts_empty() {
    let mut app = app();
    app.core(Update::Meter {
        session: "old".into(),
        meter: meter(&[("USD", 9.0)]),
    });
    assert!(app.talk.meter.is_none(), "换会话以前的旧账不要");
    app.core(Update::Meter {
        session: "main".into(),
        meter: meter(&[("USD", 0.5)]),
    });
    child(&mut app, "s-a", Push::Usage(usage(1, 1)));
    app.reset_talk();
    assert!(app.talk.meter.is_none() && app.talk.agent_meters.is_empty());
    assert_eq!(app.seen().spent(), None);
}

#[test]
fn a_meter_with_no_money_draws_no_spent_part() {
    let mut app = app();
    app.core(Update::Meter {
        session: "main".into(),
        meter: meter(&[]),
    });
    assert_eq!(app.seen().spent(), None, "模型没价格：不是 $0.00");
}

/// 第二十六趟：重开的会话，开它以前子代理用掉的照引擎交来的一份（`Update::EarlierAgents`）加进累计和花费，底栏、侧边栏、
/// `/status` 都照它；这个窗口里新派的照推来的另加；换会话以后清掉，别的会话的不要。
#[test]
fn a_reopened_session_counts_what_its_subagents_spent_before() {
    let mut app = app();
    app.core(Update::Meter {
        session: "main".into(),
        meter: meter(&[("USD", 0.4)]),
    });
    let mut earlier = meter(&[("USD", 0.1)]);
    earlier.main = lm_kernel::kernel::event::Usage {
        uncached: 200,
        cache_read: 0,
        cache_write: 0,
        output: 20,
        reasoning: None,
    };
    earlier.total = earlier.main;
    app.core(Update::EarlierAgents {
        session: "old".into(),
        meter: earlier.clone(),
    });
    assert!(app.talk.earlier_agents.is_none(), "别的会话的不要");
    app.core(Update::EarlierAgents {
        session: "main".into(),
        meter: earlier,
    });
    assert_eq!(app.seen().spent().as_deref(), Some("$0.50"));
    let total = app.talk.usage_total();
    assert_eq!((total.uncached, total.output), (200, 20));
    child(&mut app, "s-a", Push::Usage(usage(100, 10)));
    let total = app.talk.usage_total();
    assert_eq!((total.uncached, total.output), (300, 30), "新派的另加");
    app.reset_talk();
    assert!(app.talk.earlier_agents.is_none());
    assert_eq!(app.talk.usage_total().uncached, 0);
}
