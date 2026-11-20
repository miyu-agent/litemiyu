//! 参数写错时说的话（照 Miyu `cli/main.md`「参数写错时」）：标准错误上 `litemiyu: ` 打头一行，能照着改的另起一行提示，
//! 退出码 2。

use std::process::ExitCode;

use super::parse::Misuse;
use super::words::Words;

/// 用法不对的退出码。
pub const USAGE: u8 = 2;

/// 说 `misuse`（写在命令 `path` 上，例如 `litemiyu ask`），交回退出码 2。
pub fn tell(words: &Words, path: &str, misuse: &Misuse) -> ExitCode {
    let see = words.say("see_help", &[("command", path)]);
    let (message, hint) = match misuse {
        Misuse::UnknownOption(arg, near) => (
            words.say("unknown_option", &[("arg", arg)]),
            near.as_deref()
                .map_or(see, |near| words.say("did_you_mean", &[("name", near)])),
        ),
        Misuse::NoValue(long) => (
            words.say("no_value", &[("option", &format!("--{long}"))]),
            see,
        ),
        Misuse::UnknownSub(at, name, near) => (
            words.say("unknown_sub", &[("command", at), ("name", name)]),
            near.map_or_else(
                || words.say("see_help", &[("command", at)]),
                |near| words.say("did_you_mean", &[("name", near)]),
            ),
        ),
        Misuse::Unexpected(arg) => (words.say("unexpected", &[("arg", arg)]), see),
        Misuse::Missing(at) => (
            words.get("missing").to_string(),
            words.say("see_help", &[("command", at)]),
        ),
        Misuse::Conflict(a, b) => (
            words.say(
                "conflict",
                &[("a", &format!("--{a}")), ("b", &format!("--{b}"))],
            ),
            see,
        ),
        Misuse::BadValue(long, value, values) => (
            words.say(
                "bad_value",
                &[("option", &format!("--{long}")), ("value", value)],
            ),
            words.say("possible", &[("values", &joined(words, values))]),
        ),
        Misuse::Invalid(long, value, hint) => (
            words.say(
                "bad_value",
                &[("option", &format!("--{long}")), ("value", value)],
            ),
            words.get(hint).to_string(),
        ),
        Misuse::Needs(a, b) => (
            words.say(
                "needs",
                &[("a", &format!("--{a}")), ("b", &format!("--{b}"))],
            ),
            see,
        ),
        Misuse::NotUtf8 => (words.get("not_utf8_arg").to_string(), see),
    };
    words.error(&message, Some(&hint));
    ExitCode::from(USAGE)
}

/// 不认识的子命令（最外面那一层）：`litemiyu: 未知命令 '<名字>'`，有最接近的提示它，没有的提示看帮助；退出码 2。
pub fn unknown_command(words: &Words, name: &str, near: Option<&str>) -> ExitCode {
    let hint = near.map_or_else(
        || words.say("see_help", &[("command", "litemiyu")]),
        |near| words.say("did_you_mean", &[("name", near)]),
    );
    words.error(
        &words.say("unknown_command", &[("name", name)]),
        Some(&hint),
    );
    ExitCode::from(USAGE)
}

/// 几个名字连起来：两个用「或」，三个以上前面用顿号（英文逗号），最后一个前面用「或」（Miyu 的连法）。
pub fn joined(words: &Words, names: &[&str]) -> String {
    let or = words.get("or");
    let comma = words.get("comma");
    match names {
        [] => String::new(),
        [one] => (*one).to_string(),
        [init @ .., last] => format!("{}{or}{last}", init.join(comma)),
    }
}
