//! 后台面板（Miyu 终端 `ui/background.rs` 的测试）：标题、在跑的在前、结束了的暗、展开的那一块、收起结束了的、超时的黄、
//! 选中的一直露着。

use std::time::{Duration, Instant};

use super::{PANEL_ROWS, panel};
use crate::app::JobsView;
use crate::config::Config;
use crate::core::{JobEnd, JobStart};
use crate::jobs::{Jobs, PanelItem};
use crate::theme;

fn start(job: &str) -> JobStart {
    JobStart {
        call_id: String::new(),
        job: job.into(),
        agent: false,
        title: String::new(),
        session: None,
    }
}

fn end(job: &str, reason: &str, exit_code: Option<i32>, ms: u64) -> JobEnd {
    JobEnd {
        job: job.into(),
        reason: reason.into(),
        exit_code,
        signal: None,
        duration_ms: Some(ms),
    }
}

/// 两条后台命令：`npm run build` 在跑，`cargo test` 被停掉了。
fn jobs(t0: Instant) -> Jobs {
    let mut jobs = Jobs::default();
    jobs.start(&start("j1"), "cargo test".into(), t0);
    jobs.start(&start("j2"), "npm run build".into(), t0);
    jobs.end(
        &end("j1", "stopped", None, 4000),
        t0 + Duration::from_secs(4),
    );
    jobs
}

fn view(picked: usize, open: Option<&str>, all: bool) -> JobsView {
    JobsView {
        picked,
        open: open.map(str::to_string),
        all,
        ..JobsView::default()
    }
}

fn text(lines: &[ratatui::text::Line]) -> Vec<String> {
    lines
        .iter()
        .map(|l| l.to_string().trim_end().to_string())
        .collect()
}

#[test]
fn the_panel_lists_running_then_finished_with_the_selected_one_marked() {
    let config = Config::builtin().unwrap();
    let t0 = Instant::now();
    let jobs = jobs(t0);
    let (chrome, lines, map) = panel(
        &jobs,
        &view(0, None, false),
        &config,
        (70, usize::MAX),
        t0 + Duration::from_secs(12),
    );
    let text: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
    // Miyu 2026-09-30 项目主人：三样一个框，标题嵌在上边框、按键提示嵌在下边框，框里不空行。
    let title: String = chrome.title.iter().map(|s| s.content.as_ref()).collect();
    assert_eq!(title, "后台 · 1 个运行中的命令");
    assert_eq!(
        chrome.hint.as_deref(),
        Some("↑/↓ 选择 · Enter 展开 · x 停止 · Esc 关闭")
    );
    assert!(
        text[0].starts_with("❯ npm run build"),
        "在跑的在前、选中的带 ❯：{}",
        text[0]
    );
    assert!(text[0].ends_with("（运行中 12s）"));
    assert!(text[1].starts_with("  cargo test"), "结束了的在后");
    assert!(text[1].ends_with("（已停止）"));
    // 选中的铺强调色的底、字深色，状态贴着右边。
    assert_eq!(lines[0].style, theme::picked_bar());
    assert_eq!(lines[0].width(), 70);
    assert_ne!(lines[1].style, theme::picked_bar(), "没选中的不铺");
    // Miyu `858796fb`：结束了的命令那一截暗，状态照旧。
    let command = |line: &ratatui::text::Line, text: &str| {
        line.spans
            .iter()
            .find(|s| s.content.contains(text))
            .unwrap()
            .style
    };
    assert_eq!(command(&lines[1], "cargo test"), theme::dim(), "结束了的暗");
    assert_eq!(map, [Some(0), Some(1)], "点哪一行点中哪一条");
}

#[test]
fn a_running_command_that_is_not_selected_stays_in_the_normal_color() {
    // Miyu 2026-10-02 项目主人：没选中的、在跑的命令用正常色，不压暗；结束了的暗（Miyu `858796fb`）。
    let config = Config::builtin().unwrap();
    let t0 = Instant::now();
    let jobs = jobs(t0);
    let (_, lines, _) = panel(&jobs, &view(1, None, false), &config, (70, usize::MAX), t0);
    let running = lines[0]
        .spans
        .iter()
        .find(|s| s.content.contains("npm run build"))
        .unwrap();
    assert_eq!(
        running.style,
        ratatui::style::Style::new(),
        "在跑的照正常色"
    );
    assert_eq!(lines[1].style, theme::picked_bar(), "选中的照旧铺底");
}

#[test]
fn an_opened_command_shows_its_state_and_the_last_lines_of_its_output() {
    // 点开写状态（暗）和最后几行输出（照常的颜色，洗掉转义序列），整块铺底色。
    let config = Config::builtin().unwrap();
    let t0 = Instant::now();
    let jobs = jobs(t0);
    let open = |output: Option<Option<String>>| {
        let mut v = view(0, Some("j2"), false);
        v.output = output;
        let later = t0 + Duration::from_secs(5);
        let (_, lines, map) = panel(&jobs, &v, &config, (70, usize::MAX), later);
        let shown = text(&lines);
        (lines, shown, map)
    };
    // 还没读到：状态和完整的命令。
    let (_, shown, _) = open(None);
    assert_eq!(shown[1..5], ["", "  （运行中 5s）", "  npm run build", ""]);
    let rows = PANEL_ROWS;
    let many: String = (1..=rows + 3)
        .map(|i| format!("\x1b[32mline {i}\x1b[0m\n"))
        .collect();
    let (lines, shown, map) = open(Some(Some(many)));
    assert_eq!(shown[1], "", "点开的下面先空一行");
    assert_eq!(shown[2], "  （运行中 5s）");
    assert_eq!(
        lines[2].style.fg,
        theme::dim().fg,
        "状态暗，比收着的那一行亮一档"
    );
    assert_eq!(shown[3], "  npm run build", "完整的命令");
    assert_eq!(shown[4], "", "空一行接输出");
    assert_eq!(shown[5], "  line 4", "只露最后几行");
    assert_eq!(shown[4 + rows], format!("  line {}", rows + 3));
    assert_eq!(lines[5].style.fg, None, "输出照常的颜色");
    assert_eq!(shown[5 + rows], "");
    assert!(shown[6 + rows].starts_with("  cargo test"), "下一条接着");
    assert_eq!(lines[5].style.bg, theme::shade().bg, "铺底色");
    assert_eq!(lines[5].width(), 70, "铺满宽度");
    assert!(
        map[1..6 + rows].iter().all(|m| *m == Some(0)),
        "点开的那一块都算这一条"
    );
    // 一个字都没有、读不到：写「没有输出」。
    assert_eq!(open(Some(Some(String::new()))).1[5], "  没有输出");
    assert_eq!(open(Some(None)).1[5], "  没有输出");
    // 末尾的空行不占「最后几行」的名额（Miyu `trim_end`）。
    let (_, shown, _) = open(Some(Some("only\n\n\n".into())));
    assert_eq!(shown[5..7], ["  only", ""]);
}

#[test]
fn a_long_command_folds_when_opened_and_shows_its_first_line_when_shut() {
    // Miyu `Job::headline`：多行的命令那一行只写第一行接 ` …`；点开的那一块写完整的命令，放不下的折行。
    let config = Config::builtin().unwrap();
    let t0 = Instant::now();
    let mut jobs = Jobs::default();
    jobs.start(&start("j1"), "cd x\nmake all".into(), t0);
    let (_, lines, _) = panel(
        &jobs,
        &view(0, Some("j1"), false),
        &config,
        (40, usize::MAX),
        t0,
    );
    let shown = text(&lines);
    assert!(shown[0].starts_with("❯ cd x …"), "{shown:?}");
    assert_eq!(shown[3..5], ["  cd x", "  make all"], "{shown:?}");
}

#[test]
fn more_than_one_ended_command_folds_into_one_row() {
    // Miyu 2026-09-30 项目主人定：结束了的多于一条时只露最近结束的那条，别的收成一行，选中按 Enter 展开、再按收起。
    let config = Config::builtin().unwrap();
    let t0 = Instant::now();
    let mut jobs = jobs(t0);
    jobs.start(&start("j3"), "ls".into(), t0);
    jobs.end(
        &end("j3", "exited", Some(0), 9000),
        t0 + Duration::from_secs(9),
    );
    let show = |all: bool| {
        let (_, lines, _) = panel(&jobs, &view(0, None, all), &config, (70, usize::MAX), t0);
        text(&lines)
    };
    let folded = show(false);
    assert_eq!(folded.len(), 3);
    assert!(folded[0].starts_with("❯ npm run build"));
    assert!(
        folded[1].starts_with("  ls"),
        "最近结束的那条：{}",
        folded[1]
    );
    assert_eq!(
        folded[2], "  已收起 1 个",
        "Miyu 2026-09-30 项目主人改的写法"
    );
    let all = show(true);
    assert!(all[1].starts_with("  cargo test"), "展开：照开始的先后");
    assert!(all[2].starts_with("  ls"));
    assert_eq!(all[3], "  收起已结束的");
    assert_eq!(
        jobs.panel_items(false),
        [
            PanelItem::Job("j2".into()),
            PanelItem::Job("j3".into()),
            PanelItem::More(1)
        ]
    );
}

#[test]
fn a_command_stopped_for_running_too_long_is_yellow_and_a_failed_one_red() {
    // Miyu `background::state`：超时、输出太大、太勤停掉的黄，失败的红，别的 `faint`。
    let config = Config::builtin().unwrap();
    let t0 = Instant::now();
    let mut jobs = Jobs::default();
    jobs.start(&start("j1"), "sleep 999".into(), t0);
    jobs.start(&start("j2"), "false".into(), t0);
    jobs.end(&end("j1", "timed_out", None, 1000), t0);
    jobs.end(
        &end("j2", "exited", Some(1), 2000),
        t0 + Duration::from_secs(1),
    );
    // 选中最后那一行「收起已结束的」：选中的整行铺底、看不出状态的颜色。
    let (_, lines, _) = panel(&jobs, &view(2, None, true), &config, (70, usize::MAX), t0);
    let state = |line: &ratatui::text::Line| line.spans.last().unwrap().style;
    assert_eq!(state(&lines[0]), theme::warn(), "{:?}", lines[0]);
    assert_eq!(state(&lines[1]), theme::error(), "{:?}", lines[1]);
}

#[test]
fn the_selected_row_stays_in_view_when_the_panel_is_short() {
    // Miyu `panel::fit`：行多过框能放的，选中的那一行一直露着。
    let config = Config::builtin().unwrap();
    let t0 = Instant::now();
    let mut jobs = Jobs::default();
    for i in 0..6 {
        jobs.start(&start(&format!("j{i}")), format!("cmd{i}"), t0);
    }
    let (_, lines, map) = panel(&jobs, &view(5, None, false), &config, (70, 3), t0);
    let shown = text(&lines);
    assert_eq!(shown.len(), 3);
    assert!(shown[2].starts_with("❯ cmd5"), "{shown:?}");
    assert_eq!(map[2], Some(5));
}
