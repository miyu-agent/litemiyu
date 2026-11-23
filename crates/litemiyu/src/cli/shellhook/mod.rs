//! shell 集成（`litemiyu shellhook …`，施工线 shellhook，底子是 Miyu 终端的 `shellhook/`）：在 shell 的提示符后面
//! 直接说话。给人用的是 `install`、`uninstall`、`status`、`doctor`（帮助页列这四个）；`script`、`classify`、`say` 是装进 shell 的脚本
//! 调的，`worker` 是 `say` 起的后台进程，帮助页都不列，各自的 `-h` 照样有。
//!
//! 只在 `litemiyu shellhook` 这条路上执行：全屏界面启动时不读、不初始化这里的任何东西（项目主人的要求：不用的时候不增加
//! 占用）。印脚本、判断命令这两样每次 shell 起来、每次回车多行时都要跑，所以不读配置、不读给人看的字，要说话时才读。

mod classify;
mod inline;
mod install;
mod paint;
mod say;
mod script;
mod shells;
mod terminal;
mod wire;
mod worker;

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use install::{Done, Elsewhere, Found, Retired};
use shells::{ALL, Other, Shell};

use super::misuse;
use super::parse::{self, Misuse, Parsed};
use super::spec::{Cmd, Opt, nearest};
use super::words::Words;

/// 哪种 shell。
const SHELL: Opt = Opt::value("shell", None, "v.shell", "o.shellhook.shell");
/// 脚本调哪个程序。
const PROGRAM: Opt = Opt::value("program", None, "v.file", "o.shellhook.program");
const REPLACE: Opt = Opt::flag("replace", None, "o.shellhook.replace");

/// 表里的这一条：帮助页照它列给人用的三个。
pub const CMD: Cmd = Cmd {
    name: "shellhook",
    usage: "u.shellhook",
    summary: "s.shellhook",
    about: "a.shellhook",
    extra: "",
    opts: &[],
    subs: &[
        Cmd {
            name: "install",
            usage: "u.options",
            summary: "s.shellhook.install",
            about: "a.shellhook.install",
            extra: "",
            opts: &[SHELL, PROGRAM, REPLACE],
            subs: &[],
            run: None,
        },
        Cmd {
            name: "uninstall",
            usage: "u.options",
            summary: "s.shellhook.uninstall",
            about: "",
            extra: "",
            opts: &[SHELL],
            subs: &[],
            run: None,
        },
        Cmd {
            name: "status",
            usage: "",
            summary: "s.shellhook.status",
            about: "",
            extra: "",
            opts: &[],
            subs: &[],
            run: None,
        },
        Cmd {
            name: "doctor",
            usage: "u.options",
            summary: "s.shellhook.doctor",
            about: "a.shellhook.doctor",
            extra: "",
            opts: &[SHELL],
            subs: &[],
            run: None,
        },
    ],
    run: None,
};

/// 哪个 shell 在调：它的进程号。
const SHELL_PID: Opt = Opt::value("shell-pid", None, "v.id", "o.shellhook.pid");
/// 这个终端的记号。
const TERMINAL: Opt = Opt::value("terminal", None, "v.id", "o.shellhook.terminal");
/// 提前敲的字写在哪。
const LEFTOVER: Opt = Opt::value("leftover", None, "v.file", "o.shellhook.leftover");
/// 这一句是 `/edit` 改过的。
const REDO: Opt = Opt::flag("redo", None, "o.shellhook.redo");
/// 这个 shell 接后台回报的信号。
const REPORTS: Opt = Opt::flag("reports", None, "o.shellhook.reports");
const ASKED: Opt = Opt::flag("asked", None, "o.shellhook.asked");

/// 脚本调的那几条：帮助页不列。
const INTERNAL: &[Cmd] = &[
    Cmd {
        name: "say",
        usage: "u.options",
        summary: "s.shellhook.say",
        about: "",
        extra: "",
        opts: &[SHELL, SHELL_PID, TERMINAL, LEFTOVER, REDO, REPORTS],
        subs: &[],
        run: None,
    },
    Cmd {
        name: "worker",
        usage: "u.options",
        summary: "s.shellhook.worker",
        about: "",
        extra: "",
        opts: &[SHELL_PID, TERMINAL, LEFTOVER, REDO, REPORTS, ASKED],
        subs: &[],
        run: None,
    },
    Cmd {
        name: "report",
        usage: "u.options",
        summary: "s.shellhook.report",
        about: "",
        extra: "",
        opts: &[
            SHELL,
            SHELL_PID,
            TERMINAL,
            LEFTOVER,
            Opt::flag("pending", None, "o.shellhook.pending"),
            Opt::value("above", None, "v.n", "o.shellhook.above"),
        ],
        subs: &[],
        run: None,
    },
    Cmd {
        name: "script",
        usage: "u.options",
        summary: "s.shellhook.script",
        about: "",
        extra: "",
        opts: &[SHELL, PROGRAM],
        subs: &[],
        run: None,
    },
    Cmd {
        name: "classify",
        usage: "u.options",
        summary: "s.shellhook.classify",
        about: "",
        extra: "",
        opts: &[SHELL],
        subs: &[],
        run: None,
    },
];

/// 这一层的全名。
const PATH: &str = "litemiyu shellhook";

/// 办 `litemiyu shellhook <args>`。
pub fn main(args: &[String]) -> ExitCode {
    let (hook, args) = hook(args);
    // 开着的 shell 里是升级以前印的脚本（`script.rs`）：旧脚本调的参数这一版不认时说一声重开终端（坑 A2）。
    let stale = hook.is_some_and(|hook| hook != script::stamp());
    let code = run(args, stale);
    if stale && code == ExitCode::from(misuse::USAGE) {
        let words = Words::current();
        words.error(words.get("shellhook.stale"), None);
    }
    code
}

/// 脚本调的头上带的 `--hook <记号>` 拿出来，剩下的照原样。
fn hook(args: &[String]) -> (Option<&str>, &[String]) {
    match args {
        [flag, stamp, rest @ ..] if flag == "--hook" => (Some(stamp.as_str()), rest),
        _ => (None, args),
    }
}

fn run(args: &[String], stale: bool) -> ExitCode {
    let Some((name, rest)) = args.split_first() else {
        return super::print(&super::help::page(&Words::current(), PATH, &CMD));
    };
    if name == "-h" || name == "--help" {
        return super::print(&super::help::page(&Words::current(), PATH, &CMD));
    }
    let known = CMD
        .sub(name)
        .or_else(|| INTERNAL.iter().find(|cmd| cmd.name == name));
    let Some(cmd) = known else {
        let near = nearest(name, CMD.subs.iter().map(|cmd| cmd.name));
        let bad = Misuse::UnknownSub(PATH.to_string(), name.clone(), near);
        return misuse::tell(&Words::current(), PATH, &bad);
    };
    let path = format!("{PATH} {name}");
    let parsed = match checked(cmd, rest) {
        Ok(parsed) => parsed,
        Err(bad) => return misuse::tell(&Words::current(), &path, &bad),
    };
    if parsed.has("help") {
        return super::print(&super::help::page(&Words::current(), &path, cmd));
    }
    match cmd.name {
        "install" => install(&parsed, &Words::current()),
        "uninstall" => uninstall(&parsed, &Words::current()),
        "status" => status(&Words::current()),
        "doctor" => doctor(&parsed, &Words::current()),
        "script" => print_script(&parsed, &path),
        "say" => say::run(&parsed, stale),
        "report" => say::report(&parsed),
        "worker" => worker::run(&parsed),
        _ => classify_stdin(&parsed),
    }
}

/// 照 `cmd` 读参数；这一层的命令都不带词，`--shell` 写的得是认得的。
fn checked(cmd: &Cmd, args: &[String]) -> Result<Parsed, Misuse> {
    let parsed = parse::read(cmd, args)?;
    if let Some(word) = parsed.words.first() {
        return Err(Misuse::Unexpected(word.clone()));
    }
    if let Some(name) = parsed.value("shell")
        && Shell::named(name).is_none()
    {
        let names = ALL.iter().map(|shell| shell.name()).collect();
        return Err(Misuse::BadValue("shell", name.to_string(), names));
    }
    Ok(parsed)
}

/// `--shell` 写的那一种（[`checked`] 查过认得）。
fn written(parsed: &Parsed) -> Option<Shell> {
    parsed.value("shell").and_then(Shell::named)
}

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

/// 给了 `--shell` 的照它，没给的照上一层进程认；认不出的说一声，连同能写哪几种。
fn chosen(parsed: &Parsed, words: &Words) -> Result<Shell, ExitCode> {
    written(parsed).or_else(shells::detect).ok_or_else(|| {
        let names: Vec<&str> = ALL.iter().map(|shell| shell.name()).collect();
        let hint = words.say(
            "shellhook.unknown_shell.hint",
            &[("shells", &misuse::joined(words, &names))],
        );
        words.error(words.get("shellhook.unknown_shell"), Some(&hint));
        ExitCode::FAILURE
    })
}

/// 启动文件在哪；没有 `HOME` 的说一声。
fn target(shell: Shell, words: &Words) -> Result<PathBuf, ExitCode> {
    shell.target(&env).ok_or_else(|| {
        words.error(words.get("shellhook.no_home"), None);
        ExitCode::FAILURE
    })
}

/// 读不了、写不进：说一声，退出码 1。
fn failed(words: &Words, key: &str, path: &Path, error: &std::io::Error) -> ExitCode {
    let file = path.display().to_string();
    let reason = words.why(&error.to_string());
    words.error(
        &words.say(key, &[("file", &file), ("reason", &reason)]),
        None,
    );
    ExitCode::FAILURE
}

/// 别家的集成写成一句：哪个文件（第几行）是什么。
fn other_line(words: &Words, other: &Elsewhere) -> String {
    let path = other.path.display().to_string();
    let place = match other.line {
        Some(line) => words.say(
            "shellhook.at_line",
            &[("path", &path), ("line", &line.to_string())],
        ),
        None => path,
    };
    let key = match other.kind {
        Other::Miyu => "shellhook.other.miyu",
        Other::OldMiyu => "shellhook.other.old",
        Other::Maybe => "shellhook.other.maybe",
    };
    words.say(key, &[("where", &place)])
}

/// `shellhook install`。
fn install(parsed: &Parsed, words: &Words) -> ExitCode {
    let (shell, path) =
        match chosen(parsed, words).and_then(|shell| Ok((shell, target(shell, words)?))) {
            Ok(found) => found,
            Err(code) => return code,
        };
    let program = match parsed.value("program") {
        Some(program) => std::path::absolute(program),
        None => install::default_program(),
    };
    let program = match program {
        Ok(program) if program.is_file() => program,
        Ok(program) => {
            let program = program.display().to_string();
            words.error(
                &words.say("shellhook.no_program", &[("program", &program)]),
                None,
            );
            return ExitCode::FAILURE;
        }
        Err(error) => return failed(words, "shellhook.unreadable", Path::new("litemiyu"), &error),
    };
    // 别家的集成也在（坑 A7）：两份都接管回车，一句话会被发送两遍。认得准的：写了 `--replace` 的、它调的 `miyu` 已经不在
    // 了的（项目主人 2026-11-23 问的：旧版 Miyu 被卸掉以后它的集成还留着，终端一按回车就出错）停用它再装；`miyu` 还在、又
    // 没写 `--replace` 的不装，说清在哪、怎么办。认不准的装完提醒。
    let others = match install::elsewhere(shell, &path, &env) {
        Ok(others) => others,
        Err(error) => return failed(words, "shellhook.unreadable", &path, &error),
    };
    let take_over =
        parsed.has("replace") || !install::has_program("miyu", std::env::var_os("PATH").as_deref());
    let mut refused = false;
    // 从后往前：同一个启动文件里删掉后面的一段，前面那一段的行号不变。
    for other in others
        .iter()
        .rev()
        .filter(|other| other.kind != Other::Maybe)
    {
        if take_over {
            let from = other.path.display().to_string();
            match install::retire(other) {
                Ok(Retired::Moved(to)) => {
                    let to = to.display().to_string();
                    let fields = [("from", from.as_str()), ("to", to.as_str())];
                    println!("{}", words.say("shellhook.other.moved", &fields));
                }
                Ok(Retired::Cut) => {
                    println!("{}", words.say("shellhook.other.cut", &[("path", &from)]));
                }
                Err(error) => return failed(words, "write_failed", &other.path, &error),
            }
            continue;
        }
        let fix = match other.kind {
            Other::Miyu => words.say("shellhook.other.miyu.fix", &[("shell", shell.name())]),
            _ => words.say("shellhook.other.old.fix", &[("shell", shell.name())]),
        };
        words.error(&other_line(words, other), Some(&fix));
        refused = true;
    }
    if refused {
        return ExitCode::FAILURE;
    }
    let shown = path.display().to_string();
    let fields = [("path", shown.as_str()), ("shell", shell.name())];
    match install::install_at(shell, &path, &program) {
        Ok(Done::Installed) => println!("{}", words.say("shellhook.installed", &fields)),
        Ok(Done::Updated) => println!("{}", words.say("shellhook.updated", &fields)),
        Ok(Done::Foreign) => {
            words.error(&words.say("shellhook.foreign", &fields), None);
            return ExitCode::FAILURE;
        }
        Err(error) => return failed(words, "write_failed", &path, &error),
    }
    for other in others.iter().filter(|other| other.kind == Other::Maybe) {
        words.warn(
            &other_line(words, other),
            Some(words.get("shellhook.other.twice")),
        );
    }
    ExitCode::SUCCESS
}

/// `shellhook uninstall`。
fn uninstall(parsed: &Parsed, words: &Words) -> ExitCode {
    let (shell, path) =
        match chosen(parsed, words).and_then(|shell| Ok((shell, target(shell, words)?))) {
            Ok(found) => found,
            Err(code) => return code,
        };
    let shown = path.display().to_string();
    let fields = [("path", shown.as_str()), ("shell", shell.name())];
    match install::uninstall_at(shell, &path) {
        Ok(true) => println!("{}", words.say("shellhook.removed", &fields)),
        Ok(false) => println!("{}", words.say("shellhook.not_installed", &fields)),
        Err(error) => return failed(words, "write_failed", &path, &error),
    }
    ExitCode::SUCCESS
}

/// `shellhook status`：三种 shell 各一行；别家的集成也在的说明。
fn status(words: &Words) -> ExitCode {
    match listed(words) {
        true => ExitCode::SUCCESS,
        false => ExitCode::FAILURE,
    }
}

/// 三种 shell 各印一行（`status`、`doctor` 都用）；没有 `HOME`、启动文件读不了的说一声，交回 `false`。
fn listed(words: &Words) -> bool {
    for shell in ALL {
        let Some(path) = shell.target(&env) else {
            words.error(words.get("shellhook.no_home"), None);
            return false;
        };
        let shown = path.display().to_string();
        let mut fields = vec![("path", shown.as_str()), ("shell", shell.name())];
        let found = match install::found(shell, &path) {
            Ok(found) => found,
            Err(error) => {
                let _code = failed(words, "shellhook.unreadable", &path, &error);
                return false;
            }
        };
        let gone;
        let line = match &found {
            Found::Missing => words.say("shellhook.status.missing", &fields),
            Found::Foreign => words.say("shellhook.status.foreign", &fields),
            Found::Ours(Some(program)) if !program.is_file() => {
                gone = program.display().to_string();
                fields.push(("program", &gone));
                words.say("shellhook.status.gone", &fields)
            }
            Found::Ours(_) => words.say("shellhook.status.installed", &fields),
        };
        println!("{line}");
        let others = install::elsewhere(shell, &path, &env).unwrap_or_default();
        for other in &others {
            words.warn(
                &other_line(words, other),
                Some(words.get("shellhook.other.twice")),
            );
        }
    }
    true
}

/// `shellhook doctor`：逐条检查说话要用到的几样，有不对的说怎么办，退出码 1；都对的 0。
///
/// 查的：三种 shell 装没装（同 `status`）、当前这种 shell 装没装、有没有能用的模型、留下来的进程放不放得下套接字（放不下的
/// 没有后台回报）、有没有记下终端的颜色（没有的照 16 色画）。
fn doctor(parsed: &Parsed, words: &Words) -> ExitCode {
    use lm_engine::store::paths::{Env, Paths};
    if !listed(words) {
        return ExitCode::FAILURE;
    }
    let mut wrong = false;
    // 当前这种 shell：认不出的不查（`status` 已经三种都列了）。
    if let Some(shell) = written(parsed).or_else(shells::detect)
        && let Some(path) = shell.target(&env)
        && matches!(install::found(shell, &path), Ok(Found::Missing))
    {
        let hint = words.say("shellhook.doctor.install", &[("shell", shell.name())]);
        words.error(
            &words.say("shellhook.doctor.not_here", &[("shell", shell.name())]),
            Some(&hint),
        );
        wrong = true;
    }
    // 没有能用的模型：`model_ready` 自己说了原因和怎么办。
    match crate::cli::ask::model_ready(words) {
        Ok(()) => println!("{}", words.get("shellhook.doctor.model")),
        Err(_) => wrong = true,
    }
    let paths = Paths::locate(&Env::current()).ok();
    match paths
        .as_ref()
        .and_then(|paths| terminal::socket(paths, "0-0", true))
    {
        Some(_) => println!("{}", words.get("shellhook.doctor.reports")),
        None => words.warn(
            words.get("shellhook.doctor.no_reports"),
            Some(words.get("shellhook.doctor.no_reports.hint")),
        ),
    }
    let colors = crate::theme::record::path()
        .as_deref()
        .and_then(crate::theme::record::read);
    match colors {
        Some(_) => println!("{}", words.get("shellhook.doctor.colors")),
        None => println!("{}", words.get("shellhook.doctor.no_colors")),
    }
    match wrong {
        true => ExitCode::FAILURE,
        false => ExitCode::SUCCESS,
    }
}

/// `shellhook script --shell <名字> --program <路径>`：印这种 shell 的脚本。两个都得写。
fn print_script(parsed: &Parsed, path: &str) -> ExitCode {
    match (written(parsed), parsed.value("program")) {
        (Some(shell), Some(program)) => super::print(&script::render(shell, Path::new(program))),
        _ => misuse::tell(&Words::current(), path, &Misuse::Missing(path.to_string())),
    }
}

/// `shellhook classify`：标准输入是多行的一段，是命令退出码 0，是一句话 1；读不了的 2（脚本照命令办）。
fn classify_stdin(parsed: &Parsed) -> ExitCode {
    let mut text = String::new();
    if std::io::stdin().read_to_string(&mut text).is_err() {
        return ExitCode::from(misuse::USAGE);
    }
    match classify::is_shell_command(&text, written(parsed).unwrap_or(Shell::Fish)) {
        true => ExitCode::SUCCESS,
        false => ExitCode::FAILURE,
    }
}

#[cfg(test)]
mod tests;
