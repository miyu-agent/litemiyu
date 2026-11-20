//! 子代理状态行：占几行、画成什么样、焦点怎么走。只测不发命令的几样（`Enter` 切进去、`x` 停掉要引擎线程，伪终端测试
//! `tests/pty/child.rs` 守着）：单元测试里发命令会起真的引擎线程。

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;

use crate::app::App;
use crate::config::Config;
use crate::core::{Core, JobStart};
use crate::focus::{Button, Focus};

fn app() -> App {
    let mut app = App::new(
        Config::builtin().expect("出厂的配置读得懂"),
        Core::new(|_| true),
    );
    app.talk.transcript.user("派两个子代理".into(), Vec::new());
    app
}

fn agent(app: &mut App, job: &str, title: &str) {
    let start = JobStart {
        call_id: format!("call_{job}"),
        job: job.into(),
        agent: true,
        title: title.into(),
        session: Some(format!("s-{job}")),
        fork: false,
    };
    app.talk
        .jobs
        .start(&start, title.into(), std::time::Instant::now());
}

/// 照 `width` 列画一帧，交回每一行的字（去掉行尾空白）。
fn draw(app: &mut App, width: u16) -> Vec<String> {
    let mut buffer = Buffer::empty(Rect::new(0, 0, width, 20));
    crate::ui::draw(&mut crate::ui::Frame::new(&mut buffer), app);
    (0..20)
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
        .collect()
}

/// 派一条后台命令（不是子代理）：底栏有了后台按钮。
fn command(app: &mut App, job: &str, title: &str) {
    let start = JobStart {
        call_id: format!("call_{job}"),
        job: job.into(),
        agent: false,
        title: title.into(),
        session: None,
        fork: false,
    };
    app.talk
        .jobs
        .start(&start, title.into(), std::time::Instant::now());
}

fn key(app: &mut App, code: KeyCode) {
    app.key(KeyEvent::new(code, KeyModifiers::NONE));
}

#[test]
fn no_agents_take_no_rows_and_down_stays_in_the_input() {
    let mut app = app();
    let rows = draw(&mut app, 60);
    let bottom = rows
        .iter()
        .rposition(|r| r.contains('╰'))
        .expect("有输入框");
    assert_eq!(bottom, 18, "框下面只有底栏那一行：{rows:#?}");
    assert!(!rows.iter().any(|r| r.contains("主会话")), "{rows:#?}");
    key(&mut app, KeyCode::Down);
    assert_eq!(app.typing.focus, Focus::Input, "没有子代理：焦点停不过去");
    assert_eq!(app.screen.areas.agent_rows, Rect::default());
}

#[test]
fn a_narrow_window_lists_the_main_session_and_each_running_agent_under_the_footer() {
    let mut app = app();
    agent(&mut app, "j1", "查旧版缓存");
    agent(&mut app, "j2", "找主题映射");
    let rows = draw(&mut app, 60);
    let main = rows
        .iter()
        .position(|r| r.contains("● 主会话"))
        .expect("主会话那一行");
    assert_eq!(rows[main - 1], "", "上面空一行：{rows:#?}");
    assert!(
        rows[main - 3].contains('╰'),
        "空行上面是底栏、再上面是框：{rows:#?}"
    );
    assert!(rows[main + 1].contains("○ 查旧版缓存"), "{rows:#?}");
    assert!(rows[main + 2].contains("○ 找主题映射"), "{rows:#?}");
    assert_eq!(main + 2, 19, "列到最底下一行");
    assert!(rows[main + 1].ends_with("0s"), "用时靠右：{rows:#?}");
    assert_eq!(
        app.screen.areas.agent_rows.height, 3,
        "主会话那一行起都能点"
    );
    assert_eq!(
        app.screen.agent_rows,
        [None, Some("s-j1".into()), Some("s-j2".into())]
    );
}

#[test]
fn down_moves_the_focus_through_the_rows_and_esc_or_typing_brings_it_back() {
    let mut app = app();
    agent(&mut app, "j1", "查旧版缓存");
    agent(&mut app, "j2", "找主题映射");
    key(&mut app, KeyCode::Down);
    assert_eq!(
        app.typing.focus,
        Focus::Agent(0),
        "输入框空着：到主会话那一行"
    );
    let rows = draw(&mut app, 60);
    assert!(
        rows.iter()
            .any(|r| r.starts_with("❯ ● 主会话") || r.contains(" ❯ ● 主会话")),
        "焦点那一行前面一个箭头：{rows:#?}"
    );
    for _ in 0..5 {
        key(&mut app, KeyCode::Down);
    }
    assert_eq!(app.typing.focus, Focus::Agent(2), "到最后一行就停");
    key(&mut app, KeyCode::Up);
    assert_eq!(app.typing.focus, Focus::Agent(1));
    key(&mut app, KeyCode::Esc);
    assert_eq!(app.typing.focus, Focus::Input, "Esc 回输入框");
    key(&mut app, KeyCode::Down);
    key(&mut app, KeyCode::Up);
    assert_eq!(app.typing.focus, Focus::Input, "第一行往上回输入框");
    key(&mut app, KeyCode::Down);
    key(&mut app, KeyCode::Char('a'));
    assert_eq!(app.typing.focus, Focus::Input, "别的键：焦点回输入框");
    assert_eq!(app.typing.input.editor.text(), "a", "字照打进去");
}

#[test]
fn rows_past_the_limit_fold_into_one_and_a_finished_agent_leaves_the_list() {
    let mut app = app();
    let most = app.config.layout.agent_rows;
    for i in 0..=most {
        agent(&mut app, &format!("j{i}"), &format!("活{i}"));
    }
    let rows = draw(&mut app, 60);
    assert!(rows[19].contains("还有 1 个"), "{rows:#?}");
    let end = crate::core::ChildEnd {
        job: "j0".into(),
        session: "s-j0".into(),
        reason: "done".into(),
        text: String::new(),
    };
    app.talk.jobs.report(&end, std::time::Instant::now());
    let rows = draw(&mut app, 60);
    assert!(!rows.iter().any(|r| r.contains("还有")), "{rows:#?}");
    assert!(
        !rows.iter().any(|r| r.contains("活0")),
        "报完了不列：{rows:#?}"
    );
}

/// Miyu `ui/agents.rs` 的 `the_session_being_viewed_is_the_solid_dot_and_nested_ones_are_counted`：子代理自己又派了的，名字后面
/// 写「（+N）」，连着它对齐名字那一列；报回来了的不再数。
#[test]
fn an_agent_that_sent_its_own_agents_counts_them_after_its_name() {
    let mut app = app();
    agent(&mut app, "j1", "查旧版缓存");
    agent(&mut app, "j2", "找主题映射");
    let child = |job: &str| crate::core::Update::Child {
        session: "s-j2".into(),
        push: crate::core::Push::JobStarted(JobStart {
            call_id: format!("call_{job}"),
            job: job.into(),
            agent: true,
            title: "再分头".into(),
            session: Some(format!("s-{job}")),
            fork: false,
        }),
    };
    app.core(child("j2.1"));
    app.core(child("j2.2"));
    let rows = draw(&mut app, 60);
    let second = rows
        .iter()
        .find(|r| r.contains("找主题映射"))
        .expect("第二个子代理");
    assert!(second.contains("○ 找主题映射（+2）"), "{rows:#?}");
    let first = rows
        .iter()
        .find(|r| r.contains("查旧版缓存"))
        .expect("第一个子代理");
    let clock = |r: &str| unicode_width::UnicodeWidthStr::width(&r[..r.rfind("0s").unwrap()]);
    assert_eq!(clock(first), clock(second), "用时那一列对齐：{rows:#?}");
    let reported = crate::core::Update::Child {
        session: "s-j2".into(),
        push: crate::core::Push::ChildEnded(crate::core::ChildEnd {
            job: "j2.1".into(),
            session: "s-j2.1".into(),
            reason: "done".into(),
            text: String::new(),
        }),
    };
    app.core(reported);
    let rows = draw(&mut app, 60);
    assert!(
        rows.iter().any(|r| r.contains("○ 找主题映射（+1）")),
        "{rows:#?}"
    );
}

/// Miyu `ui/agents.rs` 的 `style`：指针停的那一行亮一档（`theme::hover`），别的照旧暗。
#[test]
fn the_row_under_the_pointer_lights_up() {
    let mut app = app();
    agent(&mut app, "j1", "查旧版缓存");
    agent(&mut app, "j2", "找主题映射");
    draw(&mut app, 60);
    let area = app.screen.areas.agent_rows;
    // 能点的第 0 行是主会话，第 1 行是第一个子代理。
    let (x, y) = (area.x + 4, area.y + 1);
    app.screen.pointer = Some(ratatui::layout::Position::new(x, y));
    let mut buffer = Buffer::empty(Rect::new(0, 0, 60, 20));
    crate::ui::draw(&mut crate::ui::Frame::new(&mut buffer), &mut app);
    assert_eq!(
        buffer[(x, y)].fg,
        crate::theme::hover().fg.unwrap_or_default()
    );
    assert_eq!(
        buffer[(x, y + 1)].fg,
        crate::theme::dim().fg.unwrap_or_default(),
        "别的照旧暗"
    );
}

/// Miyu `focus.rs`、`jobs_key`（「后台命令、子代理和侧边栏」第 2 条）：有后台命令时空输入框按 `↓` 先停在底栏的后台按钮上
/// （反色），`Enter` 才开面板；按钮再 `↓` 到子代理状态行，状态行第一行 `↑` 回按钮；只有一个按钮 `←` `→` 不动；面板 `Esc` 关了
/// 焦点还在按钮上，再 `Esc` 回输入框。
#[test]
fn down_stops_on_the_jobs_button_first_and_enter_opens_the_panel() {
    let mut app = app();
    command(&mut app, "c1", "sleep 9");
    agent(&mut app, "j1", "查旧版缓存");
    key(&mut app, KeyCode::Down);
    assert_eq!(app.typing.focus, Focus::Footer(Button::Background));
    assert_eq!(app.boxes.panel, None, "只停在按钮上，不开面板");
    let mut buffer = Buffer::empty(Rect::new(0, 0, 80, 20));
    crate::ui::draw(&mut crate::ui::Frame::new(&mut buffer), &mut app);
    let button = app.screen.areas.jobs_button;
    assert!(button.width > 0, "按钮画了");
    let cell = &buffer[(button.x, button.y)];
    assert!(
        cell.modifier.contains(ratatui::style::Modifier::REVERSED),
        "焦点在按钮上反色（theme::selected）：{cell:?}"
    );
    key(&mut app, KeyCode::Left);
    key(&mut app, KeyCode::Right);
    assert_eq!(
        app.typing.focus,
        Focus::Footer(Button::Background),
        "只有一个按钮"
    );
    key(&mut app, KeyCode::Down);
    assert_eq!(app.typing.focus, Focus::Agent(0), "按钮往下到状态行");
    key(&mut app, KeyCode::Up);
    assert_eq!(
        app.typing.focus,
        Focus::Footer(Button::Background),
        "第一行往上回按钮"
    );
    key(&mut app, KeyCode::Enter);
    assert_eq!(
        app.boxes.panel,
        Some(crate::app::Panel::Jobs),
        "Enter 开面板"
    );
    key(&mut app, KeyCode::Esc);
    assert_eq!(app.boxes.panel, None, "Esc 关面板");
    assert_eq!(
        app.typing.focus,
        Focus::Footer(Button::Background),
        "焦点还在按钮上"
    );
    key(&mut app, KeyCode::Esc);
    assert_eq!(app.typing.focus, Focus::Input, "再 Esc 回输入框");
}

/// fork 出来的子代理名字前面接分叉记号（LM-18，照 Miyu 终端 `ui/agents.rs`：2026-10-26 Miyu 项目主人），别的照旧；名字那一列
/// 连着记号对齐。
#[test]
fn a_forked_agent_has_the_fork_mark_before_its_name() {
    let mut app = app();
    let start = JobStart {
        call_id: "call_j1".into(),
        job: "j1".into(),
        agent: true,
        title: "分头查".into(),
        session: Some("s-j1".into()),
        fork: true,
    };
    app.talk
        .jobs
        .start(&start, "分头查".into(), std::time::Instant::now());
    agent(&mut app, "j2", "只看交代");
    let rows = draw(&mut app, 60);
    let fork = app.config.icons.fork.clone();
    assert!(!fork.is_empty(), "出厂的这一套有分叉记号");
    assert!(
        rows.iter().any(|r| r.contains(&format!("○ {fork}分头查"))),
        "{rows:#?}"
    );
    assert!(
        rows.iter()
            .any(|r| r.contains("○ 只看交代") && !r.contains(fork.trim_end())),
        "{rows:#?}"
    );
}
