//! 子会话的时间线照主会话那一套只留几屏（「做法」第 9 条，第十三趟）：切进一个很长的子会话，全留和只留几屏逐格比；翻到顶、
//! 翻回底、切回主会话、再切进去；子会话还在跑时翻上去、回底，新出的字照旧跟着。读回照命令里的会话编号走子会话自己的日志
//! （测试里 `Ui::serve` 照编号挑那一份事件）。

use crossterm::event::KeyCode;
use lm_kernel::kernel::event::Event;

use super::live::{back_down, one_more, steady};
use super::{Ui, both, replayed, session};
use crate::core::{Push, Update};

const CHILD: &str = "01a1d25e-0c1d-7a3a-b0c2-5d8e0f7a1b2c";

impl Ui {
    /// 切进子会话（日志里是 `events`）：同引擎（`core/engine/view.rs`）先交一条空的头，再照补发一块块交（每块几十条，边读边交），
    /// 画稳。
    fn enter(&mut self, events: &[Event]) -> Vec<String> {
        self.child = Some((CHILD.to_string(), events.to_vec()));
        self.app.enter_child(CHILD.to_string());
        self.draw();
        let (done, _) = std::sync::mpsc::channel();
        self.app.core(viewed(Vec::new(), true, done));
        for chunk in replayed(events).chunks(37) {
            let (done, back) = std::sync::mpsc::channel();
            self.app.core(viewed(chunk.to_vec(), false, done));
            assert!(back.try_recv().is_ok(), "办完一块回一声");
            self.draw();
        }
        self.settle()
    }

    /// 回主会话，画稳。
    fn leave(&mut self) -> Vec<String> {
        self.app.leave_child();
        self.child = None;
        self.settle()
    }
}

fn viewed(pushes: Vec<Push>, first: bool, done: std::sync::mpsc::Sender<()>) -> Update {
    Update::Viewed {
        session: CHILD.to_string(),
        pushes,
        first,
        done,
    }
}

/// 一屏屏翻到顶，每一屏都比。
fn to_top(full: &mut Ui, kept: &mut Ui, at: &str) {
    let mut pages = 0;
    while full.first() > 0 {
        pages += 1;
        both(
            full,
            kept,
            KeyCode::PageUp,
            &format!("{at}：上翻第 {pages} 下"),
        );
        assert!(pages < 2000, "翻不到顶");
    }
}

#[test]
fn a_long_child_session_kept_to_a_few_screens_matches_keeping_it_all() {
    let main = session(30);
    let child = session(120);
    let mut full = Ui::open(&main, usize::MAX, 72, 20);
    let mut kept = Ui::open(&main, super::super::SCREENS, 72, 20);
    assert_eq!(full.enter(&child), kept.enter(&child), "切进子会话");
    assert!(
        kept.resident() * 4 < full.resident(),
        "子会话也只留底下几屏：常驻 {} 条，全留 {} 条",
        kept.resident(),
        full.resident()
    );
    to_top(&mut full, &mut kept, "子会话");
    back_down(&mut full, &mut kept, "子会话");
    assert_eq!(full.leave(), kept.leave(), "切回主会话");
    for k in 0..3 {
        both(
            &mut full,
            &mut kept,
            KeyCode::PageUp,
            &format!("主会话上翻第 {k} 下"),
        );
    }
    back_down(&mut full, &mut kept, "主会话");
    assert_eq!(full.enter(&child), kept.enter(&child), "再切进去");
    for k in 0..20 {
        both(
            &mut full,
            &mut kept,
            KeyCode::PageUp,
            &format!("再切进去上翻第 {k} 下"),
        );
    }
    // 翻上去时切回主会话（子会话要的读回在路上也不要了），再切进去。
    assert_eq!(full.leave(), kept.leave(), "翻上去时切回主会话");
    assert_eq!(full.enter(&child), kept.enter(&child), "第三次切进去");
    back_down(&mut full, &mut kept, "第三次切进去");
    assert!(kept.resident() * 4 < full.resident(), "回到底以后又放掉了");
}

#[test]
fn paging_in_a_child_session_that_is_still_running_matches() {
    let main = session(20);
    let child = session(100);
    let extra = one_more(&child, false);
    let mut full = Ui::open(&main, usize::MAX, 72, 20);
    let mut kept = Ui::open(&main, super::super::SCREENS, 72, 20);
    assert_eq!(full.enter(&child), kept.enter(&child), "切进子会话");
    // 子会话还在跑：推来的一截截到（`ViewPush`），每四条一起上翻一屏，翻上去以后视口不动。
    let pushes = replayed(&extra);
    let (head, tail) = pushes.split_at(pushes.len() / 2);
    for ui in [&mut full, &mut kept] {
        if let Some((_, events)) = &mut ui.child {
            events.extend_from_slice(&extra);
        }
    }
    for (k, push) in head.iter().enumerate() {
        for ui in [&mut full, &mut kept] {
            ui.app.core(Update::ViewPush {
                session: CHILD.to_string(),
                push: push.clone(),
            });
        }
        // 回合进行中转圈那一行和运行状态行（转到第几格看时刻、词随机）不比。
        assert_eq!(
            steady(full.settle()),
            steady(kept.settle()),
            "推来第 {k} 条"
        );
        if k % 4 == 3 {
            let page = KeyCode::PageUp;
            let (a, b) = (steady(full.key(page)), steady(kept.key(page)));
            assert_eq!(a, b, "推来第 {k} 条时上翻");
        }
    }
    assert!(full.app.screen.view.top.is_some(), "翻上去了");
    let mut down = 0;
    while full.app.screen.view.top.is_some() {
        down += 1;
        let page = KeyCode::PageDown;
        let (a, b) = (steady(full.key(page)), steady(kept.key(page)));
        assert_eq!(a, b, "子会话还在跑：下翻第 {down} 下");
        assert!(down < 4000, "翻不回底");
    }
    // 回到底：新出的字照旧跟着。
    for (k, push) in tail.iter().enumerate() {
        for ui in [&mut full, &mut kept] {
            ui.app.core(Update::ViewPush {
                session: CHILD.to_string(),
                push: push.clone(),
            });
        }
        assert_eq!(
            steady(full.settle()),
            steady(kept.settle()),
            "回底以后推来第 {k} 条"
        );
    }
    // 回答长过视口的，替人停在它的开头（「正文」第 1 条），两边一样。
    assert_eq!(full.settle(), kept.settle(), "这一轮完了，整屏一样");
    let last = kept.draw().join("\n");
    assert!(
        last.contains("最后一轮回答的第 0 段"),
        "新出的字跟着画出来了：{last}"
    );
}
