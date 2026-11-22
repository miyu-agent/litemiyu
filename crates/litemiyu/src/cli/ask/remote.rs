//! 对正开在别的窗口里的会话说一句（LM-59，2026-11-23 项目主人：别的程序要能用命令行对正在运行的会话说话、拿到回答）。
//!
//! LiteMiyu 没有常驻的核心：一个会话同一时间只在一个进程里，`ask` 不能再打开它。这里不打开，分两头做：
//!
//! - 送进去：往那个会话的收件目录放一封「人说的一句」（`lm_engine::session::say`），敲拿着它的那个窗口的门铃。那个窗口
//!   照人在输入框里打的一句办：会话闲着的开一轮，正忙着的排进这一轮。窗口本来就等着收件目录，不为这件事多起任何东西。
//! - 拿回来：只读地跟着那个会话的日志（`store::log::read_marked`，从记下的位置读多出来的那一截），照补发的那一套读成推送
//!   （`core::push::replay`），交给和平常一样的账（`follow.rs`）和输出（`out.rs`）。日志里这一句的事件带着它的命令编号
//!   （`cause`），认出它以后的才算数、才输出；它所在的那一轮结束、没有接着的一轮，就了结。
//!
//! 和平常的 `ask` 不一样的地方：回答是照日志一段一段来的（一次请求回完才落盘），不是一个字一个字；`Ctrl+C`、`--timeout`
//! 只是不再等，不打断那个窗口里的会话；要问人的高危命令由那个窗口里的人答，这里答不了（`--yes` 不起作用）；`--model`、
//! `--max-steps` 不起作用。

use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, Instant};

use lm_engine::session::Said;
use lm_engine::store::log::{self, Mark};
use lm_engine::store::paths::{Env, Paths};
use lm_kernel::kernel::event::Event as Logged;
use lm_kernel::kernel::id::SessionId;

use super::drive::{Event, Plan, signals};
use super::ending::{Ending, Stop, ending};
use super::follow::{Follow, Out, Sink};
use super::out::Output;
use crate::cli::words::Words;
use crate::core::Push;
use crate::core::push::replay::Replay;

/// 多久看一次日志。
const POLL: Duration = Duration::from_millis(100);

/// 那个窗口最晚多久收下这一句：门铃响了当场收；门铃没响的靠它每 30 秒扫一次收件目录，再留些余量。
const PICKUP: Duration = Duration::from_secs(45);

/// 收下以后最晚多久在日志里看到这一句：交给内核、落盘是几十毫秒的事；那个会话正在撤销、恢复的会退避着再交，最多几秒。
const APPEAR: Duration = Duration::from_secs(30);

/// 这一轮结束以后再看几次日志都没有新的一轮，才算了结（同一批里接着开的一轮不会还在路上）。
const QUIET_ROUNDS: u32 = 3;

/// 不输出的那一头：补上「这一轮已经开了」的账时用。
struct Quiet;

impl Sink for Quiet {
    fn put(&mut self, _out: Out<'_>) {}
}

/// 照 `plan` 对开在别的窗口里的会话说一句，跟到了结，交给 `output`，交回收尾。
pub fn drive(plan: Plan, words: &Words, output: &mut dyn Output) -> Ending {
    let id = plan.session.clone().unwrap_or_default();
    let mut follow = Follow::new(words.code());
    let ended = run(&plan, &id, words, output, &mut follow);
    output.finish(&ended, &follow, Some(&id));
    ended
}

/// 开好以前就办不下去的：说一句，退出码 1。
fn failed(words: &Words, message: &str, reason: &str) -> Ending {
    words.error(message, None);
    Ending::early(1, reason)
}

fn run(
    plan: &Plan,
    id: &str,
    words: &Words,
    output: &mut dyn Output,
    follow: &mut Follow,
) -> Ending {
    let list_failed = |why: &str| {
        failed(
            words,
            &words.say("list_failed", &[("reason", &words.why(why))]),
            "failed",
        )
    };
    let paths = match Paths::locate(&Env::current()) {
        Ok(paths) => paths,
        Err(error) => return list_failed(&error.to_string()),
    };
    let Ok(session) = SessionId::parse(id) else {
        return failed(
            words,
            &words.say("no_such_session", &[("id", id)]),
            "not_found",
        );
    };
    let dir = paths.session(&session);
    // 不起作用的选项先说一声（别的程序多半带着固定的选项调，不因为会话恰好开着就报错）。
    let ignored: Vec<&str> = [
        (plan.yes, "--yes"),
        (plan.model.is_some(), "--model"),
        (plan.max_steps.is_some(), "--max-steps"),
    ]
    .into_iter()
    .filter_map(|(given, name)| given.then_some(name))
    .collect();
    if !ignored.is_empty() {
        words.warn(
            &words.say("remote.ignored", &[("options", &ignored.join(" "))]),
            None,
        );
    }
    // 先记下日志现在照到哪里：这一句的事件只会在它后面。
    let mut tail = match Tail::at_end(&dir) {
        Ok(tail) => tail,
        Err(why) => return list_failed(&why),
    };
    let (tx, rx) = mpsc::channel();
    signals(&tx);
    let said = match lm_engine::session::say(&paths, &session, &plan.message) {
        Ok(said) => said,
        Err(error) => {
            let why = words.why(&error.to_string());
            return failed(words, &words.say("unsent", &[("reason", &why)]), "failed");
        }
    };
    output.session(id, false);
    let deadline = plan
        .timeout
        .and_then(|timeout| Instant::now().checked_add(timeout));
    let mut watch = Watch {
        said,
        posted: Instant::now(),
        taken: None,
        started: false,
        asked: false,
        quiet: 0,
        replay: Replay::default(),
    };
    let stop = loop {
        match rx.recv_timeout(POLL) {
            Ok(Event::Signal(_)) => break Some(Stop::Interrupted),
            Ok(Event::Update(_)) | Err(mpsc::RecvTimeoutError::Timeout) => {}
            // 收信号的线程没装上：照旧每隔一会儿看一次。
            Err(mpsc::RecvTimeoutError::Disconnected) => std::thread::sleep(POLL),
        }
        if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
            break Some(Stop::TimedOut);
        }
        let events = match tail.more(&dir) {
            Ok(events) => events,
            Err(why) => return list_failed(&why),
        };
        match watch.step(events, id, words, output, follow) {
            Ok(true) => break None,
            Ok(false) => {}
            Err(early) => return early,
        }
    };
    for push in watch.replay.finish() {
        follow.push(push, output);
    }
    if stop.is_some() {
        // 不等了：还没被收下的收回来，那个窗口不会过一会儿才收到一句没人等的话；已经收下的照旧在那边跑。
        if watch.taken.is_none() && !watch.said.give_up() {
            watch.taken = Some(Instant::now());
        }
        if watch.taken.is_some() {
            words.warn(words.get("remote.left_running"), None);
        }
    }
    ending(follow, stop, words)
}

/// 等这一句的过程。
struct Watch {
    said: Said,
    /// 放进收件目录的那一刻。
    posted: Instant,
    /// 那个窗口收下的那一刻（信不在了）。
    taken: Option<Instant>,
    /// 日志里看到这一句了：之后的事件才算数。
    started: bool,
    /// 说过「在等那个窗口里的人确认」了。
    asked: bool,
    /// 这一轮结束以后连着几次没有新的事件。
    quiet: u32,
    replay: Replay,
}

impl Watch {
    /// 办新读到的一截日志，交回了结了没有。
    fn step(
        &mut self,
        events: Vec<Logged>,
        id: &str,
        words: &Words,
        output: &mut dyn Output,
        follow: &mut Follow,
    ) -> Result<bool, Ending> {
        let fresh = !events.is_empty();
        for event in events {
            if !self.started {
                if event.cause.as_ref().map(|cause| cause.as_str()) != Some(self.said.cause()) {
                    continue;
                }
                self.started = true;
                // 会话正忙着的，这一句排进了已经开着的那一轮：补上「这一轮开了」的账，前面的不输出。
                if let Some(turn) = event.turn {
                    follow.push(Push::TurnStarted(turn.started().get(), None), &mut Quiet);
                }
            }
            for push in self.replay.read(event) {
                match push {
                    Push::Asking(_) => {
                        if !self.asked {
                            self.asked = true;
                            words.warn(words.get("remote.asking"), None);
                        }
                    }
                    push => follow.push(push, output),
                }
            }
        }
        if self.taken.is_none() && (self.started || self.said.taken()) {
            self.taken = Some(Instant::now());
        }
        if !self.started {
            return match self.taken {
                // 没被收下：那个窗口不在了、卡住了，或者门铃和兜底的扫都没轮到。收回这一句。
                None if self.posted.elapsed() >= PICKUP => {
                    if self.said.give_up() {
                        let seconds = PICKUP.as_secs().to_string();
                        let said =
                            words.say("remote.not_taken", &[("id", id), ("seconds", &seconds)]);
                        Err(failed(words, &said, "not_taken"))
                    } else {
                        self.taken = Some(Instant::now());
                        Ok(false)
                    }
                }
                // 收下了却一直没进日志：那个会话没有接受它（空的话、正在撤销恢复一直没轮到）。
                Some(taken) if taken.elapsed() >= APPEAR => Err(failed(
                    words,
                    &words.say("remote.refused", &[("id", id)]),
                    "refused",
                )),
                _ => Ok(false),
            };
        }
        // 了结：跟过一轮、这时没有一轮在进行，再看几次日志都没有新的。
        if follow.turns.is_empty() || follow.busy() || fresh {
            self.quiet = 0;
            return Ok(false);
        }
        self.quiet += 1;
        Ok(self.quiet >= QUIET_ROUNDS)
    }
}

/// 跟着读一份日志：记着照到了哪里，每次只读多出来的那一截。
struct Tail {
    mark: Option<Mark>,
}

impl Tail {
    /// 从日志现在的末尾跟起。
    fn at_end(dir: &Path) -> Result<Tail, String> {
        let mark = log::read_marked(dir, None, |_| true).map_err(|error| error.to_string())?;
        Ok(Tail { mark })
    }

    /// 上一次以后多出来的事件，照先后。
    fn more(&mut self, dir: &Path) -> Result<Vec<Logged>, String> {
        let mut events = Vec::new();
        let read = log::read_marked(dir, self.mark.as_ref(), |piece| {
            events.extend(piece);
            true
        })
        .map_err(|error| error.to_string())?;
        match (read, self.mark) {
            (Some(now), _) => self.mark = Some(now),
            // 记的位置和日志对不上了（那一段没有了）：从头读一遍，只要记的那一条以后的。
            (None, Some(old)) => {
                events.clear();
                let read = log::read_marked(dir, None, |piece| {
                    events.extend(piece.into_iter().filter(|event| event.seq >= old.next));
                    true
                })
                .map_err(|error| error.to_string())?;
                if read.is_some() {
                    self.mark = read;
                }
            }
            (None, None) => {}
        }
        Ok(events)
    }
}
