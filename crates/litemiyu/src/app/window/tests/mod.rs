//! 只留几屏和全留逐格对照（第十趟「做法」第 10 条）：同一份会话（日志里的事件），一个界面 `keep_screens` 取很大（全留），
//! 一个照默认只留几屏；打开、翻到顶、一屏屏翻回来、到底、变宽变窄、翻上去时来了新的一轮、选区跨过放掉又读回的行，
//! 同样的位置画出来逐格一样。读回不经引擎线程：测试照引擎的做法（`core/engine/readback.rs`）把要的那一截事件补发成推送交回。

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use lm_kernel::kernel::event::Event;
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::app::App;
use crate::config::Config;
use crate::core::push::replay::Replay;
use crate::core::{Command, Core, Push, Update};

const SESSION: &str = "01a1d25d-b0db-733a-a362-186ba586969e";

/// 第 `seq` 条事件的时刻：一条隔七秒。
fn at(seq: u64) -> String {
    let secs = seq * 7;
    format!(
        "2026-11-12T{:02}:{:02}:{:02}.000Z",
        8 + secs / 3600,
        secs / 60 % 60,
        secs % 60
    )
}

/// 下一条的序号。
fn next(seq: &mut u64) -> u64 {
    *seq += 1;
    *seq
}

/// 一份 `turns` 轮的会话：每轮长短不一，有的先想、调一次工具，回答几段到十几段。交回日志里的事件。
pub(super) fn session(turns: u64) -> Vec<Event> {
    let mut lines = vec![format!(
        r#"{{"seq":1,"at":"{}","kind":"session.created","by":{{"kind":"person"}},"cause":"cmd-0","body":{{"policy":"sha256:94410d4462d213ada41b8cca21af0c1693277d5a4baf3ede3a94b12144f6698d","cwd":"/tmp"}}}}"#,
        at(1)
    )];
    let mut seq = 1;
    for n in 0..turns {
        let said = next(&mut seq);
        let ask = (0..=n % 3)
            .map(|k| format!("第 {n} 轮的第 {k} 行问话，长一点好让它折行：照构建日志看链接错误是哪个库的顺序。"))
            .collect::<Vec<_>>()
            .join("\\n");
        lines.push(format!(
            r#"{{"seq":{said},"at":"{}","kind":"message.user","by":{{"kind":"person"}},"cause":"cmd-{n}","body":{{"blocks":[{{"type":"text","text":"{ask}"}}]}}}}"#,
            at(said)
        ));
        let started = next(&mut seq);
        lines.push(format!(
            r#"{{"seq":{started},"at":"{}","kind":"turn.started","turn":{started},"by":{{"kind":"kernel"}},"cause":"cmd-{n}","body":{{"trigger":{said},"cwd":"/tmp"}}}}"#,
            at(started)
        ));
        if n % 2 == 0 {
            let reply = next(&mut seq);
            lines.push(format!(
                r#"{{"seq":{reply},"at":"{}","kind":"message.assistant","turn":{started},"by":{{"kind":"model","endpoint":"fake","model":"m1"}},"cause":"cmd-{n}","body":{{"blocks":[{{"type":"reasoning","text":"想一想第 {n} 轮"}},{{"type":"tool_call","call_id":"call_{reply}_1","name":"echo","args":"{{\"n\":{n}}}"}}],"seen":{started}}}}}"#,
                at(reply)
            ));
            let called = next(&mut seq);
            lines.push(format!(
                r#"{{"seq":{called},"at":"{}","kind":"model.called","turn":{started},"by":{{"kind":"kernel"}},"cause":"cmd-{n}","body":{{"seen":{started},"endpoint":"fake","model":"m1","messages":1,"usage":{{"uncached":60,"cache_read":56,"cache_write":0,"output":10}},"first_token_ms":300,"duration_ms":5000,"blocks":[{{"start_ms":300,"end_ms":2{n:03}}},{{"start_ms":3000,"end_ms":4900}}],"result":"ok"}}}}"#,
                at(called)
            ));
            let result = next(&mut seq);
            lines.push(format!(
                r#"{{"seq":{result},"at":"{}","kind":"tool.result","turn":{started},"by":{{"kind":"tool","call_id":"call_{reply}_1"}},"cause":"cmd-{n}","body":{{"call_id":"call_{reply}_1","status":"ok","blocks":[{{"type":"text","text":"echo {n}"}}]}}}}"#,
                at(result)
            ));
        }
        let seen = seq;
        let reply = next(&mut seq);
        let text = (0..=(n * 7) % 13)
            .map(|k| format!("第 {n} 轮回答的第 {k} 段：把第二个静态库挪到用它的那个后面再链接，就不会丢符号。"))
            .collect::<Vec<_>>()
            .join("\\n\\n");
        lines.push(format!(
            r#"{{"seq":{reply},"at":"{}","kind":"message.assistant","turn":{started},"by":{{"kind":"model","endpoint":"fake","model":"m1"}},"cause":"cmd-{n}","body":{{"blocks":[{{"type":"text","text":"{text}"}}],"seen":{seen}}}}}"#,
            at(reply)
        ));
        let called = next(&mut seq);
        lines.push(format!(
            r#"{{"seq":{called},"at":"{}","kind":"model.called","turn":{started},"by":{{"kind":"kernel"}},"cause":"cmd-{n}","body":{{"seen":{seen},"endpoint":"fake","model":"m1","messages":3,"usage":{{"uncached":60,"cache_read":147,"cache_write":0,"output":10}},"first_token_ms":200,"duration_ms":3000,"blocks":[{{"start_ms":200,"end_ms":2900}}],"result":"ok"}}}}"#,
            at(called)
        ));
        let ended = next(&mut seq);
        lines.push(format!(
            r#"{{"seq":{ended},"at":"{}","kind":"turn.ended","turn":{started},"by":{{"kind":"kernel"}},"cause":"cmd-{n}","body":{{"reason":"completed"}}}}"#,
            at(ended)
        ));
    }
    lines
        .iter()
        .map(|line| serde_json::from_str(line).expect("事件的写法"))
        .collect()
}

/// 照补发读成推送（同引擎）。
fn replayed(events: &[Event]) -> Vec<Push> {
    let mut replay = Replay::default();
    let mut out: Vec<Push> = events.iter().flat_map(|e| replay.read(e.clone())).collect();
    out.extend(replay.finish());
    out
}

/// 一个界面：只留 `keep` 屏（`usize::MAX` 是全留），`width`×`height` 的窗口。
pub(super) struct Ui {
    pub app: App,
    events: Vec<Event>,
    width: u16,
    height: u16,
    /// 切进去看着的子会话：编号、它日志里的事件（`child.rs`）。
    child: Option<(String, Vec<Event>)>,
}

impl Ui {
    /// 打开这份会话：同打开旧会话，补发的推送一条条交给界面，再画稳。
    pub fn open(events: &[Event], keep: usize, width: u16, height: u16) -> Ui {
        let mut app = App::new(
            Config::builtin().expect("出厂的配置读得懂"),
            Core::new(|_| true),
        );
        app.media.keep_screens = keep;
        // 整份重排一帧排完：照时间分帧的话全留的那一份排几帧、视口跟着找几次全看机器快慢，没法逐格比。
        app.config.layout.relayout_budget_ms = 60_000;
        let mut ui = Ui {
            app,
            events: events.to_vec(),
            width,
            height,
            child: None,
        };
        ui.draw();
        ui.app.core(Update::Ready(SESSION.to_string()));
        for push in replayed(events) {
            ui.app.core(Update::Push(push));
        }
        ui.settle();
        ui
    }

    /// 画一帧，交回正文那几行的字（去掉行尾空白；输入框里轮换的小贴士随机，不比）；画完照主循环放掉、要读回。
    pub fn draw(&mut self) -> Vec<String> {
        let (width, height) = (self.width, self.height);
        let mut buffer = Buffer::empty(Rect::new(0, 0, width, height));
        crate::ui::draw(&mut crate::ui::Frame::new(&mut buffer), &mut self.app);
        crate::ui::wide::tidy(&mut buffer);
        self.app.keep_window();
        (0..height)
            .map(|y| {
                let mut row = String::new();
                let mut x = 0;
                while x < width {
                    let symbol = buffer[(x, y)].symbol();
                    row.push_str(symbol);
                    let wide = unicode_width::UnicodeWidthStr::width(symbol);
                    x += u16::try_from(wide.max(1)).unwrap_or(1);
                }
                row.trim_end().to_string()
            })
            .take_while(|row| !row.trim_start().starts_with('╭'))
            .collect()
    }

    /// 照引擎的做法交回要的读回（日志里那一截事件补发成推送）、日志里你说的话（补发办进草稿）。
    fn serve(&mut self) -> bool {
        let wanted = std::mem::take(&mut self.app.talk.wanted);
        let served = !wanted.is_empty();
        for command in wanted {
            let Command::ReadBack {
                session,
                piece,
                count,
                after,
                upto,
            } = command
            else {
                self.spoken(command);
                continue;
            };
            let pushes = Ok(replayed(&self.slice(&session, after, upto)));
            self.app.core(Update::ReadBack {
                session,
                piece,
                count,
                pushes,
            });
        }
        served
    }

    /// 会话 `session`（主会话、看着的子会话）日志里序号在 `after` 之后、`upto` 为止的事件。
    fn slice(&self, session: &str, after: u64, upto: u64) -> Vec<Event> {
        let events = match &self.child {
            Some((id, events)) if id == session => events,
            _ => &self.events,
        };
        events
            .iter()
            .filter(|e| e.seq.get() > after && e.seq.get() <= upto)
            .cloned()
            .collect()
    }

    /// 照引擎的做法交回 `/jump` 要的日志里你说的话（`Command::Spoken`，界面发给引擎的那一条）。
    fn spoken(&mut self, command: Command) {
        let Command::Spoken {
            session,
            upto,
            texts,
        } = command
        else {
            panic!("要的不是你说的话：{command:?}");
        };
        let mut gather = crate::transcript::Gather::new(texts);
        for push in replayed(&self.slice(&session, 0, upto)) {
            gather.push(push);
        }
        self.app.core(Update::Spoken {
            session,
            said: Ok(gather.finish()),
        });
    }

    /// 一帧帧画、交读回，直到不再要读回、排版不再分帧：交回最后一帧。
    pub fn settle(&mut self) -> Vec<String> {
        let mut screen = self.draw();
        for _ in 0..200 {
            let stale = self.app.media.row_cache.borrow().stale > 0;
            if !self.serve() && !stale {
                return screen;
            }
            screen = self.draw();
        }
        panic!("两百帧还没画稳");
    }

    /// 按一个键，画稳。
    pub fn key(&mut self, code: KeyCode) -> Vec<String> {
        self.app.key(KeyEvent::new(code, KeyModifiers::NONE));
        self.settle()
    }

    /// 换窗口大小，画稳。
    pub fn resize(&mut self, width: u16, height: u16) -> Vec<String> {
        self.width = width;
        self.height = height;
        self.settle()
    }

    /// 视口第一行是全部行里的第几行。
    pub fn first(&self) -> usize {
        self.app.screen.view.first
    }

    /// 常驻的条目有几条。
    pub fn resident(&self) -> usize {
        self.app.talk.transcript.entries.len()
    }
}

/// 两个界面一起按同一个键，画出来的一样。行号可以不一样：上面放掉的段是估的行数，视口跟着内容走。
fn both(full: &mut Ui, kept: &mut Ui, code: KeyCode, at: &str) {
    let a = full.key(code);
    let b = kept.key(code);
    assert_eq!(a, b, "{at}：全留和只留几屏画出来的不一样");
}

#[test]
fn opening_a_long_session_keeps_only_the_bottom_screens_and_draws_the_same() {
    let events = session(160);
    let mut full = Ui::open(&events, usize::MAX, 72, 20);
    let mut kept = Ui::open(&events, super::SCREENS, 72, 20);
    assert_eq!(full.draw(), kept.draw(), "打开以后底下一样");
    assert!(
        kept.resident() * 10 < full.resident(),
        "只留底下几屏：常驻 {} 条，全留 {} 条",
        kept.resident(),
        full.resident()
    );
}

#[test]
fn paging_to_the_top_and_back_matches_screen_by_screen() {
    let events = session(120);
    let mut full = Ui::open(&events, usize::MAX, 72, 20);
    let mut kept = Ui::open(&events, super::SCREENS, 72, 20);
    // 往上一屏屏翻到顶：每一屏都一样（读回的段排出来的行和全留的一样，空档估的行数换成真的以后视口跟着内容走）。
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
    assert_eq!(kept.first(), 0, "只留几屏的也到了顶（上面没有空档了）");
    let top = kept.resident();
    // 再一屏屏翻回底：上面的放掉，画出来照旧一样。
    let mut down = 0;
    while full.app.screen.view.top.is_some() {
        down += 1;
        both(
            &mut full,
            &mut kept,
            KeyCode::PageDown,
            &format!("下翻第 {down} 下"),
        );
        assert!(down < 4000, "翻不回底");
    }
    assert!(
        kept.resident() * 4 < full.resident() && kept.resident() < top + full.resident(),
        "回到底以后上面的放掉了：常驻 {} 条，全留 {} 条",
        kept.resident(),
        full.resident()
    );
}

#[test]
fn a_wider_or_narrower_window_matches_after_paging_up() {
    let events = session(90);
    let mut full = Ui::open(&events, usize::MAX, 72, 20);
    let mut kept = Ui::open(&events, super::SCREENS, 72, 20);
    for k in 0..12 {
        both(
            &mut full,
            &mut kept,
            KeyCode::PageUp,
            &format!("上翻第 {k} 下"),
        );
    }
    for (width, height) in [(50, 24), (110, 16), (72, 20)] {
        let a = full.resize(width, height);
        let b = kept.resize(width, height);
        assert_eq!(a, b, "换成 {width}×{height}");
        both(
            &mut full,
            &mut kept,
            KeyCode::PageUp,
            &format!("{width}×{height} 再上翻"),
        );
    }
    while full.app.screen.view.top.is_some() {
        both(&mut full, &mut kept, KeyCode::PageDown, "翻回底");
    }
}

#[test]
fn a_new_turn_while_paged_up_matches_and_both_follow_it_back_down() {
    let all = session(82);
    let events = session(80);
    let mut full = Ui::open(&events, usize::MAX, 72, 20);
    let mut kept = Ui::open(&events, super::SCREENS, 72, 20);
    for k in 0..15 {
        both(
            &mut full,
            &mut kept,
            KeyCode::PageUp,
            &format!("上翻第 {k} 下"),
        );
    }
    // 翻上去时又来了两轮（日志接着往后写，推送一条条到）。
    let extra = &all[events.len()..];
    for ui in [&mut full, &mut kept] {
        ui.events.extend_from_slice(extra);
        for push in replayed(extra) {
            ui.app.core(Update::Push(push));
        }
    }
    assert_eq!(full.settle(), kept.settle(), "来了新的一轮，视口不动");
    for k in 0..6 {
        both(
            &mut full,
            &mut kept,
            KeyCode::PageUp,
            &format!("再上翻第 {k} 下"),
        );
    }
    while full.app.screen.view.top.is_some() {
        both(&mut full, &mut kept, KeyCode::PageDown, "翻回底");
    }
}

#[test]
fn a_selection_across_rows_shed_and_read_back_copies_the_same_text() {
    let events = session(80);
    let mut full = Ui::open(&events, usize::MAX, 72, 20);
    let mut kept = Ui::open(&events, super::SCREENS, 72, 20);
    for k in 0..25 {
        both(
            &mut full,
            &mut kept,
            KeyCode::PageUp,
            &format!("上翻第 {k} 下"),
        );
    }
    // 从视口顶上选到再往下三屏（拖着选、滚过去的样子）。
    for ui in [&mut full, &mut kept] {
        let first = ui.first();
        ui.app.screen.view.select = Some(((first, 0), (first + 60, 40)));
    }
    assert_eq!(
        full.app.screen.view.selected_text(),
        kept.app.screen.view.selected_text(),
        "选区复制出来的字"
    );
    while full.app.screen.view.top.is_some() {
        both(&mut full, &mut kept, KeyCode::PageDown, "带着选区翻回底");
    }
    assert_eq!(
        full.app.screen.view.selected_text(),
        kept.app.screen.view.selected_text(),
        "翻回底以后选区的字照旧（选区碰到的段不放）"
    );
}

mod child;
mod jump;
mod late;
mod live;
mod pixel;
mod redo;
mod undo;
mod unread;
