//! `litemiyu ask`：不进全屏，说一句就回（照 Miyu `cli/ask.md`）。读参数、凑话（`input.rs`）、
//! 找会话（`sessions.rs`）、看有没有能用的模型，交给 `drive.rs` 跑；三种输出在 `text.rs`、`out.rs`。

mod drive;
mod ending;
mod follow;
mod input;
mod out;
mod remote;
pub mod sessions;
mod steps;
mod text;
mod wash;

use std::process::ExitCode;

use ending::Ending;
use input::NoMessage;
use out::Output;

use super::misuse;
use super::parse::{self, Misuse, Parsed};
use super::spec::{Cmd, Opt};
use super::words::Words;

/// 表里的这一条。
pub const CMD: Cmd = Cmd {
    name: "ask",
    usage: "u.ask",
    summary: "s.ask",
    about: "a.ask",
    extra: "x.ask",
    opts: &[
        Opt::flag("continue", Some('c'), "o.ask.continue"),
        Opt::value("session", Some('s'), "v.id", "o.ask.session"),
        Opt::value("cd", Some('C'), "v.dir", "o.ask.cd"),
        Opt::value("format", None, "v.format3", "o.ask.format"),
        Opt::value("preset", None, "v.preset", "o.ask.preset"),
        Opt::flag("bare", None, "o.ask.bare"),
        Opt::flag("yes", None, "o.ask.yes"),
        Opt::value("timeout", None, "v.time", "o.ask.timeout"),
        Opt::value("max-steps", None, "v.n", "o.ask.max_steps"),
        Opt::flag("rescue", None, "o.ask.rescue"),
        Opt::value("base-url", None, "v.url", "o.root.base_url"),
        Opt::value("model", None, "v.name", "o.ask.model"),
        Opt::value("file", None, "v.file", "o.ask.file"),
        Opt::value("api", None, "v.api", "o.root.api"),
    ],
    subs: &[],
    run: Some(ask),
};

/// 三种输出。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Format {
    Text,
    Json,
    Jsonl,
}

/// 办 `litemiyu ask`。
fn ask(parsed: Parsed, words: &Words) -> ExitCode {
    let format = match parsed.value("format") {
        None | Some("text") => Format::Text,
        Some("json") => Format::Json,
        Some("jsonl") => Format::Jsonl,
        Some(other) => {
            let bad = Misuse::BadValue("format", other.to_string(), vec!["text", "json", "jsonl"]);
            return early_misuse(Format::Text, words, &bad);
        }
    };
    let mut output: Box<dyn Output + '_> = match format {
        Format::Text => Box::new(text::Text::new(words)),
        Format::Json => Box::new(out::Json::new(words)),
        Format::Jsonl => Box::new(out::Jsonl::new(words)),
    };
    let plan = match plan(&parsed, words) {
        Ok(plan) => plan,
        Err(Early::Misuse(bad)) => return early_misuse(format, words, &bad),
        Err(Early::Ended(ending)) => {
            output.finish(&ending, &follow::Follow::new(words.code()), None);
            return ExitCode::from(ending.exit);
        }
    };
    let ending = match plan.elsewhere {
        true => remote::drive(plan, words, output.as_mut()),
        false => drive::drive(plan, words, output.as_mut()),
    };
    ExitCode::from(ending.exit)
}

/// 开好会话以前就办不下去的。
enum Early {
    /// 参数不对。
    Misuse(Misuse),
    /// 别的：话已经说过了。
    Ended(Ending),
}

/// 参数不对：照 Miyu 说话（退出码 2）；`json`、`jsonl` 另在标准输出上印收尾的那一行。
fn early_misuse(format: Format, words: &Words, bad: &Misuse) -> ExitCode {
    let code = misuse::tell(words, "litemiyu ask", bad);
    if format != Format::Text {
        let ending = Ending::early(misuse::USAGE, "usage");
        let follow = follow::Follow::new(words.code());
        match format {
            Format::Json => out::Json::new(words).finish(&ending, &follow, None),
            _ => out::Jsonl::new(words).finish(&ending, &follow, None),
        }
    }
    code
}

/// 读参数、凑话、找会话、看模型，排成 [`drive::Plan`]。
fn plan(parsed: &Parsed, words: &Words) -> Result<drive::Plan, Early> {
    parse::exclusive(parsed, &[&["session", "continue"]]).map_err(Early::Misuse)?;
    parse::only_new(parsed, &["cd", "bare", "preset", "rescue"]).map_err(Early::Misuse)?;
    // 用哪个预设（同全屏的 `--preset`、`--bare`）。
    let preset = super::preset(parsed).map_err(Early::Misuse)?;
    for rescue_only in ["base-url", "api"] {
        if parsed.has(rescue_only) && !parsed.has("rescue") {
            return Err(Early::Misuse(Misuse::Needs(rescue_only, "rescue")));
        }
    }
    let timeout = match parsed.value("timeout") {
        Some(text) => Some(parse::duration(text).ok_or_else(|| {
            Early::Misuse(Misuse::Invalid(
                "timeout",
                text.to_string(),
                "hint.duration",
            ))
        })?),
        None => None,
    };
    let max_steps = match parsed.value("max-steps") {
        Some(text) => Some(parse::positive(text).ok_or_else(|| {
            Early::Misuse(Misuse::Invalid(
                "max-steps",
                text.to_string(),
                "hint.positive",
            ))
        })?),
        None => None,
    };
    // `--file`：相对的照敲命令时的目录接成绝对的（不跟着 `-C` 换，同 Miyu），不在的说清；路径照全屏 `@` 选文件的写法接在话后面。
    let files = files(parsed, words)?;
    if let Some(dir) = parsed.value("cd") {
        let path = std::path::Path::new(dir);
        if !path.is_dir() || std::env::set_current_dir(path).is_err() {
            let bad = Misuse::Invalid("cd", dir.to_string(), "hint.dir");
            return Err(Early::Misuse(bad));
        }
    }
    let message = input::message(&parsed.words).map_err(|why| match why {
        NoMessage::Missing => Early::Misuse(Misuse::Missing("litemiyu ask".to_string())),
        NoMessage::NotUtf8 => early(words, misuse::USAGE, "usage", words.get("stdin_not_utf8")),
        NoMessage::TooLong => early(words, misuse::USAGE, "usage", words.get("too_long")),
        NoMessage::Unreadable(why) => early(
            words,
            1,
            "failed",
            &words.say("stdin_unreadable", &[("reason", &words.why(&why))]),
        ),
    })?;
    let mut message = message;
    for file in &files {
        message.push(' ');
        message.push_str(&crate::input::quoted(file));
    }
    // `--model`：应急模式的是应急的模型名；平常的照配置查过，开好会话先换成它（同 Miyu `ask --model`）。
    let (rescue, model) = match parsed.has("rescue") {
        true => (Some(rescued(parsed, words)?), None),
        false => (None, ready(words, parsed.value("model"))?),
    };
    let (session, elsewhere) = match (parsed.value("session"), parsed.has("continue")) {
        (None, false) => (None, false),
        (id, _) => {
            let (id, open) = sessions::pick_even_open(words, id)
                .map_err(|not| early(words, 1, not.reason, &not.message))?;
            (Some(id), open)
        }
    };
    Ok(drive::Plan {
        elsewhere,
        session,
        message,
        yes: parsed.has("yes"),
        preset,
        rescue,
        timeout,
        max_steps,
        model,
    })
}

/// `--file` 写的几个文件：接成绝对路径，照写的先后、不去重；不在的、不是文件的说清，退出码 1，不开会话。
fn files(parsed: &Parsed, words: &Words) -> Result<Vec<std::path::PathBuf>, Early> {
    let here = std::env::current_dir().unwrap_or_default();
    let mut files = Vec::new();
    for written in parsed.values("file") {
        let path = here.join(written);
        if !path.is_file() {
            return Err(early(
                words,
                1,
                "failed",
                &words.say("no_file", &[("path", written)]),
            ));
        }
        files.push(path);
    }
    Ok(files)
}

/// 说一句错，交回收尾。
fn early(words: &Words, exit: u8, reason: &str, message: &str) -> Early {
    words.error(message, None);
    Early::Ended(Ending::early(exit, reason))
}

/// 应急模式：照命令行、环境变量备好（同 `litemiyu --rescue`）；没有能用的模型的退出码 5。
fn rescued(parsed: &Parsed, words: &Words) -> Result<crate::rescue::Prepared, Early> {
    let value = |long| parsed.value(long).map(str::to_string);
    let args = crate::rescue::Args::given(true, value("base-url"), value("model"), value("api"));
    crate::rescue::prepare(&args, |name| std::env::var(name).ok()).map_err(|why| match why {
        crate::rescue::NotReady::Missing(names, _) => {
            let missing = names.join(", ");
            early(
                words,
                5,
                "no_model",
                &words.say("rescue_missing", &[("missing", &missing)]),
            )
        }
        crate::rescue::NotReady::Bad(name) => early(
            words,
            misuse::USAGE,
            "usage",
            &words.say("rescue_bad", &[("name", name)]),
        ),
    })
}

/// 有没有能用的模型（Miyu `ask` 说话之前先看 `models.chat`）：配置读不了的退出码 1，没配模型的 5；写了 `--model` 的照配置查
/// （`lm_kernel::models::reference::record`），查不过的退出码 1，交回记下的写法。
fn ready(words: &Words, model: Option<&str>) -> Result<Option<String>, Early> {
    use lm_engine::store::paths::{Env, Paths};
    let paths = Paths::locate(&Env::current())
        .map_err(|error| early(words, 1, "failed", &words.why(&error.to_string())))?;
    let loaded =
        lm_engine::configured::load(&paths, |name| std::env::var(name).ok()).map_err(|error| {
            let reason = words.why(&error.to_string());
            let message = words.say("config_unreadable", &[("reason", &reason)]);
            early(words, 1, "failed", &message)
        })?;
    let values = loaded.resolved.values();
    if let Some(model) = model {
        return lm_kernel::models::reference::record(&values, model)
            .map(Some)
            .map_err(|why| {
                let message = words.say("unknown_model", &[("model", model), ("reason", &why.0)]);
                early(words, 1, "unknown_model", &message)
            });
    }
    if lm_kernel::models::provider::chat(&values).is_none() {
        let hint = words.get("no_model.hint");
        words.error(words.get("no_model"), Some(hint));
        return Err(Early::Ended(Ending::early(5, "no_model")));
    }
    Ok(None)
}

pub use follow::Turn;

/// 从日志读出的一个会话（`/export`）：编号、标题、几轮。
pub struct Replayed {
    /// 编号。
    pub id: String,
    /// 标题：没起的是空的。
    pub title: String,
    /// 读出的几轮，照先后。
    pub turns: Vec<Turn>,
}

/// 读会话 `id` 的日志（只读，开着的也读得了），照打开旧会话时补发的那一套读成推送（`core::push::replay`），再照 `ask` 的账
/// 记成一轮轮；给人看的字照 `language`。
///
/// # Errors
///
/// 数据目录找不到、日志读不了：原话。
pub fn replay(id: &str, title: String, language: &str) -> Result<Replayed, String> {
    use lm_engine::store::paths::{Env, Paths};
    struct Quiet;
    impl follow::Sink for Quiet {
        fn put(&mut self, _out: follow::Out<'_>) {}
    }
    let paths = Paths::locate(&Env::current()).map_err(|error| error.to_string())?;
    let events = lm_engine::store::log::read_events(&paths.sessions().join(id))
        .map_err(|error| error.to_string())?;
    let mut replay = crate::core::push::replay::Replay::default();
    let mut follow = follow::Follow::new(language);
    for event in events {
        for push in replay.read(event) {
            follow.push(push, &mut Quiet);
        }
    }
    for push in replay.finish() {
        follow.push(push, &mut Quiet);
    }
    Ok(Replayed {
        id: id.to_string(),
        title,
        turns: follow.turns,
    })
}
