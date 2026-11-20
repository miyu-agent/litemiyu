//! 跑一轮：起全屏同一个引擎线程（[`Core`]），开会话、说一句，照推送记账、交给输出，等这一轮
//! 和它派出去的子代理、后台命令都了结；Ctrl+C、SIGTERM、`--timeout`、`--max-steps` 打断；最后有计划地停下会话（日志落好盘，
//! 后台命令整组停掉）再收尾。界面那一套（终端、读按键的线程）一样都不起。

use std::os::unix::net::UnixStream;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use lm_kernel::kernel::event::{Decision, Response};
use lm_kernel::kernel::session::Answer;

use super::ending::{Ending, Stop, ending};
use super::follow::Follow;
use super::out::Output;
use crate::cli::words::Words;
use crate::core::{Asking, Command, Core, Push, Update};

/// 收尾时等会话停下最多多久：停不下的（卡在写盘上）不一直等。
const CLOSING: Duration = Duration::from_secs(30);

/// 主线程等的一样。
enum Event {
    /// 引擎交来的（装在盒子里：它比信号大得多）。
    Update(Box<Update>),
    /// 收到的信号：SIGINT 是假，SIGTERM 是真。
    Signal(bool),
}

/// 怎么跑。
pub struct Plan {
    /// 接着说的会话；`None` 新开一个。
    pub session: Option<String>,
    /// 说的话。
    pub message: String,
    /// 要问人的高危命令放行（`--yes`）。
    pub yes: bool,
    /// 新开无功能的那一档。
    pub bare: bool,
    /// 应急模式备好的。
    pub rescue: Option<crate::rescue::Prepared>,
    /// `--timeout`。
    pub timeout: Option<Duration>,
    /// `--max-steps`。
    pub max_steps: Option<u64>,
    /// `--model`：先换成它再说（照配置查过）。
    pub model: Option<String>,
}

/// 照 `plan` 跑，交给 `output`，交回收尾。
pub fn drive(plan: Plan, words: &Words, output: &mut dyn Output) -> Ending {
    let (tx, rx) = mpsc::channel();
    signals(&tx);
    let mut core = Core::new(move |update| tx.send(Event::Update(Box::new(update))).is_ok());
    if plan.bare {
        core.first_bare();
    }
    if let Some(prepared) = plan.rescue {
        core.rescue(prepared);
    }
    let mut run = Run {
        core,
        rx,
        follow: Follow::new(words.code()),
        yes: plan.yes,
        session: None,
        stop: None,
        deadline: plan
            .timeout
            .and_then(|timeout| Instant::now().checked_add(timeout)),
        max_steps: plan.max_steps,
    };
    if !plan.yes {
        run.core.send(Command::Unattended);
    }
    let new = plan.session.is_none();
    let command = Command::Send { text: plan.message };
    let ended = run.go(plan.session, (command, plan.model), words, output, new);
    run.close();
    output.finish(&ended, &run.follow, run.session.as_deref());
    ended
}

/// 跑着的这一次。
struct Run {
    core: Core,
    rx: mpsc::Receiver<Event>,
    follow: Follow,
    yes: bool,
    /// 开好的会话。
    session: Option<String>,
    /// 命令行自己停下的。
    stop: Option<Stop>,
    deadline: Option<Instant>,
    max_steps: Option<u64>,
}

impl Run {
    /// 开会话（接着说的先打开）、说一句、跟到了结，交回收尾。
    fn go(
        &mut self,
        session: Option<String>,
        (command, model): (Command, Option<String>),
        words: &Words,
        output: &mut dyn Output,
        new: bool,
    ) -> Ending {
        if let Some(id) = session {
            if let Err(early) = self.open(id, words) {
                return early;
            }
            output.session(self.session.as_deref().unwrap_or_default(), false);
        }
        if let Some(model) = model {
            self.core.send(Command::Configure(model));
        }
        self.core.send(command);
        if let Err(early) = self.follow_until_done(words, output, new) {
            return early;
        }
        ending(&self.follow, self.stop, words)
    }

    /// 打开旧会话 `id`：补发的一截不看（只回声），开好了记下编号。
    fn open(&mut self, id: String, words: &Words) -> Result<(), Ending> {
        self.core.send(Command::Open {
            id: id.clone(),
            view_broken: false,
        });
        loop {
            let update = match self.next() {
                Some(Event::Update(update)) => *update,
                Some(Event::Signal(_)) | None => return Err(self.stopped(words)),
            };
            match update {
                Update::Opened(opened) => {
                    self.session = Some(opened);
                    return Ok(());
                }
                Update::OpenFailed { taken, message, .. } => {
                    let (reason, message) = match taken {
                        true => ("taken", words.say("taken", &[("id", &id)])),
                        false => ("failed", words.say("open_failed", &[("reason", &message)])),
                    };
                    words.error(&message, None);
                    return Err(Ending::early(1, reason));
                }
                Update::Failed(message) => {
                    words.error(&message, None);
                    return Err(Ending::early(1, "failed"));
                }
                Update::Replayed { done, .. } => drop(done.send(())),
                _ => {}
            }
        }
    }

    /// 开会话时就停下的（按了 Ctrl+C、到了时限）。
    fn stopped(&mut self, words: &Words) -> Ending {
        let stop = *self.stop.get_or_insert(Stop::Interrupted);
        ending(&self.follow, Some(stop), words)
    }

    /// 下一样：到了时限的是 `None`（记成停下）。
    fn next(&mut self) -> Option<Event> {
        let got = match self.deadline {
            Some(deadline) => {
                let left = deadline.saturating_duration_since(Instant::now());
                self.rx.recv_timeout(left).ok()
            }
            None => self.rx.recv().ok(),
        };
        if got.is_none() && self.stop.is_none() {
            self.stop = Some(Stop::TimedOut);
        }
        got
    }

    /// 跟着说的这一句开的几轮，直到了结：这一轮结束了、派出去的都报了、没有要来的一轮。
    fn follow_until_done(
        &mut self,
        words: &Words,
        output: &mut dyn Output,
        new: bool,
    ) -> Result<(), Ending> {
        loop {
            let Some(event) = self.next() else {
                // 到了时限：打断在进行的这一轮，不等它收尾。
                self.interrupt();
                return Ok(());
            };
            match event {
                Event::Signal(term) => {
                    if self.stop.is_some() || term {
                        self.stop.get_or_insert(Stop::Interrupted);
                        self.interrupt();
                        return Ok(());
                    }
                    self.stop = Some(Stop::Interrupted);
                    self.interrupt();
                }
                Event::Update(update) => self.update(*update, words, output, new)?,
            }
            if self.max_steps.is_some_and(|max| self.follow.opened > max) && self.stop.is_none() {
                self.stop = Some(Stop::MaxSteps);
                self.interrupt();
            }
            if self.settled(words, output, new)? {
                return Ok(());
            }
        }
    }

    /// 办引擎交来的一样。
    fn update(
        &mut self,
        update: Update,
        words: &Words,
        output: &mut dyn Output,
        new: bool,
    ) -> Result<(), Ending> {
        match update {
            Update::Ready(id) if new && self.session.is_none() => {
                output.session(&id, true);
                self.session = Some(id);
            }
            Update::Push(Push::Asking(asking)) => {
                let session = self.session.clone().unwrap_or_default();
                self.answer(session, &asking);
            }
            Update::Push(push) => self.follow.push(push, output),
            Update::Child {
                session,
                push: Push::Asking(asking),
            } => self.answer(session, &asking),
            Update::Unsent { reason, message } => {
                words.error(&words.say("unsent", &[("reason", &message)]), None);
                let why = if reason.is_some() {
                    "refused"
                } else {
                    "failed"
                };
                return Err(Ending::early(1, why));
            }
            Update::Failed(message) => {
                words.error(&message, None);
                return Err(Ending::early(1, "failed"));
            }
            // 子代理的推送掉了队、补不上（审计 #063）：警告一句，接着等。
            Update::ChildBehind => words.warn(words.get("child_behind"), None),
            Update::Refused { message, .. } => {
                words.error(&words.say("refused", &[("reason", &message)]), None);
                return Err(Ending::early(1, "refused"));
            }
            // 补发的一截交过来了：回一声（读日志的那一头不等了的没处可回）。
            Update::Replayed { done, .. } => drop(done.send(())),
            _ => {}
        }
        Ok(())
    }

    /// 了结了没有：跟过一轮、这时没有一轮在进行、派出去的都报了，再过一道栅栏（同一批里接着开的一轮不会还在路上）还是这样。
    /// 还有没报的说在等。
    fn settled(
        &mut self,
        words: &Words,
        output: &mut dyn Output,
        new: bool,
    ) -> Result<bool, Ending> {
        if self.follow.turns.is_empty() || self.follow.busy() {
            return Ok(false);
        }
        if self.stop.is_some() {
            return Ok(true);
        }
        let (done, back) = mpsc::channel();
        self.core.send(Command::Barrier(done));
        if back.recv_timeout(CLOSING).is_err() {
            return Ok(true);
        }
        while let Ok(event) = self.rx.try_recv() {
            match event {
                Event::Update(update) => self.update(*update, words, output, new)?,
                Event::Signal(_) => {
                    self.stop = Some(Stop::Interrupted);
                    return Ok(true);
                }
            }
        }
        if self.follow.busy() {
            return Ok(false);
        }
        if self.follow.jobs.is_empty() {
            return Ok(true);
        }
        let agents = self.follow.jobs.values().filter(|job| job.agent).count();
        output.waiting(agents, self.follow.jobs.len() - agents);
        Ok(false)
    }

    /// 替人答一次问（开关交到以前就问了的、`--yes`）：确认的，`--yes` 允许这一次，不然拒；提问的，每道题都不答。
    fn answer(&mut self, session: String, asking: &Asking) {
        let (call, answer) = match asking {
            Asking::Approval(asked) => {
                let decision = if self.yes {
                    Decision::Once
                } else {
                    Decision::Deny
                };
                if !self.yes {
                    self.follow.refused.push(asked.call_id.to_string());
                }
                let answer = Answer::Approval {
                    decision,
                    reason: None,
                    attachments: Vec::new(),
                };
                (asked.call_id.to_string(), answer)
            }
            Asking::Asked(asked) => {
                let blank = asked
                    .questions
                    .iter()
                    .map(|_| Response {
                        picked: Vec::new(),
                        text: None,
                        notes: None,
                        attachments: Vec::new(),
                    })
                    .collect();
                (asked.call_id.to_string(), Answer::Questions(blank))
            }
            Asking::Answered(_) | Asking::Decided(_) => return,
        };
        self.core.send(Command::Answer {
            session,
            call,
            answer,
        });
    }

    /// 打断在进行的这一轮（同全屏 Ctrl+C：排着的话退回来）。
    fn interrupt(&self) {
        self.core.send(Command::Interrupt { send: false });
    }

    /// 有计划地停下会话：日志落好盘，后台命令、子代理整组停掉（同全屏 `/new` 停下旧会话），等它办完再交回。
    fn close(&self) {
        self.core.send(Command::New { bare: None });
        let (done, back) = mpsc::channel();
        self.core.send(Command::Barrier(done));
        let _waited = back.recv_timeout(CLOSING);
    }
}

/// SIGINT、SIGTERM 写进一对套接字，各一条线程读出来交给主线程（不轮询）。装不上的照常跑，只是收不到。
fn signals(tx: &mpsc::Sender<Event>) {
    for (signal, term) in [
        (signal_hook::consts::SIGINT, false),
        (signal_hook::consts::SIGTERM, true),
    ] {
        let Ok((knock, listen)) = UnixStream::pair() else {
            continue;
        };
        if signal_hook::low_level::pipe::register(signal, knock).is_err() {
            continue;
        }
        let tx = tx.clone();
        let spawned = std::thread::Builder::new()
            .name("signals".into())
            .spawn(move || {
                use std::io::Read;
                let mut listen = listen;
                let mut byte = [0u8; 1];
                while matches!(listen.read(&mut byte), Ok(1)) {
                    if tx.send(Event::Signal(term)).is_err() {
                        return;
                    }
                }
            });
        if spawned.is_err() {}
    }
}
