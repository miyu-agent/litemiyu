//! 照一条命令的表读参数（`spec.rs`）：`--长[=值]`、`--长 值`、`-短 值`、`-短值`；`--` 以后的都是词；单独一个 `-` 是词
//! （`ask` 的「读标准输入」）。读不成的照 Miyu `cli/main.md`「参数写错时」分几种（[`Misuse`]），由 `misuse.rs` 说话。

use std::collections::BTreeMap;

use super::spec::{Cmd, nearest};

/// 读好的参数。
#[derive(Debug, Default)]
pub struct Parsed {
    /// 选项（长的写法）到写的值，写几次记几次；不要值的记空的。
    opts: BTreeMap<&'static str, Vec<String>>,
    /// 词，照先后。
    pub words: Vec<String>,
}

impl Parsed {
    /// 写了这个选项没有。
    pub fn has(&self, long: &str) -> bool {
        self.opts.contains_key(long)
    }

    /// 这个选项每一次写的值，照先后。
    pub fn values(&self, long: &str) -> &[String] {
        self.opts.get(long).map_or(&[], Vec::as_slice)
    }

    /// 这个选项最后一次写的值。
    pub fn value(&self, long: &str) -> Option<&str> {
        self.opts.get(long)?.last().map(String::as_str)
    }
}

/// 读不成。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Misuse {
    /// 不认识的选项：原样，最接近的那个（带 `--`）。
    UnknownOption(String, Option<String>),
    /// 选项后面没写值：长的写法。
    NoValue(&'static str),
    /// `help` 后面写的命令不认识（`litemiyu help ask x`）：那一层的全名、写的、最接近的。
    UnknownSub(String, String, Option<&'static str>),
    /// 多余的词。
    Unexpected(String),
    /// 缺了必写的：那一条命令的全名。
    Missing(String),
    /// 两个选项不能一起写（长的写法）。
    Conflict(&'static str, &'static str),
    /// 值不认识：选项、写的值、能写的几个。
    BadValue(&'static str, String, Vec<&'static str>),
    /// 值不认识，能写的几个是现查的（`--preset`：这一刻配置里的预设）：选项、写的值、能写的几个。
    BadChoice(&'static str, String, Vec<String>),
    /// 值写得不对（不是时长、不是正整数、不是目录……）：选项、写的值、提示的编号。
    Invalid(&'static str, String, &'static str),
    /// 这个选项要和另一个一起写（长的写法）：`--base-url` 要 `--rescue`。
    Needs(&'static str, &'static str),
    /// 不是 UTF-8 的参数。
    NotUtf8,
}

/// 照 `cmd` 读 `args`（子命令名字以后的那些）。
///
/// # Errors
///
/// 见 [`Misuse`]。
pub fn read(cmd: &Cmd, args: &[String]) -> Result<Parsed, Misuse> {
    let mut parsed = Parsed::default();
    let mut rest = args.iter();
    let mut words_only = false;
    while let Some(arg) = rest.next() {
        if words_only || arg == "-" || !arg.starts_with('-') {
            parsed.words.push(arg.clone());
            continue;
        }
        if arg == "--" {
            words_only = true;
            continue;
        }
        let (opt, inline) = if let Some(long) = arg.strip_prefix("--") {
            let (name, inline) = match long.split_once('=') {
                Some((name, value)) => (name, Some(value.to_string())),
                None => (long, None),
            };
            let Some(opt) = cmd.opt(name) else {
                return Err(unknown(cmd, arg));
            };
            (opt, inline)
        } else {
            let mut chars = arg[1..].chars();
            let Some(opt) = chars.next().and_then(|short| cmd.short(short)) else {
                return Err(unknown(cmd, arg));
            };
            let tail: String = chars.collect();
            (opt, (!tail.is_empty()).then_some(tail))
        };
        let value = match (opt.value, inline) {
            // 不要值的写成 `--bare=1`：照不认识报（同上一趟 `--version` 定的）。
            (None, Some(_)) => return Err(unknown(cmd, arg)),
            (None, None) => String::new(),
            (Some(_), Some(value)) => value,
            (Some(_), None) => rest.next().cloned().ok_or(Misuse::NoValue(opt.long))?,
        };
        parsed.opts.entry(opt.long).or_default().push(value);
    }
    Ok(parsed)
}

/// 不认识的选项，带最接近的那个。
fn unknown(cmd: &Cmd, arg: &str) -> Misuse {
    let name = arg.trim_start_matches('-');
    let name = name.split_once('=').map_or(name, |(name, _)| name);
    let near = nearest(name, cmd.opts.iter().map(|opt| opt.long)).map(|long| format!("--{long}"));
    Misuse::UnknownOption(arg.to_string(), near)
}

/// 照「只能写一个」的几组查：`groups` 里每一组写了两个以上的，交回头两个。
///
/// # Errors
///
/// 同一组写了两个。
pub fn exclusive(parsed: &Parsed, groups: &[&[&'static str]]) -> Result<(), Misuse> {
    for group in groups {
        let mut written = group.iter().filter(|long| parsed.has(long));
        if let (Some(a), Some(b)) = (written.next(), written.next()) {
            return Err(Misuse::Conflict(a, b));
        }
    }
    Ok(())
}

/// 照「这几个只管新会话」查：`news` 里写了的和 `-s`、`-c` 一起写的报不能一起写。
///
/// # Errors
///
/// 只管新会话的和接着说的写在一起。
pub fn only_new(parsed: &Parsed, news: &[&'static str]) -> Result<(), Misuse> {
    for old in ["session", "continue"] {
        if !parsed.has(old) {
            continue;
        }
        if let Some(new) = news.iter().find(|long| parsed.has(long)) {
            return Err(Misuse::Conflict(new, old));
        }
    }
    Ok(())
}

/// 时长：正整数，后面可以跟 `s`、`m`、`h`，不写是秒（照 GNU `timeout`，Miyu `ask --timeout`）。0、负数、小数、别的单位是 `None`。
pub fn duration(text: &str) -> Option<std::time::Duration> {
    let (digits, unit) = match text.char_indices().last()? {
        (at, 's') => (&text[..at], 1),
        (at, 'm') => (&text[..at], 60),
        (at, 'h') => (&text[..at], 3600),
        _ => (text, 1),
    };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let n: u64 = digits.parse().ok()?;
    (n > 0).then(|| std::time::Duration::from_secs(n.saturating_mul(unit)))
}

/// 正整数（`--max-steps`）。
pub fn positive(text: &str) -> Option<u64> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok().filter(|n| *n > 0)
}
