//! 改过的文件在只留几屏时（`transcript/changed.rs`、`app/files_page.rs`，照 Miyu 终端 `app/tests/files_page.rs`）：侧边栏那一段
//! 和全留时列的一样（放掉的段里的改动照样算）；点一行开整屏的「文件修改」那一页，放掉的段里的差异照日志读来，读完以后和全留的
//! 逐格一样；`↑` `↓` 换文件、滚动、点列表、`Esc` 回会话；没改过文件的 `/files` 说一句。

use crossterm::event::{KeyCode, MouseButton, MouseEvent, MouseEventKind};
use lm_kernel::kernel::event::Event;
use serde_json::{Value, json};

use super::{Ui, at, next, session};

/// 一份 `turns` 轮的会话：三轮一组，头一轮编辑 `/w/src/a.rs`（加 2 行删 1 行），第二轮写 `/w/notes/b.md`（3 行），第三轮
/// 跑一条命令、再编辑 `/w/src/c.rs` 没成（不算）。每轮回答几段，够放掉。
pub(super) fn edited(turns: u64) -> Vec<Event> {
    let mut out: Vec<Value> = vec![json!({
        "seq": 1, "at": at(1), "kind": "session.created", "by": {"kind": "person"}, "cause": "cmd-0",
        "body": {"policy": "sha256:94410d4462d213ada41b8cca21af0c1693277d5a4baf3ede3a94b12144f6698d", "cwd": "/tmp"}
    })];
    let mut seq = 1;
    for n in 0..turns {
        let cause = format!("cmd-{n}");
        let said = next(&mut seq);
        out.push(json!({
            "seq": said, "at": at(said), "kind": "message.user", "by": {"kind": "person"}, "cause": cause,
            "body": {"blocks": [{"type": "text", "text": format!("第 {n} 轮：改一下代码")}]}
        }));
        let started = next(&mut seq);
        out.push(json!({
            "seq": started, "at": at(started), "kind": "turn.started", "turn": started, "by": {"kind": "kernel"},
            "cause": cause, "body": {"trigger": said, "cwd": "/tmp"}
        }));
        let (name, args, status) = match n % 3 {
            0 => (
                "edit",
                json!({"file_path": "/w/src/a.rs", "edits": [{
                    "old_string": format!("let old_{n} = 0;\n"),
                    "new_string": format!("let new_{n} = 1;\nlet more_{n} = 2;\n"),
                }]}),
                "ok",
            ),
            1 => (
                "write",
                json!({"file_path": "/w/notes/b.md", "content": format!("# 第 {n} 版\n\n正文 {n}\n")}),
                "ok",
            ),
            _ => (
                "edit",
                json!({"file_path": "/w/src/c.rs", "edits": [{"old_string": "x\n", "new_string": "y\n"}]}),
                "error",
            ),
        };
        let reply = next(&mut seq);
        let call = format!("call_{reply}_1");
        out.push(json!({
            "seq": reply, "at": at(reply), "kind": "message.assistant", "turn": started,
            "by": {"kind": "model", "endpoint": "fake", "model": "m1"}, "cause": cause,
            "body": {"blocks": [{"type": "tool_call", "call_id": call, "name": name, "args": args.to_string()}], "seen": started}
        }));
        let called = next(&mut seq);
        out.push(json!({
            "seq": called, "at": at(called), "kind": "model.called", "turn": started, "by": {"kind": "kernel"},
            "cause": cause,
            "body": {"seen": started, "endpoint": "fake", "model": "m1", "messages": 1,
                "usage": {"uncached": 60, "cache_read": 56, "cache_write": 0, "output": 10},
                "first_token_ms": 300, "duration_ms": 5000,
                "blocks": [{"start_ms": 300, "end_ms": 2000}], "result": "ok"}
        }));
        let result = next(&mut seq);
        out.push(json!({
            "seq": result, "at": at(result), "kind": "tool.result", "turn": started,
            "by": {"kind": "tool", "call_id": call}, "cause": cause,
            "body": {"call_id": call, "status": status, "blocks": [{"type": "text", "text": "done"}]}
        }));
        let seen = seq;
        let reply = next(&mut seq);
        let text = (0..=(n * 5) % 9)
            .map(|k| format!("第 {n} 轮回答的第 {k} 段：改好了，照你说的把那一处换成了新的写法。"))
            .collect::<Vec<_>>()
            .join("\n\n");
        out.push(json!({
            "seq": reply, "at": at(reply), "kind": "message.assistant", "turn": started,
            "by": {"kind": "model", "endpoint": "fake", "model": "m1"}, "cause": cause,
            "body": {"blocks": [{"type": "text", "text": text}], "seen": seen}
        }));
        let called = next(&mut seq);
        out.push(json!({
            "seq": called, "at": at(called), "kind": "model.called", "turn": started, "by": {"kind": "kernel"},
            "cause": cause,
            "body": {"seen": seen, "endpoint": "fake", "model": "m1", "messages": 3,
                "usage": {"uncached": 60, "cache_read": 147, "cache_write": 0, "output": 10},
                "first_token_ms": 200, "duration_ms": 3000,
                "blocks": [{"start_ms": 200, "end_ms": 2900}], "result": "ok"}
        }));
        let ended = next(&mut seq);
        out.push(json!({
            "seq": ended, "at": at(ended), "kind": "turn.ended", "turn": started, "by": {"kind": "kernel"},
            "cause": cause, "body": {"reason": "completed"}
        }));
    }
    out.into_iter()
        .map(|event| serde_json::from_value(event).expect("事件的写法"))
        .collect()
}

fn listed(ui: &Ui) -> Vec<(String, u32, u32)> {
    ui.app
        .talk
        .transcript
        .changed()
        .into_iter()
        .map(|f| (f.path.to_string(), f.added, f.removed))
        .collect()
}

/// 按一个键（照主循环交给 `App::handle`：那一页开着时键归它），画稳。
fn press(ui: &mut Ui, code: KeyCode) -> Vec<String> {
    ui.app.handle(crossterm::event::Event::Key(
        crossterm::event::KeyEvent::new(code, crossterm::event::KeyModifiers::NONE),
    ));
    ui.settle()
}

/// 两个界面一起按同一个键，画出来的一样。
fn both(full: &mut Ui, kept: &mut Ui, code: KeyCode, at: &str) {
    let (a, b) = (press(full, code), press(kept, code));
    assert_eq!(a, b, "{at}：全留和只留几屏画出来的不一样");
}

fn click(ui: &mut Ui, column: u16, row: u16) {
    for kind in [
        MouseEventKind::Down(MouseButton::Left),
        MouseEventKind::Up(MouseButton::Left),
    ] {
        ui.app.handle(crossterm::event::Event::Mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers: crossterm::event::KeyModifiers::NONE,
        }));
    }
}

#[test]
fn the_ledger_is_whole_when_only_a_few_screens_are_kept() {
    let events = edited(90);
    let full = Ui::open(&events, usize::MAX, 160, 30);
    let kept = Ui::open(&events, super::super::SCREENS, 160, 30);
    assert!(
        kept.resident() * 5 < full.resident(),
        "只留底下几屏：常驻 {} 条，全留 {} 条",
        kept.resident(),
        full.resident()
    );
    let want = [
        ("/w/src/a.rs".to_string(), 60, 30),
        ("/w/notes/b.md".to_string(), 90, 0),
    ];
    assert_eq!(
        listed(&full),
        want,
        "30 次编辑各 +2 -1，30 次写入各 3 行；没成的不算"
    );
    assert_eq!(listed(&kept), want, "放掉的段里的改动照样记着");
}

#[test]
fn the_sidebar_lists_the_changed_files_between_jumps_and_recent() {
    let events = edited(30);
    let mut full = Ui::open(&events, usize::MAX, 160, 40);
    let mut kept = Ui::open(&events, super::super::SCREENS, 160, 40);
    let (a, b) = (full.draw(), kept.draw());
    assert_eq!(a, b, "全留和只留几屏画出来的一样");
    // 侧边栏那一栏：竖线右边的（竖线后面空一格；左边正文里有宽字，不照列数切）。
    let side: Vec<String> = a
        .iter()
        .map(|row| match row.rsplit_once('│') {
            Some((_, right)) => right.strip_prefix(' ').unwrap_or(right).to_string(),
            None => String::new(),
        })
        .collect();
    let at = |word: &str| {
        side.iter()
            .position(|l| l.trim() == word)
            .unwrap_or_else(|| panic!("侧边栏有「{word}」：\n{}", side.join("\n")))
    };
    let (jumps, files) = (at("跳转消息"), at("文件修改"));
    assert!(jumps < files, "文件修改在跳转消息下面");
    assert!(
        side[files + 1].starts_with("  a.rs /w/src/"),
        "{:?}",
        side[files + 1]
    );
    assert!(
        side[files + 1].ends_with("+20 -10"),
        "{:?}",
        side[files + 1]
    );
    assert!(
        side[files + 2].starts_with("  b.md /w/notes/"),
        "{:?}",
        side[files + 2]
    );
    assert!(side[files + 2].ends_with("+30 -0"), "{:?}", side[files + 2]);
    assert_eq!(side[files + 3].trim(), "", "这一段后面空一行");
    // 能点的是标题下面那两行。
    let area = kept.app.screen.areas.files;
    assert_eq!((usize::from(area.y), area.height), (files + 1, 2));
    assert_eq!(kept.app.screen.files, [0, 1]);
    // 侧边栏里没有「循环」那一段。
    assert!(!side.iter().any(|l| l.trim() == "循环"));
}

#[test]
fn a_session_without_changes_has_no_files_section_and_files_says_so() {
    let events = session(12);
    let mut ui = Ui::open(&events, usize::MAX, 160, 40);
    let screen = ui.draw();
    assert!(
        !screen.iter().any(|l| l.contains("文件修改")),
        "没改过文件的不出这一段"
    );
    for c in "/files".chars() {
        press(&mut ui, KeyCode::Char(c));
    }
    press(&mut ui, KeyCode::Enter);
    assert!(ui.app.boxes.files_page.is_none(), "不开那一页");
    let notice = ui.app.typing.notice.as_ref().map(|n| n.text.clone());
    assert_eq!(notice.as_deref(), Some("这个会话还没有修改过文件"));
}

#[test]
fn clicking_a_file_opens_the_page_and_the_diffs_match_keeping_everything() {
    let events = edited(60);
    let mut full = Ui::open(&events, usize::MAX, 160, 30);
    let mut kept = Ui::open(&events, super::super::SCREENS, 160, 30);
    let before = kept.draw();
    assert_eq!(full.draw(), before);
    for ui in [&mut full, &mut kept] {
        let area = ui.app.screen.areas.files;
        click(ui, area.x + 3, area.y);
        assert!(ui.app.boxes.files_page.is_some(), "点了开那一页");
    }
    // 只留几屏的：较早的改动在放掉的段里，这一帧先写「正在读取」，向引擎要了那一截日志。
    let first = kept.draw();
    assert!(
        first.iter().any(|l| l.contains("正在读取这一处修改…")),
        "还没读来的先写一句：\n{}",
        first.join("\n")
    );
    let (a, b) = (full.settle(), kept.settle());
    assert_eq!(a, b, "读来以后和全留的逐格一样");
    assert!(!b.iter().any(|l| l.contains("正在读取")), "读来了");
    // 左边：路径、合计，空一行，头一处改动；右边完整的列表，选中的是第一个。
    assert!(
        b[0].trim_start().starts_with("/w/src/a.rs  +40 -20"),
        "{:?}",
        b[0]
    );
    assert!(b[0].contains("文件修改 · 2 个文件"), "{:?}", b[0]);
    assert!(b[2].contains("- let old_0 = 0;"), "{:?}", b[2]);
    assert!(b[3].contains("+ let new_0 = 1;"), "{:?}", b[3]);
    assert!(b[4].contains("+ let more_0 = 2;"), "{:?}", b[4]);
    assert!(b[5].contains("────"), "两处改动之间一道细线：{:?}", b[5]);
    assert!(b[1].contains("a.rs /w/src/") && b[2].contains("b.md /w/notes/"));
    let last = b.last().expect("有按键提示");
    assert!(
        last.contains("↑/↓ 切换文件") && last.contains("Esc 返回"),
        "{last:?}"
    );
    // 滚到底：最后一处改动是第 57 轮的。
    both(&mut full, &mut kept, KeyCode::End, "End 滚到底");
    let end = kept.draw();
    assert!(
        end.iter().any(|l| l.contains("+ let more_57 = 2;")),
        "{}",
        end.join("\n")
    );
    both(&mut full, &mut kept, KeyCode::PageUp, "PgUp");
    both(&mut full, &mut kept, KeyCode::Home, "Home 回到头");
    // `↓` 换到第二个文件：写入的整份是加上的，带行号。
    both(&mut full, &mut kept, KeyCode::Down, "换文件");
    let second = kept.draw();
    assert!(
        second[0].trim_start().starts_with("/w/notes/b.md  +60 -0"),
        "{:?}",
        second[0]
    );
    assert!(second[2].contains("1  + # 第 1 版"), "{:?}", second[2]);
    both(&mut full, &mut kept, KeyCode::Down, "到头停");
    assert_eq!(
        kept.app.boxes.files_page.as_ref().map(|p| p.selected),
        Some(1)
    );
    // 点列表里第一行换回去。
    for ui in [&mut full, &mut kept] {
        let list = ui.app.screen.files_page.list;
        click(ui, list.x + 3, list.y);
    }
    assert_eq!(full.settle(), kept.settle(), "点列表换回第一个文件");
    assert_eq!(
        kept.app.boxes.files_page.as_ref().map(|p| p.selected),
        Some(0)
    );
    // `Esc` 回会话：正文照原样，读来的差异扔了、照旧只留几屏。
    both(&mut full, &mut kept, KeyCode::Esc, "Esc 回会话");
    assert!(kept.app.boxes.files_page.is_none());
    assert_eq!(kept.draw(), before, "回来以后正文照原样");
    assert!(kept.resident() * 3 < full.resident(), "照旧只留几屏");
}

#[test]
fn a_narrow_window_puts_the_list_above_the_diff() {
    let events = edited(12);
    let mut ui = Ui::open(&events, usize::MAX, 80, 30);
    for c in "/files".chars() {
        press(&mut ui, KeyCode::Char(c));
    }
    let screen = press(&mut ui, KeyCode::Enter);
    assert!(ui.app.boxes.files_page.is_some());
    assert!(screen[0].contains("文件修改 · 2 个文件"), "{:?}", screen[0]);
    assert!(
        screen[1].contains("a.rs /w/src/") && screen[1].contains("+8 -4"),
        "{:?}",
        screen[1]
    );
    assert!(screen[2].contains("b.md /w/notes/"), "{:?}", screen[2]);
    assert!(
        screen[6].trim_start().starts_with("/w/src/a.rs  +8 -4"),
        "{:?}",
        screen[6]
    );
    assert!(screen[8].contains("- let old_0 = 0;"), "{:?}", screen[8]);
    // `q` 也回会话。
    press(&mut ui, KeyCode::Char('q'));
    assert!(ui.app.boxes.files_page.is_none());
}

#[test]
fn undoing_the_last_turn_takes_its_change_off_the_list() {
    let events = edited(4);
    let mut ui = Ui::open(&events, usize::MAX, 160, 30);
    assert_eq!(
        listed(&ui),
        [
            ("/w/src/a.rs".to_string(), 4, 2),
            ("/w/notes/b.md".to_string(), 3, 0)
        ]
    );
    // 第 3 轮（最后一轮，编辑 a.rs）的回合编号：最后一条 `turn.started`。
    let turn = events
        .iter()
        .rev()
        .find_map(|e| match &e.body {
            lm_kernel::kernel::event::Body::TurnStarted(_) => Some(e.seq.get()),
            _ => None,
        })
        .expect("有一轮");
    ui.app
        .core(crate::core::Update::Push(crate::core::Push::Reverted {
            turns: vec![turn],
            seq: 9_999,
            cause: None,
        }));
    assert_eq!(
        listed(&ui),
        [
            ("/w/src/a.rs".to_string(), 2, 1),
            ("/w/notes/b.md".to_string(), 3, 0)
        ],
        "撤掉的那一轮不算"
    );
}
