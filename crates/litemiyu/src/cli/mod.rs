//! 命令行：读启动参数。只有四样（LM-3，项目主人拍板）：不带子命令的进全屏，`ask` 当场办完、交回
//! 退出码（不进全屏、不碰终端），`config` 进配置界面，`help`（和 `--help`、`--version`）印了就退出。原来照 Miyu 搬的
//! `provider`、`models`、`skills`、`mcp`、`session`、`rename`、`compact`、`doctor`、`completions` 和全屏里的界面重复，删了；
//! 再敲的照不认识的命令说（退出码 2）。
//!
//! 命令一张表（[`ROOT`]，`spec.rs`）：解析、帮助页都照它。参数的名字、输出的格式、退出码照 Miyu 的命令行（`miyu-cli`）；底下是
//! LiteMiyu 自己的引擎（`ask` 走全屏同一个引擎线程，`core::Core`）。

mod ask;
mod export;
mod help;
mod misuse;
mod parse;
mod spec;
mod words;

use std::io::Write;
use std::process::ExitCode;

use parse::Misuse;
use spec::{Cmd, Opt, nearest};
use words::Words;

pub use export::write as export_session;
pub use words::clean;

/// 不带子命令时的选项（进全屏的那几样）。
const ROOT_OPTS: &[Opt] = &[
    Opt::value("session", Some('s'), "v.id", "o.root.session"),
    Opt::flag("continue", Some('c'), "o.root.continue"),
    Opt::flag("bare", None, "o.root.bare"),
    Opt::flag("rescue", None, "o.root.rescue"),
    Opt::value("base-url", None, "v.url", "o.root.base_url"),
    Opt::value("model", None, "v.name", "o.root.model"),
    Opt::value("api", None, "v.api", "o.root.api"),
    Opt::flag("version", Some('V'), "o.version"),
];

/// 整张表。
pub static ROOT: Cmd = Cmd {
    name: "litemiyu",
    usage: "u.root",
    summary: "s.root",
    about: "a.root",
    extra: "x.root",
    opts: ROOT_OPTS,
    subs: &[
        ask::CMD,
        CONFIG,
        Cmd {
            name: "help",
            usage: "u.help",
            summary: "s.help",
            about: "",
            extra: "",
            opts: &[],
            subs: &[],
            run: None,
        },
    ],
    run: None,
};

/// `litemiyu config`：只写它进配置界面，只认 `-h`、`--help`（`start` 里办，没有处理函数）。
const CONFIG: Cmd = Cmd {
    name: "config",
    usage: "",
    summary: "s.config",
    about: "a.config",
    extra: "",
    opts: &[],
    subs: &[],
    run: None,
};

/// 进全屏要的：应急模式的参数、`--bare`、`config`、开哪个旧会话。
#[derive(Debug, Default)]
pub struct Fullscreen {
    /// 应急模式（`--rescue`、`--base-url`、`--model`、`--api`）。
    pub rescue: crate::rescue::Args,
    /// 第一个会话开无功能的那一档。
    pub bare: bool,
    /// 直接进配置界面（`litemiyu config`）。
    pub config: bool,
    /// 直接打开这个旧会话（`-s`、`-c`，开之前查过在、没被别的窗口开着）。
    pub open: Option<String>,
}

/// 读完启动参数以后怎么办。
pub enum Start {
    /// 办完了（子命令、`--help`、`--version`、参数不对）：这个退出码退出。
    Done(ExitCode),
    /// 进全屏。
    Fullscreen(Fullscreen),
}

/// 读启动参数（`std::env::args_os`），`ask`、`help` 当场办完。`--version` 连配置都不读。
pub fn start() -> Start {
    let mut args = Vec::new();
    for arg in std::env::args_os().skip(1) {
        match arg.into_string() {
            Ok(arg) => args.push(arg),
            Err(_) => {
                let words = Words::current();
                return Start::Done(misuse::tell(&words, "litemiyu", &Misuse::NotUtf8));
            }
        }
    }
    match args.first().map(String::as_str) {
        Some("config") => config(&args[1..]),
        Some("help") => Start::Done(help_command(&args[1..])),
        Some(first) if !first.starts_with('-') => {
            let words = Words::current();
            match ROOT.sub(first) {
                Some(cmd) => {
                    Start::Done(run(cmd, &format!("litemiyu {first}"), &args[1..], &words))
                }
                None => {
                    let near = nearest(first, ROOT.subs.iter().map(|cmd| cmd.name));
                    Start::Done(misuse::unknown_command(&words, first, near))
                }
            }
        }
        _ => fullscreen(&args),
    }
}

/// 办命令 `cmd`（全名 `path`）：`-h`、`--help` 印那一页。
fn run(cmd: &Cmd, path: &str, args: &[String], words: &Words) -> ExitCode {
    let parsed = match parse::read(cmd, args) {
        Ok(parsed) => parsed,
        Err(bad) => return misuse::tell(words, path, &bad),
    };
    if parsed.has("help") {
        return print(&help::page(words, path, cmd));
    }
    match cmd.run {
        Some(handler) => handler(parsed, words),
        // 表里没有处理函数的只有 `config`、`help`，都在 `start` 里办，走不到这里。
        None => misuse::tell(words, path, &Misuse::Missing(path.to_string())),
    }
}

/// `litemiyu config [-h]`：只写它的进配置界面；`-h` 印帮助；别的（多写了词、不认识的选项）照参数不对说、退出码 2，不进全屏。
fn config(args: &[String]) -> Start {
    if args.is_empty() {
        return Start::Fullscreen(Fullscreen {
            config: true,
            ..Fullscreen::default()
        });
    }
    let words = Words::current();
    let parsed = match parse::read(&CONFIG, args) {
        Ok(parsed) => parsed,
        Err(bad) => return Start::Done(misuse::tell(&words, "litemiyu config", &bad)),
    };
    if parsed.has("help") {
        return Start::Done(print(&help::page(&words, "litemiyu config", &CONFIG)));
    }
    if let Some(word) = parsed.words.first() {
        let bad = Misuse::Unexpected(word.clone());
        return Start::Done(misuse::tell(&words, "litemiyu config", &bad));
    }
    // 只写了 `--` 的：照只写 `config`。
    Start::Fullscreen(Fullscreen {
        config: true,
        ..Fullscreen::default()
    })
}

/// `litemiyu help [<命令> [<子命令>…]]`：印那一页。
fn help_command(names: &[String]) -> ExitCode {
    let words = Words::current();
    let mut cmd = &ROOT;
    let mut path = "litemiyu".to_string();
    for name in names {
        let Some(sub) = cmd.sub(name) else {
            let near = nearest(name, cmd.subs.iter().map(|cmd| cmd.name));
            return match cmd.name {
                "litemiyu" => misuse::unknown_command(&words, name, near),
                _ => misuse::tell(
                    &words,
                    &path,
                    &Misuse::UnknownSub(path.clone(), name.clone(), near),
                ),
            };
        };
        path = format!("{path} {name}");
        cmd = sub;
    }
    print(&help::page(&words, &path, cmd))
}

/// 不带子命令：读进全屏的那几样；`--help`、`--version` 印了就退出。命令行的字（[`Words`]）只在要说话时读：照常进全屏的
/// 一句都不读，不为子命令多占常驻内存。
fn fullscreen(args: &[String]) -> Start {
    let parsed = match parse::read(&ROOT, args) {
        Ok(parsed) => parsed,
        Err(bad) => return Start::Done(misuse::tell(&Words::current(), "litemiyu", &bad)),
    };
    // `--version` 照旧在读配置以前：整行读得了才算（`--version --nope` 照不认识报）。
    if parsed.has("version") && parsed.words.is_empty() {
        return Start::Done(print(&format!("litemiyu {}\n", env!("CARGO_PKG_VERSION"))));
    }
    if let Some(word) = parsed.words.first() {
        // 应急模式不写盘，没有配置界面（`config` 只认排头的那一个，写在 `--rescue` 后面的走到这里）：照实说，不说成「未知命令
        // 'config'，是不是 'config'」。退出码照参数不对的。
        if word == "config" && parsed.has("rescue") {
            let words = Words::current();
            let hint = words.say("see_help", &[("command", "litemiyu")]);
            words.error(words.get("rescue_no_config"), Some(&hint));
            return Start::Done(ExitCode::from(misuse::USAGE));
        }
        let near = nearest(word, ROOT.subs.iter().map(|cmd| cmd.name));
        return Start::Done(misuse::unknown_command(&Words::current(), word, near));
    }
    if parsed.has("help") {
        return Start::Done(print(&help::page(&Words::current(), "litemiyu", &ROOT)));
    }
    let groups: &[&[&str]] = &[
        &["session", "continue"],
        &["rescue", "session"],
        &["rescue", "continue"],
    ];
    if let Err(bad) = parse::exclusive(&parsed, groups) {
        return Start::Done(misuse::tell(&Words::current(), "litemiyu", &bad));
    }
    let open = match pick_session(&parsed) {
        Ok(open) => open,
        Err(code) => return Start::Done(code),
    };
    let value = |long| parsed.value(long).map(str::to_string);
    Start::Fullscreen(Fullscreen {
        rescue: crate::rescue::Args::given(
            parsed.has("rescue"),
            value("base-url"),
            value("model"),
            value("api"),
        ),
        bare: parsed.has("bare"),
        config: false,
        open,
    })
}

/// 进全屏时要打开的旧会话（`-s`、`-c`）：先照会话目录查在不在、有没有窗口开着，不在的、开着的照实说、退出码 1，不进全屏。
fn pick_session(parsed: &parse::Parsed) -> Result<Option<String>, ExitCode> {
    if !parsed.has("session") && !parsed.has("continue") {
        return Ok(None);
    }
    let words = Words::current();
    match ask::sessions::pick(&words, parsed.value("session")) {
        Ok(id) => Ok(Some(id)),
        Err(not) => {
            words.error(&not.message, None);
            Err(ExitCode::FAILURE)
        }
    }
}

/// 印到标准输出，交回 0；标准输出那头关了的（`| head`）照样 0，不崩。
fn print(text: &str) -> ExitCode {
    let mut out = std::io::stdout().lock();
    // 写不进的（那头关了）没处可报。
    drop(out.write_all(text.as_bytes()).and_then(|()| out.flush()));
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests;
