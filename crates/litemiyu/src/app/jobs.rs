//! 后台任务在界面这一头（Miyu 终端 `app/jobs.rs`、`app/output.rs`，蓝图 `tui.md`「后台命令、子代理和侧边栏」）：派出去的记进
//! 任务表（`jobs.rs`），结束了在正文末尾写一行（后台命令的点开看命令和输出）；底栏有在跑的后台命令时一个按钮，点它（或输入
//! 框空着时按 `↓` 焦点停到它上面再 `Enter`，`agents.rs`）开后台面板：`↑` `↓` 选，`Enter` 展开看输出（在跑的每秒再读一次，
//! 跟着长）、展开收起来的结束了的，`x` 停掉，`Esc` 关。子代理推来的（确认它的高危命令）归抽屉，「谁在问」写它的名字。
//!
//! 和 Miyu 不一样的：输出每次读末尾一截（引擎的 `job_output`），不照 `size` 接着读。

use std::time::{Duration, Instant};

use crossterm::event::{KeyCode, KeyEvent, MouseEvent, MouseEventKind};
use ratatui::layout::Position;

use super::{App, Panel};
use crate::core::{ChildEnd, Command, JobEnd, JobStart, Push, Update};
use crate::jobs::PanelItem;
use crate::transcript::{MarkTone, ReportClass};

/// 展开着在跑的命令隔多久再读一次输出（Miyu `layout.json` 的 `output.poll_ms`）。
const POLL: Duration = Duration::from_millis(1000);

/// 后台面板开着时的样子：选中第几行、结束了的展开了没有、展开了哪一条、读到的输出（还没读到的是 `None`，读不到的是
/// `Some(None)`），和读输出的请求什么时候发的、回没回来。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct JobsView {
    /// 选中第几行（`Jobs::panel_items` 的第几个）。
    pub picked: usize,
    /// 结束了的多于一条时收起来的，展开了。
    pub all: bool,
    /// 展开着的那一条的任务编号。
    pub open: Option<String>,
    /// 展开着的那一条读到的输出。
    pub output: Option<Option<String>>,
    /// 最近一次去读输出的时刻。
    pub asked: Option<Instant>,
    /// 读输出的请求发出去了、还没回来：不再发第二个。
    pub waiting: bool,
}

impl App {
    /// 后台任务的几种消息：交回 `true` 的不再往下走。
    pub(super) fn jobs_update(&mut self, update: &Update) -> bool {
        let now = Instant::now();
        match update {
            Update::Push(Push::JobStarted(started)) => self.job_started(started, now),
            Update::Push(Push::JobEnded(end)) => self.job_ended(end, now),
            // 监视的命令出了一批行：正文末尾一行「监视 · 标题 · N 行」，记号强调色、字暗，点开看这一批（同 miyu-tui
            // `app/sessions/watched.rs`）。不记任务编号：命令了结时读回来的整份输出补进了结那一行，不补进这里。
            Update::Push(Push::JobLines { job, text, lines }) => {
                let title = self
                    .talk
                    .jobs
                    .by_id(job)
                    .map_or_else(|| job.clone(), crate::jobs::Job::headline);
                let note = self
                    .config
                    .text()
                    .jobs
                    .lines_note
                    .replace("{title}", &title)
                    .replace("{lines}", &lines.to_string());
                self.job_note((MarkTone::Accent, ReportClass::Other), note, text.clone());
            }
            Update::Push(Push::ChildEnded(end)) => self.child_ended(end, now),
            Update::Child { session, push } => {
                self.child_push(session, push);
            }
            Update::ChildBehind => {
                let text = self.config.text().jobs.fell_behind.clone();
                self.hint(text, false);
            }
            Update::JobOutput { job, text } => self.took_output(job, text.as_deref()),
            Update::StopRefused { reason, .. } => {
                let text = match reason.as_str() {
                    "job_ended" | "unknown_job" => self.config.text().jobs.already_ended.clone(),
                    other => self.config.text().refused.replace("{reason}", other),
                };
                self.hint(text, false);
            }
            _ => return false,
        }
        true
    }

    /// 派出去一个：后台命令写命令本身（照开它的那一步 `shell` 的参数），拿不到的、子代理写调用时的 `description`。
    fn job_started(&mut self, started: &JobStart, now: Instant) {
        let command = (!started.agent)
            .then(|| self.talk.transcript.tool_args(&started.call_id))
            .flatten()
            .and_then(|args| args["command"].as_str().map(str::to_string));
        let title = command.unwrap_or_else(|| started.title.clone());
        self.talk.jobs.start(started, title, now);
    }

    /// 后台命令结束了：正文末尾写一行（完成绿点、失败红叉、停了暗点），点开看的全文先是命令本身，读它的输出，读回来了接在
    /// 后面（Miyu `announce`、`fetch_final`）；展开着看它的，面板跟着换成这一份。
    fn job_ended(&mut self, end: &JobEnd, now: Instant) {
        let Some(job) = self.talk.jobs.end(end, now).cloned() else {
            return;
        };
        let texts = &self.config.text().jobs;
        let elapsed = crate::meter::clock(job.ended.as_ref().map_or(0, |e| e.took.as_secs()));
        let (tone, text) = match (end.reason.as_str(), end.exit_code, end.signal) {
            ("exited", Some(0), _) => (
                MarkTone::Good,
                texts.done_note.replace("{elapsed}", &elapsed),
            ),
            ("exited", _, Some(signal)) => (
                MarkTone::Bad,
                texts.killed_note.replace("{signal}", &signal.to_string()),
            ),
            ("exited", code, None) => (
                MarkTone::Bad,
                texts
                    .failed_note
                    .replace("{code}", &code.map_or_else(String::new, |c| c.to_string())),
            ),
            (reason, ..) => (
                MarkTone::Quiet,
                texts.stopped_note.replace("{why}", why(texts, reason)),
            ),
        };
        let note = self.job_note(
            (tone, ReportClass::Command),
            text.replace("{title}", &job.headline()),
            job.title.trim().to_string(),
        );
        if let Some(entry) = self.talk.jobs.by_id_mut(&job.id) {
            entry.note = Some(note);
        }
        let view = &mut self.boxes.jobs;
        if view.open.as_deref() == Some(job.id.as_str()) {
            view.waiting = true;
            view.asked = Some(now);
        }
        self.link.core.send(Command::JobOutput(job.id));
    }

    /// 引擎交回了一条命令的输出：展开着看它的面板换上；结束了的补进正文里它那一行（命令、空行、洗过的输出，Miyu
    /// `took_output`）。
    fn took_output(&mut self, job: &str, text: Option<&str>) {
        let view = &mut self.boxes.jobs;
        if view.open.as_deref() == Some(job) {
            view.waiting = false;
            view.output = Some(text.map(str::to_string));
        }
        let Some(entry) = self.talk.jobs.by_id(job) else {
            return;
        };
        let (Some(_), Some(note), Some(text)) = (&entry.ended, entry.note, text) else {
            return;
        };
        let nothing = &self.config.text().jobs.no_output;
        let output = crate::jobs::clean::note_detail(text, self.config.layout.tab, nothing);
        let detail = format!("{}\n\n{output}", entry.title.trim());
        self.talk.transcript.fill_report(note, detail);
    }

    /// 子代理报回来了：正文末尾写一行，点开看它交回来的报告全文（Miyu「后台命令、子代理和侧边栏」第 5 条，
    /// `ui/report_rows.rs`）。
    fn child_ended(&mut self, end: &ChildEnd, now: Instant) {
        let Some(job) = self.talk.jobs.report(end, now).cloned() else {
            return;
        };
        let texts = &self.config.text().jobs;
        let (tone, text) = match end.reason.as_str() {
            "done" => {
                let took = job.ended.as_ref().map_or(0, |e| e.took.as_secs());
                let elapsed = crate::meter::clock(took);
                (
                    MarkTone::Good,
                    texts.agent_note.replace("{elapsed}", &elapsed),
                )
            }
            reason => (
                MarkTone::Quiet,
                texts
                    .agent_stopped_note
                    .replace("{why}", why(texts, reason)),
            ),
        };
        self.job_note(
            (tone, ReportClass::Task),
            text.replace("{title}", &job.headline()),
            end.text.clone(),
        );
    }

    /// 正文末尾一行后台任务的通知，`detail` 是点开看的全文，`class` 是一轮进行中来的记成时间线里一步时收起那一行照它数的
    /// 那一类（Miyu `jobs/announce.rs`）；交回这一行的编号。
    fn job_note(
        &mut self,
        (tone, class): (MarkTone, ReportClass),
        text: String,
        detail: String,
    ) -> u64 {
        let texts = &self.config.text().jobs;
        let mark = match tone {
            MarkTone::Good => texts.ok_mark.clone(),
            MarkTone::Bad => texts.fail_mark.clone(),
            MarkTone::Quiet => texts.stop_mark.clone(),
            MarkTone::Accent => texts.lines_mark.clone(),
        };
        self.talk
            .transcript
            .marked_from((mark, tone), text, detail, class)
    }

    /// 子会话推来的：确认、提问归抽屉（「谁在问」写它的名字）；工具有了结果的收起它的抽屉。它在做什么、用了多少、自己又派了
    /// 几个记进它那一条（子代理状态行照它写，`jobs/activity.rs`）。
    fn child_push(&mut self, session: &str, push: &Push) {
        self.agent_meter(session, push);
        if let Some(job) = self.talk.jobs.agent_mut(session) {
            job.activity.push(push, &self.link.human);
        }
        let who = self
            .talk
            .jobs
            .agent(session)
            .map_or_else(|| session.to_string(), |job| job.title.clone());
        let who = self.config.text().drawer.agent.replace("{name}", &who);
        self.asking_push_from(session, Some(who), push);
    }

    /// 开后台面板：选中第一条，什么都不展开。
    pub(super) fn open_jobs(&mut self) {
        self.boxes.jobs = JobsView::default();
        self.boxes.panel = Some(Panel::Jobs);
    }

    /// 后台面板开着时的按键。
    pub(super) fn jobs_key(&mut self, key: KeyEvent) {
        let items = self.talk.jobs.panel_items(self.boxes.jobs.all);
        let count = items.len();
        let picked = self.boxes.jobs.picked;
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => self.boxes.jobs.picked = picked.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => {
                self.boxes.jobs.picked = (picked + 1).min(count.saturating_sub(1));
            }
            KeyCode::Enter => self.pick_item(picked),
            KeyCode::Char('x') => {
                if let Some(PanelItem::Job(id)) = items.get(picked) {
                    self.stop_job(id);
                }
            }
            KeyCode::Esc => self.boxes.panel = None,
            _ => {}
        }
    }

    /// 后台面板里选了第几行（Miyu `pick_item`）：一条命令点开、收起（一次只开一条，点开时马上读一次输出）；收起来的那一行
    /// 展开，展开以后最后那一行收起，选中的跟着它走。
    fn pick_item(&mut self, index: usize) {
        let all = self.boxes.jobs.all;
        let item = self.talk.jobs.panel_items(all).get(index).cloned();
        let view = &mut self.boxes.jobs;
        match item {
            Some(PanelItem::Job(id)) => {
                view.picked = index;
                if view.open.as_deref() == Some(id.as_str()) {
                    view.open = None;
                } else {
                    view.open = Some(id);
                }
                view.output = None;
                view.asked = None;
                view.waiting = false;
            }
            Some(PanelItem::More(_) | PanelItem::Less) => {
                view.all = !all;
                view.picked = self.talk.jobs.panel_items(!all).len().saturating_sub(1);
            }
            None => return,
        }
        self.poll_output();
    }

    /// 读展开着的那一条的输出（Miyu `poll_output`）：还没读过的马上读；在跑的隔 [`POLL`] 再读一次，跟着长；结束了的读过一次
    /// 就不再读。上一个请求还没回来的不发。
    fn poll_output(&mut self) {
        let Some(id) = self.boxes.jobs.open.clone() else {
            return;
        };
        let running = self
            .talk
            .jobs
            .by_id(&id)
            .is_some_and(|job| job.ended.is_none());
        let now = Instant::now();
        let view = &mut self.boxes.jobs;
        let due = match view.asked {
            None => true,
            Some(at) => running && now.saturating_duration_since(at) >= POLL,
        };
        if view.waiting || !due {
            return;
        }
        view.waiting = true;
        view.asked = Some(now);
        self.link.core.send(Command::JobOutput(id));
    }

    /// 到点了：后台面板开着的，展开着在跑的那一条再读一次输出。
    pub(super) fn advance_jobs(&mut self) {
        if self.boxes.panel == Some(Panel::Jobs) {
            self.poll_output();
        }
    }

    /// 停掉一条在跑的后台命令：交给引擎，结束的回报来了再出通知；结束了的什么都不做（Miyu `stop_job`）。
    fn stop_job(&mut self, id: &str) {
        if self
            .talk
            .jobs
            .by_id(id)
            .is_some_and(|job| job.ended.is_none())
        {
            self.link.core.send(Command::StopJob(id.to_string()));
        }
    }

    /// 底栏的按钮、后台面板上的鼠标（Miyu `jobs_mouse`）：点按钮开面板；指针移到面板某一行就选中它（不按也跟），按下才开、收；
    /// 点在框的边上不算点中哪一条，但也归它。交回归不归它。
    pub(super) fn jobs_mouse(&mut self, mouse: MouseEvent, at: Position) -> bool {
        let press = matches!(mouse.kind, MouseEventKind::Down(_));
        let areas = self.screen.areas;
        if areas.jobs_button.contains(at) {
            if press {
                self.open_jobs();
            }
            return press;
        }
        if self.boxes.panel != Some(Panel::Jobs) || !areas.menu.contains(at) {
            return false;
        }
        // 点在框的边上不算点中哪一条（「斜杠命令列表」第 3 条）。
        let row = at.y.checked_sub(areas.menu_text.y).map(usize::from);
        let index = row.and_then(|row| self.screen.panel_rows.get(row).copied().flatten());
        if let Some(index) = index.filter(|_| areas.menu_text.contains(at)) {
            self.boxes.jobs.picked = index;
            if press {
                self.pick_item(index);
            }
        }
        true
    }

    /// 后台面板开着、有在跑的：每秒醒一次，用时跟着走；展开着在跑的到点再读输出。别的时候不醒。
    pub(super) fn jobs_deadline(&self) -> Option<Instant> {
        let open = self.boxes.panel == Some(Panel::Jobs);
        if !open || self.talk.jobs.running_commands() == 0 {
            return None;
        }
        let view = &self.boxes.jobs;
        let read = view
            .asked
            .filter(|_| !view.waiting && view.open.is_some())
            .map(|at| at + POLL);
        let tick = Instant::now() + POLL;
        Some(read.map_or(tick, |read| read.min(tick)))
    }
}

/// 停了的为什么（正文那一行接在名字后面）：人停的、她自己停的不写。
fn why<'a>(texts: &'a crate::config::JobTexts, reason: &str) -> &'a str {
    match reason {
        "undone" => &texts.why_undone,
        "restarted" | "aborted" => &texts.why_restarted,
        "timed_out" => &texts.why_timed_out,
        "too_large" => &texts.why_too_large,
        "too_chatty" => &texts.why_too_chatty,
        _ => "",
    }
}
