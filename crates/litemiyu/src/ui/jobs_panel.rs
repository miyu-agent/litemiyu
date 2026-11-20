//! 后台面板（Miyu 终端 `ui/background.rs`，蓝图 `tui.md`「后台命令、子代理和侧边栏」第 3 条，照 Claude Code）：输入框上面
//! 那个框，上边框写「后台 · N 个运行中的命令」，一条后台命令一行，状态贴着右边；在跑的在前，结束了的多于一条时只露最近结束的
//! 那条、别的收成一行。选中的加粗，没选中在跑的照常，结束了的暗。点开的一条照时间线「点开一步」的样子在它下面铺底色写状态、
//! 完整的命令、最后几行输出。

use std::time::Instant;

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use unicode_width::UnicodeWidthStr;

use super::panel::{self, Chrome};
use super::rows::clip;
use crate::app::{JobsView, Seen};
use crate::config::Config;
use crate::jobs::clean::clean;
use crate::jobs::{Job, Jobs, PanelItem};
use crate::{meter, theme};

/// 展开的输出最多露几行（Miyu `layout.json` 的 `output.panel_rows`）。
const PANEL_ROWS: usize = 12;

/// 面板的框和行，最多 `max` 行；每行是第几条（点中用，展开的那几行也算那一条）。
pub fn lines(
    seen: &Seen,
    width: u16,
    max: usize,
) -> (Chrome, Vec<Line<'static>>, Vec<Option<usize>>) {
    let (chrome, lines, map) = panel(
        &seen.talk.jobs,
        &seen.boxes.jobs,
        seen.config,
        (width, max),
        Instant::now(),
    );
    (chrome.vim(seen.config.layout.vim), lines, map)
}

/// 照任务表和面板的样子排：框（上边框写标题和在跑几条，下边框写按键提示）、框里的行、每一行是第几条。选中的那一行一直
/// 露着（Miyu `panel::fit`）。
fn panel(
    jobs: &Jobs,
    view: &JobsView,
    config: &Config,
    (width, max): (u16, usize),
    now: Instant,
) -> (Chrome, Vec<Line<'static>>, Vec<Option<usize>>) {
    let words = &config.text().jobs;
    let count = jobs.running_commands().to_string();
    let meta = vec![
        Span::styled(crate::sheet::sep(), theme::dim()),
        Span::styled(words.active.replace("{count}", &count), theme::dim()),
    ];
    let chrome = Chrome::new(&words.title, meta).hint(&words.hint);
    let (lines, map) = background(jobs, view, config, width, now);
    let rows = map.into_iter().zip(lines).collect();
    let (map, lines) = panel::fit(rows, Some(view.picked), max).into_iter().unzip();
    (chrome, lines, map)
}

/// 后台面板框里的：每条一行，点开的下面铺底色写输出；结束了的多于一条时收起来的那一行（第 3 条）。
fn background(
    jobs: &Jobs,
    view: &JobsView,
    config: &Config,
    width: u16,
    now: Instant,
) -> (Vec<Line<'static>>, Vec<Option<usize>>) {
    let words = &config.text().jobs;
    let mut out = Vec::new();
    let mut map = Vec::new();
    for (i, item) in jobs.panel_items(view.all).into_iter().enumerate() {
        let picked = i == view.picked;
        let job = match &item {
            PanelItem::Job(id) => jobs.by_id(id),
            _ => None,
        };
        // 没选中的、在跑的命令照正常色（Miyu 2026-10-02 项目主人：原来压暗）；结束了的（完成、失败、停了、超时）命令那一截暗
        // （Miyu `858796fb`），后面的状态照旧；「已收起 N 个」那种说明行照旧暗。
        let style = match (picked, job) {
            (true, _) => Style::new().add_modifier(Modifier::BOLD),
            (false, Some(job)) if job.ended.is_none() => Style::new(),
            (false, _) => theme::dim(),
        };
        let (title, state) = match (&item, job) {
            (PanelItem::More(count), _) => (
                words.more_ended.replace("{count}", &count.to_string()),
                None,
            ),
            (PanelItem::Less, _) => (words.less_ended.clone(), None),
            (_, Some(job)) => {
                let (state, state_style) = state(job, config, now);
                (job.headline(), Some(Span::styled(state, state_style)))
            }
            _ => continue,
        };
        // 照命令列表的样子：选中的 ❯、铺强调色的底，状态贴着右边（`ui/panel.rs`）。
        out.push(panel::item(
            picked,
            vec![Span::styled(title, style)],
            state,
            width,
        ));
        map.push(Some(i));
        if let Some(job) = job.filter(|j| view.open.as_deref() == Some(j.id.as_str())) {
            for line in expanded(job, view, config, now, width) {
                out.push(line);
                map.push(Some(i));
            }
        }
    }
    (out, map)
}

/// 点开的一条：空行、状态、完整的命令、空行、最后几行输出（缩进两格，照常的颜色；在跑的跟着新输出走）、空行，整块铺底色、
/// 铺满宽度。还没读到的只有状态和命令；一个字都没有的、读不到的写「没有输出」（「后台命令、子代理和侧边栏」第 3 条）。
fn expanded(
    job: &Job,
    view: &JobsView,
    config: &Config,
    now: Instant,
    width: u16,
) -> Vec<Line<'static>> {
    let shade = theme::shade();
    let fill = |text: String, style: Style| {
        let pad = usize::from(width).saturating_sub(text.width());
        Line::styled(format!("{text}{}", " ".repeat(pad)), style.patch(shade))
    };
    let (state, style) = state(job, config, now);
    // 展开的字比收着的那一行亮一档：状态暗（失败的照旧红），输出照常的颜色（Miyu 2026-09-30 项目主人：原来太暗）。
    let style = if style == theme::error() {
        style
    } else {
        theme::dim()
    };
    let mut out = vec![
        fill(String::new(), Style::new()),
        fill(format!("  {state}"), style),
    ];
    // 完整的命令（那一行只写得下第一行、截掉的；Miyu 2026-09-30 项目主人：展开看不到完整命令），空一行接输出。
    let room = width.saturating_sub(2);
    for (piece, _) in crate::input::pieces(job.title.trim(), room.max(1)) {
        out.push(fill(format!("  {piece}"), Style::new()));
    }
    if let Some(output) = &view.output {
        out.push(fill(String::new(), Style::new()));
        let text = output
            .as_deref()
            .map(|text| clean(text, config.layout.tab))
            .unwrap_or_default();
        let lines: Vec<&str> = text.trim_end().lines().collect();
        if lines.is_empty() {
            out.push(fill(
                format!("  {}", config.text().jobs.no_output),
                theme::dim(),
            ));
        }
        for line in &lines[lines.len().saturating_sub(PANEL_ROWS)..] {
            out.push(fill(format!("  {}", clip(line, room)), Style::new()));
        }
    }
    out.push(fill(String::new(), Style::new()));
    out
}

/// 一条后台任务后面的状态和颜色（Miyu `background::state`）：失败、被信号杀掉的红，超时、输出太大、太勤停掉的黄，别的
/// `faint`。子代理状态行报完了的那一行也照它写（`agents.rs`）。
pub(super) fn state(job: &Job, config: &Config, now: Instant) -> (String, Style) {
    let words = &config.text().jobs;
    let elapsed = meter::clock(job.elapsed(now).as_secs());
    let Some(ended) = &job.ended else {
        return (words.running.replace("{elapsed}", &elapsed), theme::faint());
    };
    match (ended.reason.as_str(), ended.code, ended.signal) {
        ("exited", Some(0), _) | ("done", ..) => {
            (words.done.replace("{elapsed}", &elapsed), theme::faint())
        }
        ("exited", _, Some(signal)) => (
            words.killed.replace("{signal}", &signal.to_string()),
            theme::error(),
        ),
        ("exited", code, None) => (
            words
                .failed
                .replace("{code}", &code.map_or_else(String::new, |c| c.to_string())),
            theme::error(),
        ),
        ("undone", ..) => (words.undone.clone(), theme::faint()),
        ("restarted" | "aborted", ..) => (words.restarted.clone(), theme::faint()),
        ("timed_out", ..) => (words.timed_out.clone(), theme::warn()),
        ("too_large", ..) => (words.too_large.clone(), theme::warn()),
        ("too_chatty", ..) => (words.too_chatty.clone(), theme::warn()),
        _ => (words.stopped.clone(), theme::faint()),
    }
}

#[cfg(test)]
mod tests;
