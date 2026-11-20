//! 子代理状态行（Miyu 终端 `ui/agents.rs`，蓝图 `tui.md`「后台命令、子代理和侧边栏」第 6 条）：框下面那一行下面空一行，
//! `● 主会话`，每个子代理一行 `○ 名字  正在做什么…  token · 用时`。照主会话的任务表：在跑的，加上正在看的那个（报完了也
//! 留着，写它怎么结束的，「切进子会话」第 5 条）。正在看的那个会话实心圆，别的空心；子代理自己又派了的，名字后面暗色写
//! `（+N）`。焦点在这一块时，那一行前面品红的 `❯ `、字照常的颜色，别的行暗；指针停的那一行亮一档。窄窗口没有侧边栏，从
//! 这里进子会话（`app/agents.rs`）。
//!
//! 没有列着的子代理时这一块不占行，这里的代码不跑。

use std::time::Instant;

use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

use super::rows::clip;
use crate::app::{Screen, Seen};
use crate::jobs::{Doing, Job};
use crate::{meter, theme};

/// 这一块占几行（连上面的空行）；没有列着的子代理是 0。
pub fn height(count: usize, most: usize) -> u16 {
    if count == 0 {
        return 0;
    }
    u16::try_from(2 + count.min(most) + usize::from(count > most)).unwrap_or(u16::MAX)
}

/// 每一行：空行、主会话、子代理（名字、正在做什么各对齐成一列，用时和 token 靠右）、收起来的。`focus` 是焦点在第几行
/// （0 是主会话），`hover` 是指针停在第几行（第 0 行是空行）。
pub fn lines(
    seen: &Seen,
    agents: &[&Job],
    width: u16,
    (focus, hover): (Option<usize>, Option<usize>),
    now: Instant,
) -> Vec<Line<'static>> {
    let words = &seen.config.text().jobs;
    let most = seen.config.layout.agent_rows;
    let viewing = seen.talk.viewing();
    // 第 `row` 行（第 0 行是空行）的字：焦点所在的照常的颜色；悬停的亮一档（Miyu `theme::hover`）；焦点不在这一块时正在看的
    // 那一行照常；别的暗（Miyu 2026-09-30 项目主人：原来焦点那一行整行品红）。
    let style = |row: usize, viewed: bool| {
        if focus == Some(row - 1) {
            Style::new()
        } else if hover == Some(row) {
            theme::hover()
        } else if viewed && focus.is_none() {
            Style::new()
        } else {
            theme::dim()
        }
    };
    // 每行前面两格：焦点所在的那一行写品红的 `❯ `。
    let lead = |row: usize, style: Style| {
        if focus == Some(row - 1) {
            Span::styled(crate::sheet::mark(|m| &m.pointer), theme::picked())
        } else {
            Span::styled("  ", style)
        }
    };
    let dot = |on: bool| if on { "●" } else { "○" };
    let main_style = style(1, viewing.is_none());
    let mut out = vec![
        Line::raw(""),
        Line::from(vec![
            lead(1, main_style),
            Span::styled(
                format!("{} {}", dot(viewing.is_none()), words.main),
                main_style,
            ),
        ]),
    ];
    // 名字连着「（+N）」算宽，对齐成一列（Miyu `label`）；fork 出来的名字前面接分叉记号（LM-18，照 Miyu 终端：2026-10-26
    // Miyu 项目主人）。
    let label = |job: &Job| {
        let fork = if job.fork {
            seen.config.icons.fork.as_str()
        } else {
            ""
        };
        match job.activity.nested() {
            0 => format!("{fork}{}", job.title),
            n => format!("{fork}{}（+{n}）", job.title),
        }
    };
    let name_w = agents.iter().map(|a| label(a).width()).max().unwrap_or(0);
    for (i, job) in agents.iter().take(most).enumerate() {
        let row = i + 2;
        // token 还是 0 的不写，只写用时（Miyu 2026-10-26 项目主人）。
        let clock = meter::clock(job.elapsed(now).as_secs());
        let stats = match job.activity.tokens {
            0 => clock,
            n => format!("{}{}{clock}", meter::short(n), crate::sheet::sep()),
        };
        let on = viewing.is_some() && job.session.as_deref() == viewing;
        let title = label(job);
        let pad = " ".repeat(name_w - title.width());
        let name = format!("{} {title}{pad}", dot(on));
        let room = usize::from(width).saturating_sub(2 + name.width() + 2 + stats.width() + 2);
        let what = match &job.ended {
            None => doing(seen, job),
            Some(_) => ended(job, seen.config, now),
        };
        let doing = clip(&what, u16::try_from(room).unwrap_or(0));
        let gap = room.saturating_sub(doing.width()) + 2;
        let style = style(row, on);
        out.push(Line::from(vec![
            lead(row, style),
            Span::styled(format!("{name}  {doing}{}", " ".repeat(gap)), style),
            Span::styled(stats, style),
        ]));
    }
    if agents.len() > most {
        let more = words
            .more
            .replace("{count}", &(agents.len() - most).to_string());
        let style = style(most + 2, false);
        out.push(Line::from(vec![
            lead(most + 2, style),
            Span::styled(more, style),
        ]));
    }
    out
}

/// 报完了还列着的（正在看它）：正在做什么那一列写它怎么结束的，照面板的写法去掉括号；完成的只写「完成」，用时在右边
/// （Miyu `ended`）。
fn ended(job: &Job, config: &crate::config::Config, now: Instant) -> String {
    if job.ended.as_ref().is_some_and(|e| e.reason == "done") {
        return config.text().jobs.finished.clone();
    }
    let (state, _) = super::jobs_panel::state(job, config, now);
    state.trim_matches(['（', '）']).to_string()
}

/// 子代理正在做什么（Miyu `notes.rs` 的 `doing`）：在想、在说，或者最新那一步「显示名 · 对象」；一轮没在跑的空着。
fn doing(seen: &Seen, job: &Job) -> String {
    let activity = &job.activity;
    if !activity.busy {
        return String::new();
    }
    let words = &seen.config.text().jobs;
    match &activity.doing {
        Doing::Nothing => String::new(),
        Doing::Thinking => words.doing_thinking.clone(),
        Doing::Replying => words.doing_replying.clone(),
        Doing::Tool { name, subject } => {
            let human = &seen.link.human;
            let mut out = human.step_name(name, &Default::default(), seen.config.language.code());
            if let Some(subject) = subject {
                out.push_str(&crate::sheet::sep());
                out.push_str(&crate::local::home_short(subject));
            }
            out
        }
    }
}

/// 画这一块，记下能点的那几行（主会话那一行起）和每一行切到哪个会话（`None` 是主会话，`screen.agent_rows`）。
pub fn draw(frame: &mut super::Frame, area: Rect, seen: &Seen, screen: &mut Screen) {
    screen.areas.agent_rows = Rect::default();
    screen.agent_rows.clear();
    if area.height == 0 {
        return;
    }
    let agents = seen.talk.jobs.listed(seen.talk.viewing());
    // 第 0 行是空行，收起来的那一行点了不切。
    let most = seen.config.layout.agent_rows;
    screen.agent_rows.push(None);
    screen.agent_rows.extend(
        agents
            .iter()
            .take(most)
            .map(|j| Some(j.session.clone().unwrap_or_default())),
    );
    let rows = u16::try_from(screen.agent_rows.len()).unwrap_or(u16::MAX);
    let clickable = Rect {
        y: area.y + 1,
        height: rows.min(area.height.saturating_sub(1)),
        ..area
    };
    screen.areas.agent_rows = clickable;
    // 指针停在能点的那几行上：那一行亮一档（Miyu `screen.agents_hover`，第 0 行是空行）。
    let hover = screen
        .pointer
        .filter(|p| clickable.contains(*p))
        .map(|p| usize::from(p.y - area.y));
    let focus = match seen.focus() {
        crate::focus::Focus::Agent(i) => Some(i),
        _ => None,
    };
    let lines = lines(seen, &agents, area.width, (focus, hover), Instant::now());
    frame.render_widget(Paragraph::new(lines), area);
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::ended;
    use crate::config::Config;
    use crate::core::{ChildEnd, JobStart};
    use crate::jobs::Jobs;

    /// Miyu `ui/agents.rs::ended`：报完了还列着的那一行，完成的写「完成」，别的照后台面板的状态去掉括号，各写各的（原来一律
    /// 「已停止」）。
    #[test]
    fn a_reported_agent_says_how_it_ended() {
        let config = Config::builtin().unwrap();
        let t0 = Instant::now();
        let said = |reason: &str| {
            let mut jobs = Jobs::default();
            let start = JobStart {
                call_id: String::new(),
                job: "j1".into(),
                agent: true,
                title: "慢慢读".into(),
                session: Some("s1".into()),
                fork: false,
            };
            jobs.start(&start, "慢慢读".into(), t0);
            let end = ChildEnd {
                job: "j1".into(),
                session: "s1".into(),
                reason: reason.into(),
                text: String::new(),
            };
            let job = jobs
                .report(&end, t0 + Duration::from_secs(22))
                .unwrap()
                .clone();
            ended(&job, &config, t0 + Duration::from_secs(99))
        };
        assert_eq!(said("done"), "完成");
        assert_eq!(said("stopped"), "已停止");
        assert_eq!(said("undone"), "撤销时已停止");
        assert_eq!(said("aborted"), "上次退出时已停止");
    }
}
