//! 正在看的会话派出去的后台任务（Miyu 终端 `jobs/mod.rs`，蓝图 `tui.md`「后台命令、子代理和侧边栏」）：照 `job.started`、
//! `job.reported`、`child.reported` 记一张表。后台命令进底栏的按钮和后台面板，子代理进输入框下面的子代理状态行。这次启动以来
//! 结束了的也留着（面板、状态行照它写怎么结束的）；后台面板里结束了的多于一条时收成一行（[`Jobs::panel_items`]）。

use std::time::{Duration, Instant};

use crate::core::{ChildEnd, JobEnd, JobStart};

mod activity;
pub use activity::{Activity, Doing};

pub mod clean;

/// 后台面板里的一行（Miyu `jobs/mod.rs` 的 `PanelItem`，蓝图「后台命令、子代理和侧边栏」第 3 条）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PanelItem {
    /// 一条命令：任务编号。
    Job(String),
    /// 结束了的多于一条时收起来的那一行：收了几条。
    More(usize),
    /// 展开以后最后那一行：收起。
    Less,
}

/// 一个任务。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    /// 任务编号，`j2`、`j2.1` 这样的。
    pub id: String,
    /// 是子代理；不是的是后台命令。
    pub agent: bool,
    /// 后台命令写命令本身（照开它的那一步 `shell` 的 `command`），拿不到的、子代理写调用时的 `description`。
    pub title: String,
    /// 子代理的会话。
    pub session: Option<String>,
    /// fork 出来的子代理（LM-18）：状态行名字前面画分叉记号。
    pub fork: bool,
    /// 收到 `job.started` 的时刻。
    pub started: Instant,
    /// 结束了的怎么结束的；在跑的是 `None`。
    pub ended: Option<Ended>,
    /// 子代理正在做什么、用了多少（子会话推来的，`activity.rs`）；后台命令一直是空的。
    pub activity: Activity,
    /// 后台命令结束了在正文里写的那一行的编号：输出读回来了照它补进点开看的全文（Miyu 照任务编号 `fill_job`）。
    pub note: Option<u64>,
}

impl Job {
    /// 一行里写的标题：多行的命令只写第一行，后面接 ` …`（Miyu `Job::headline`，2026-09-30 Miyu 项目主人：原来换行被吃掉，
    /// 几行挤成一串）。
    pub fn headline(&self) -> String {
        let mut lines = self.title.trim().lines();
        let first = lines.next().unwrap_or_default().trim_end();
        if lines.next().is_some() {
            format!("{first} {}", crate::sheet::ellipsis())
        } else {
            first.to_string()
        }
    }

    /// 结束的时刻：开始的时刻加跑了多久；在跑的是 `None`。
    fn ended_at(&self) -> Option<Instant> {
        self.ended.as_ref().map(|e| self.started + e.took)
    }

    /// 跑了多久：结束了的照结束时记的，在跑的照 `now`。
    pub fn elapsed(&self, now: Instant) -> Duration {
        self.ended
            .as_ref()
            .map_or_else(|| now.saturating_duration_since(self.started), |e| e.took)
    }
}

/// 怎么结束的。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ended {
    /// 内核的原因：后台命令的 `exited`、`stopped`……，子代理的 `done`、`stopped`……。
    pub reason: String,
    /// 退出码。
    pub code: Option<i32>,
    /// 杀掉它的信号。
    pub signal: Option<u32>,
    /// 跑了多久。
    pub took: Duration,
}

/// 一张表，照开始的先后。
#[derive(Debug, Clone, Default)]
pub struct Jobs {
    all: Vec<Job>,
}

impl Jobs {
    /// 派出去一个：`title` 是界面定好的那个写法。同一个编号又来一次的（子代理报过、又被留言叫醒）不重记。
    pub fn start(&mut self, started: &JobStart, title: String, now: Instant) {
        if self.all.iter().any(|job| job.id == started.job) {
            return;
        }
        self.all.push(Job {
            id: started.job.clone(),
            agent: started.agent,
            title,
            session: started.session.clone(),
            fork: started.fork,
            started: now,
            ended: None,
            activity: Activity::default(),
            note: None,
        });
    }

    /// 后台命令结束了：交回记着的那一条；不认识的是 `None`。用时照 `duration_ms`，没有的照收到的时刻算。
    pub fn end(&mut self, end: &JobEnd, now: Instant) -> Option<&Job> {
        let job = self.all.iter_mut().find(|job| job.id == end.job)?;
        let took = end.duration_ms.map_or_else(
            || now.saturating_duration_since(job.started),
            Duration::from_millis,
        );
        job.ended = Some(Ended {
            reason: end.reason.clone(),
            code: end.exit_code,
            signal: end.signal,
            took,
        });
        Some(job)
    }

    /// 子代理报回来了：交回记着的那一条。报完了被留言叫醒的，下一次再报。
    pub fn report(&mut self, end: &ChildEnd, now: Instant) -> Option<&Job> {
        let job = self.all.iter_mut().find(|job| job.id == end.job)?;
        job.ended = Some(Ended {
            reason: end.reason.clone(),
            code: None,
            signal: None,
            took: now.saturating_duration_since(job.started),
        });
        Some(job)
    }

    /// 后台面板里的一条一条（Miyu `Board::panel_items`，蓝图「后台命令、子代理和侧边栏」第 3 条）：在跑的照开始的先后；
    /// 结束了的只有一条的照列，多于一条的只露最近结束的那条，别的收成一行（2026-09-30 Miyu 项目主人定），`all` 是展开了，
    /// 照开始的先后全列、最后一行收起。
    pub fn panel_items(&self, all: bool) -> Vec<PanelItem> {
        let shells = self.all.iter().filter(|job| !job.agent);
        let (running, ended): (Vec<&Job>, Vec<&Job>) = shells.partition(|job| job.ended.is_none());
        let mut out: Vec<PanelItem> = running
            .iter()
            .map(|job| PanelItem::Job(job.id.clone()))
            .collect();
        match ended.iter().max_by_key(|job| job.ended_at()) {
            Some(_) if ended.len() == 1 || all => {
                out.extend(ended.iter().map(|job| PanelItem::Job(job.id.clone())));
                if ended.len() > 1 {
                    out.push(PanelItem::Less);
                }
            }
            Some(last) => {
                out.push(PanelItem::Job(last.id.clone()));
                out.push(PanelItem::More(ended.len() - 1));
            }
            None => {}
        }
        out
    }

    /// 任务编号是 `id` 的那一条。
    pub fn by_id(&self, id: &str) -> Option<&Job> {
        self.all.iter().find(|job| job.id == id)
    }

    /// 同上，改它用。
    pub fn by_id_mut(&mut self, id: &str) -> Option<&mut Job> {
        self.all.iter_mut().find(|job| job.id == id)
    }

    /// 还没结束的任务有几个（后台命令、子代理都算）：shell 集成等它们都报了才算了结（`app/inline.rs`）。
    pub fn pending(&self) -> usize {
        self.all.iter().filter(|job| job.ended.is_none()).count()
    }

    /// 在跑的后台命令有几条：底栏的按钮照它。
    pub fn running_commands(&self) -> usize {
        self.all
            .iter()
            .filter(|job| !job.agent && job.ended.is_none())
            .count()
    }

    /// 子代理，照开始的先后。
    pub fn agents(&self) -> impl Iterator<Item = &Job> {
        self.all.iter().filter(|job| job.agent)
    }

    /// 子代理状态行列的（Miyu `Board::listed`）：在跑的，加上正在看的那个（报完了也留着，「切进子会话」第 5 条），照开始的
    /// 先后。
    pub fn listed(&self, viewing: Option<&str>) -> Vec<&Job> {
        let viewed = |job: &Job| viewing.is_some() && job.session.as_deref() == viewing;
        self.agents()
            .filter(|job| job.ended.is_none() || viewed(job))
            .collect()
    }

    /// 子会话 `session` 的子代理，改它在做什么用。
    pub fn agent_mut(&mut self, session: &str) -> Option<&mut Job> {
        self.all
            .iter_mut()
            .find(|job| job.agent && job.session.as_deref() == Some(session))
    }

    /// 子会话 `session` 是哪个子代理：抽屉的「谁在问」照它的名字写。
    pub fn agent(&self, session: &str) -> Option<&Job> {
        self.agents()
            .find(|job| job.session.as_deref() == Some(session))
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::{Jobs, PanelItem};
    use crate::core::{ChildEnd, JobEnd, JobStart};

    fn started(job: &str, agent: bool) -> JobStart {
        JobStart {
            call_id: format!("call_{job}"),
            job: job.into(),
            agent,
            title: format!("做 {job}"),
            session: agent.then(|| format!("s-{job}")),
            fork: false,
        }
    }

    #[test]
    fn commands_list_running_first_and_agents_are_found_by_session() {
        let now = Instant::now();
        let mut jobs = Jobs::default();
        jobs.start(&started("j1", false), "sleep 9".into(), now);
        jobs.start(&started("j2", true), "做 j2".into(), now);
        jobs.start(&started("j3", false), "make".into(), now);
        jobs.start(&started("j1", false), "又来一次".into(), now);
        assert_eq!(jobs.running_commands(), 2);
        let end = JobEnd {
            job: "j1".into(),
            reason: "exited".into(),
            exit_code: Some(0),
            signal: None,
            duration_ms: Some(1500),
        };
        let ended = jobs.end(&end, now).expect("认识");
        assert_eq!(ended.title, "sleep 9", "同一个编号不重记");
        assert_eq!(
            ended.ended.as_ref().map(|e| e.took),
            Some(Duration::from_millis(1500))
        );
        let order = jobs.panel_items(false);
        assert_eq!(
            order,
            [PanelItem::Job("j3".into()), PanelItem::Job("j1".into())],
            "在跑的在前"
        );
        assert_eq!(jobs.running_commands(), 1);
        assert_eq!(jobs.agent("s-j2").map(|j| j.id.as_str()), Some("j2"));
        let report = ChildEnd {
            job: "j2".into(),
            session: "s-j2".into(),
            reason: "done".into(),
            text: "好了".into(),
        };
        assert!(jobs.report(&report, now).is_some_and(|j| j.ended.is_some()));
        assert!(
            jobs.end(
                &JobEnd {
                    job: "j9".into(),
                    ..end
                },
                now
            )
            .is_none(),
            "不认识的"
        );
    }
}
