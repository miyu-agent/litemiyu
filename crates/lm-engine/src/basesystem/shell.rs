//! `shell`（`10-自带软件.md` 第三节「`shell` 的细则」，施工 4-8）：在这一轮的工作目录里执行一条命令，交回输出和退出码。
//! 前台的：跑完才交回，超时、叫停时整组杀掉。写了 `run_in_background` 的放到后台（施工 7-3）：照前台一样起，交给任务端口，
//! 当场交回编号；写了 `watch` 的照放到后台走，标准输出每出一批行叫醒她（施工 MN-1 上）。前台的到时不杀、挪到后台接着跑
//! （施工 7-12）。
//!
//! 用哪个 shell（[`program`]）、命令拿得到哪些环境变量（[`env`](mod@env)）、起命令和整组杀（[`process`]）、主程序没了命令跟着
//! 没（[`lifeline`]）、输出怎么截（[`output`]）、后台命令（[`background`]）各在一处。每次调用起一个新的 shell，`cd`、变量都不带到下一次。
//!
//! **没有沙盒**（设计第二节「不要的」）：命令能碰的就是 LiteMiyu 这个用户能碰的一切，包括 LiteMiyu 自己的数据目录（配置、
//! 密钥、会话日志）。权限策略拦工具报的路径，命令不报路径，拦不住。拦得住的只有高危命令的认法（[`Tool::hazard`]，内核的
//! `policy::hazard`）照命令的字面认出来的那几类：删根、删家目录、删数据目录、删系统目录、格式化、写裸磁盘这些灭顶的直接拒，
//! 别的高危（`git push --force` 这一类）问人。写法绕开字面的（拼出来的路径、`eval`、脚本文件里的）认不出，照样跑。

mod background;
mod env;
pub(super) mod lifeline;
mod moved;
mod output;
mod person;
mod process;
mod program;
mod results;
#[cfg(test)]
mod tests;
mod touched;
mod waiting;

use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

use crate::fs::tilde;
use crate::tool::{
    Background, Call, Done, Effect, Exit, Halt, Hazard, PersonCommand, PersonRunning, Process as _,
    Progress, Running, Spec, Tool,
};
use lm_kernel::kernel::event::{JobKind, JobStarted};
use lm_kernel::kernel::template::Template;
use lm_kernel::kernel::tool::{Access, Worded};
use lm_kernel::policy::hazard::{Kind, Places, assess};

use crate::basesystem::blocking::blocking;
use crate::basesystem::common::{Common, said};
use crate::basesystem::load::{self, LoadError, say};
use process::{Finished, Guard, Handover, Waited};
use program::Program;
use results::Results;
use waiting::{limit, sleeps};

/// 不写 `timeout` 的等多久，毫秒（照 Claude Code）。改它要跟着改参数格式里的那一句。
const DEFAULT: u64 = 120_000;

/// `timeout` 最多写多大，毫秒（照 Claude Code）。改它要跟着改参数格式里的那一句。
const MAX: u64 = 600_000;

/// 监视的那个参数（施工 MN-1 上，设计 56 第二节）：写了它的照放到后台走，标准输出每出一批行叫醒她。和
/// `run_in_background` 一样只在有后台时给（[`crate::tool::Tool::background_parameters`]）。
const WATCH: &str = "watch";

/// `shell`。
pub(crate) struct Shell {
    spec: Spec,
    texts: Texts,
    program: Program,
}

/// 输出里给她看的几句：`software/basesystem/shell/*.txt`，和几件工具共用的。
struct Texts {
    common: Common,
    results: Results,
    timed_out: Template,
    failed: Template,
    no_background: Template,
    /// 前台干等的不跑时写给她的一句（2026-10-26）。
    no_sleep: Template,
    /// 工作目录没了时写给她的一句（施工 CW-1），字段 `cwd`。
    no_cwd: Template,
    /// 认出灭顶的高危、直接拒时写给她的一句（施工 5-17），字段 `reason`。
    hazard: Template,
    started: Template,
    /// 监视的命令交回编号时写给她的一句（`started-watch.txt`，施工 MN-1 上），字段 `job`、`minutes`。
    started_watch: Template,
    /// 直接拒的那几种各自说它做了什么（`shell/hazard/<种类>.txt`），填进 `hazard`。
    reasons: Vec<(Kind, Template)>,
}

/// 她给的参数。
#[derive(Deserialize)]
struct Args {
    command: String,
    /// 这条命令在做什么的短标题（施工 4-13，2026-09-28 项目主人定）：必填，前台的执行不用它，记在调用里给界面显示、问人时
    /// 给人看；后台的是任务的标题，记进 `job.started`（施工 7-3）。
    description: String,
    /// 前台的超时；后台的不看它（施工 7-3）。
    timeout: Option<u64>,
    #[serde(default)]
    run_in_background: bool,
    /// 监视（施工 MN-1 上）：照放到后台走，标准输出、标准错误分开读，标准输出每出一批行记 `job.lines`、叫醒她。
    #[serde(default)]
    watch: bool,
}

impl Shell {
    /// 照编进程序的字造，共用的几句是 `common`。用哪个 shell 在这时候找，写进说明里。
    pub(crate) fn load(common: Common) -> Result<Shell, LoadError> {
        let program = Program::find(std::env::var_os("PATH").as_deref());
        let text = |name: &str, fields: &[&str]| load::text("shell", name, fields);
        Ok(Shell {
            spec: load::spec_filled("shell", Access::Execute, &[("shell", program.name())])?,
            texts: Texts {
                common,
                results: Results::load()?,
                timed_out: text("timed-out", &["timeout", "max"])?,
                failed: text("failed", &["shell", "error"])?,
                no_background: text("no-background", &[])?,
                no_sleep: text("no-sleep", &[])?,
                no_cwd: text("no-cwd", &["cwd"])?,
                hazard: text("hazard", &["reason"])?,
                started: text("started", &["job"])?,
                started_watch: text("started-watch", &["job", "minutes"])?,
                reasons: Kind::ALL
                    .into_iter()
                    .filter(|kind| kind.refused())
                    .map(|kind| {
                        Ok((
                            kind,
                            text(&format!("hazard/{}", kind.code().replace('_', "-")), &[])?,
                        ))
                    })
                    .collect::<Result<_, LoadError>>()?,
            },
            program,
        })
    }

    /// 起不来、等不了。
    fn failed(&self, error: &io::Error) -> Done {
        let error = error.to_string();
        Done::error(say(
            &self.texts.failed,
            &[("shell", self.program.name()), ("error", &error)],
        ))
        .said(said("shell/failed").with("error", error))
    }

    /// 造命令：先挑主程序的环境（去掉 `LITEMIYU_*`），再设这一轮的模型、日志目录那几个。前台、后台一样。
    fn command(&self, call: &Call, line: &str) -> std::process::Command {
        let mut env = env::passed(std::env::vars_os());
        env.extend(env::told(call.caller.as_ref()));
        self.program.command(line, &workdir(call), env)
    }

    /// 放到后台（施工 7-3）：照前台一样造命令、起进程，交给任务端口，当场交回编号，报 `job.started`。起进程以后一个 `await`
    /// 都没有：交上了就一定交回结果，不会交上了却被掐掉、没人知道它在跑。没有任务端口的（会话外面的调用）不跑。
    fn background(&self, call: &Call, args: Args) -> Done {
        let Some(jobs) = &call.jobs else {
            return Done::error(say(&self.texts.no_background, &[]))
                .said(said("shell/no-background"));
        };
        let command = self.command(call, &args.command);
        let started =
            background::start(command, args.watch).and_then(|background| jobs.start(background));
        match started {
            Ok(job) => {
                let id = job.to_string();
                // 监视的说清每一行都会叫醒她、只认标准输出、跑到几分钟停下（设计 56 第二节第 2 条）。
                let text = match args.watch {
                    true => {
                        let minutes = jobs.longest().as_secs().div_ceil(60).to_string();
                        say(
                            &self.texts.started_watch,
                            &[("job", &id), ("minutes", &minutes)],
                        )
                    }
                    false => say(&self.texts.started, &[("job", &id)]),
                };
                Done::ok(text)
                    .said(said("shell/background").with("job", id))
                    .effect(Effect::JobStarted(JobStarted {
                        foreground: false,
                        fork: false,
                        job,
                        what: JobKind::Command,
                        title: args.description,
                        session: None,
                    }))
            }
            Err(error) => self.failed(&error),
        }
    }

    /// 到时挪到了后台（施工 7-12）：到这一刻读到的和之后的输出、进程交给任务端口，当场交回编号，报 `job.started`。
    /// 端口收不下的（会话已经停了），任务表整组杀掉了它，照起不来说。
    /// 给她的照放到后台的那一句（`started.txt`）：实测（2026-10-20）另写「到时挪过去了」那一句，她下一步一样，都去看输出、
    /// 没有一次重跑，不另加。
    fn moved(&self, call: &Call, handover: Handover, title: String) -> Done {
        let Some(jobs) = &call.jobs else {
            handover.process.kill();
            return self.failed(&io::Error::other("no job port"));
        };
        let Handover {
            output,
            rest,
            process,
        } = handover;
        let first = self.texts.results.so_far(&output);
        let background = Background {
            output: Box::new(moved::Rest::new(first, rest)),
            lines: None,
            process: Box::new(process),
        };
        match jobs.start(background) {
            Ok(job) => {
                let id = job.to_string();
                Done::ok(say(&self.texts.started, &[("job", &id)]))
                    .said(said("shell/moved").with("job", id))
                    .effect(Effect::JobStarted(JobStarted {
                        foreground: false,
                        fork: false,
                        job,
                        what: JobKind::Command,
                        title,
                        session: None,
                    }))
            }
            Err(error) => self.failed(&error),
        }
    }

    /// 跑完了：输出，加上它怎么结束的；到时被杀的照 `shell` 的那一句。
    fn finished(&self, finished: Finished, timeout: u64) -> Done {
        self.texts.results.ended(finished, || {
            let note = say(
                &self.texts.timed_out,
                &[("timeout", &timeout.to_string()), ("max", &MAX.to_string())],
            );
            (note, seconds(timeout))
        })
    }
}

impl Tool for Shell {
    fn spec(&self) -> &Spec {
        &self.spec
    }

    /// 人在会话里跑命令交给它（设计 57、63 第六节第 2 条）：只认内置的这一件。
    fn serves_person(&self) -> bool {
        true
    }

    /// 只给人用（`person.rs`）：她的调用走 `run`。
    fn run_for_person(&self, command: PersonCommand, halt: Halt) -> Option<PersonRunning<'_>> {
        Some(self.person(command, halt))
    }

    fn background_parameters(&self) -> Vec<&str> {
        vec![crate::tool::RUN_IN_BACKGROUND, WATCH]
    }

    /// 照命令的字面认高危（施工 5-17，`lm_kernel::policy::hazard`）：家目录、数据目录照这次调用带的，工作目录照家目录换成
    /// 绝对路径。LiteMiyu 没有放在数据目录里的工作区，`workspaces` 是空的。
    fn hazard(&self, call: &Call) -> Option<Hazard> {
        let args = lm_kernel::kernel::json::from_str::<Args>(&call.args).ok()?;
        let home = call
            .home
            .as_deref()
            .map(|home| home.to_string_lossy().into_owned());
        let cwd = crate::fs::resolve(Path::new("/"), call.home.as_deref(), &call.cwd).map_or_else(
            |_| call.cwd.clone(),
            |cwd| cwd.to_string_lossy().into_owned(),
        );
        let data_root = call
            .data_root
            .as_deref()
            .map(|root| root.to_string_lossy().into_owned())
            .unwrap_or_default();
        let places = Places {
            cwd: &cwd,
            home: home.as_deref(),
            data_root: &data_root,
            workspaces: "",
        };
        let kind = assess(&args.command, &places)?;
        let refused = self
            .texts
            .reasons
            .iter()
            .find(|(known, _)| *known == kind)
            .map(|(_, reason)| Worded {
                text: say(
                    &self.texts.hazard,
                    &[("reason", say(reason, &[]).trim_end())],
                ),
                said: Some(said(&format!("shell/hazard-{}", kind.code()))),
            });
        Some(Hazard {
            code: kind.code(),
            refused,
        })
    }

    fn asking(&self, call: &Call) -> Vec<(&'static str, String)> {
        lm_kernel::kernel::json::from_str::<Args>(&call.args).map_or_else(
            |_| Vec::new(),
            |args| vec![("command", args.command), ("title", args.description)],
        )
    }

    fn run(&self, call: Call, progress: Progress) -> Running<'_> {
        Box::pin(async move {
            let args = match lm_kernel::kernel::json::from_str::<Args>(&call.args) {
                Ok(args) => args,
                Err(error) => return self.texts.common.bad_args(&error),
            };
            // 工作目录没了（删了、挪走了）：命令起不来，照实说，不换到别处跑（施工 CW-1，2026-11-02 项目主人定）。原来报的是
            // 「起不了 bash」，她换什么命令都一样，出不去。
            if !workdir(&call).is_dir() {
                return Done::error(say(&self.texts.no_cwd, &[("cwd", &call.cwd)]))
                    .said(said("shell/no-cwd").with("cwd", call.cwd.clone()));
            }
            if args.run_in_background || args.watch {
                return self.background(&call, args);
            }
            // 前台干等的不跑（2026-10-26）：后台任务办完会自己回报。
            if waiting::waits(&args.command) {
                return Done::error(say(&self.texts.no_sleep, &[])).said(said("shell/no-sleep"));
            }
            let timeout = limit(args.timeout);
            let command = self.command(&call, &args.command);
            // 她看过的文件这一刻的样子：跑完了比，命令改了的当她自己改的（施工 E-D 补）。
            let seen = touched::before(&call.seen);
            let wakes = process::Wakes::new();
            let started = match process::start(command, move |text| progress.push(text)) {
                Ok(started) => started,
                Err(error) => return self.failed(&error),
            };
            // 起来了就看着：这次调用被叫停（future 被丢掉）时，整组杀掉。
            let guard = Guard::new(started.group());
            let wait = Duration::from_millis(timeout);
            // 后台运行开着的，到时不杀、挪到后台（施工 7-12）；开头是 `sleep` 的照旧杀：等着本来就是它要做的。
            let movable = call.jobs.is_some() && !sleeps(&args.command);
            let waited = blocking(call.stop.clone(), move |_| match movable {
                true => started.wait_or_move(wait, wakes),
                false => started.wait(wait, wakes).map(Waited::Finished),
            })
            .await;
            guard.disarm();
            match waited {
                Ok(Waited::Moved(handover)) => self.moved(&call, handover, args.description),
                Ok(Waited::Finished(finished)) => {
                    let done = self.finished(finished, timeout);
                    let done = self.texts.results.screenshot(&args.command, done);
                    touched::after(seen).into_iter().fold(done, Done::effect)
                }
                Err(error) => self.failed(&error),
            }
        })
    }
}

/// 命令在哪个目录里跑：这一轮的工作目录，`~` 开头的照家目录接上，和别的工具一个规矩（施工 4-9 再补二）；没有家目录
/// 的照原样，起不来时说工作目录不在。
fn workdir(call: &Call) -> PathBuf {
    match (tilde(&call.cwd), call.home.as_deref()) {
        (Some(rest), Some(home)) => home.join(rest),
        _ => PathBuf::from(&call.cwd),
    }
}

/// 给人看的秒数：整秒的不带小数。
fn seconds(millis: u64) -> String {
    match millis % 1000 {
        0 => (millis / 1000).to_string(),
        _ => format!("{:.1}", millis as f64 / 1000.0),
    }
}

/// 不是空的，就以换行结尾。
fn line(mut text: String) -> String {
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text
}

/// 杀掉它的信号。
fn signal(status: std::process::ExitStatus) -> String {
    use std::os::unix::process::ExitStatusExt;
    status
        .signal()
        .map_or_else(|| "?".to_string(), |signal| signal.to_string())
}

/// 怎么结束的（人跑的命令交给界面的）：有退出码的照退出码；没有的是被信号杀掉的。
fn exit(status: std::process::ExitStatus) -> Exit {
    use std::os::unix::process::ExitStatusExt;
    match status.code() {
        Some(code) => Exit::Code(code),
        None => Exit::Signal(
            status
                .signal()
                .and_then(|signal| u32::try_from(signal).ok())
                .unwrap_or(0),
        ),
    }
}
