//! 命令行的那张表：子命令、选项、说明的编号、处理函数。解析（`parse.rs`）、帮助页
//! （`help.rs`）、补全（`completions.rs`）都照它，不另写一份。

use std::process::ExitCode;

use super::parse::Parsed;
use super::words::Words;

/// 一个选项。
#[derive(Debug)]
pub struct Opt {
    /// 长的写法，不带 `--`。
    pub long: &'static str,
    /// 短的写法，不带 `-`。
    pub short: Option<char>,
    /// 要值的：值名的编号（帮助页上写成 `<编号>` 那一格，例如 `v.id`）。不要值的没有。
    pub value: Option<&'static str>,
    /// 说明的编号。
    pub help: &'static str,
}

impl Opt {
    /// 不要值的。
    pub const fn flag(long: &'static str, short: Option<char>, help: &'static str) -> Opt {
        Opt {
            long,
            short,
            value: None,
            help,
        }
    }

    /// 要值的。
    pub const fn value(
        long: &'static str,
        short: Option<char>,
        value: &'static str,
        help: &'static str,
    ) -> Opt {
        Opt {
            long,
            short,
            value: Some(value),
            help,
        }
    }
}

/// 办一条命令：读好的参数、给人看的字，交回退出码。
pub type Handler = fn(Parsed, &Words) -> ExitCode;

/// 一个子命令。
#[derive(Debug)]
pub struct Cmd {
    /// 名字。
    pub name: &'static str,
    /// 用法里接在名字后面的那一截的编号（`[<选项>] [<消息>…]`）；空的不写。
    pub usage: &'static str,
    /// 一行说明的编号（总表、上一层的命令表、补全里用）。
    pub summary: &'static str,
    /// 帮助页上用法下面那一段的编号；空的照一行说明。
    pub about: &'static str,
    /// 帮助页末尾多的那一段（退出状态、例子）的编号；空的不写。
    pub extra: &'static str,
    /// 选项（`-h`、`--help` 每条都有，不写在这里）。
    pub opts: &'static [Opt],
    /// 下面的子命令：有的，第一个词得是其中一个。
    pub subs: &'static [Cmd],
    /// 办它的：有子命令的没有。
    pub run: Option<Handler>,
}

/// 帮助页上每条都有的那一个选项。
pub const HELP: Opt = Opt::flag("help", Some('h'), "o.help");

impl Cmd {
    /// 名字是 `name` 的子命令。
    pub fn sub(&self, name: &str) -> Option<&'static Cmd> {
        self.subs.iter().find(|cmd| cmd.name == name)
    }

    /// 长的写法是 `long` 的选项（连 `--help`）。
    pub fn opt(&self, long: &str) -> Option<&Opt> {
        self.opts
            .iter()
            .chain(std::iter::once(&HELP))
            .find(|opt| opt.long == long)
    }

    /// 短的写法是 `short` 的选项（连 `-h`）。
    pub fn short(&self, short: char) -> Option<&Opt> {
        self.opts
            .iter()
            .chain(std::iter::once(&HELP))
            .find(|opt| opt.short == Some(short))
    }
}

/// 和 `wrong` 最接近的那个名字：编辑距离不超过 2（短名字不超过 1），或者是它的开头；没有的是 `None`。
pub fn nearest<'a>(wrong: &str, names: impl IntoIterator<Item = &'a str>) -> Option<&'a str> {
    let limit = if wrong.chars().count() <= 2 { 1 } else { 2 };
    names
        .into_iter()
        .map(|name| (distance(wrong, name), name))
        .filter(|(d, name)| *d <= limit || (wrong.len() >= 2 && name.starts_with(wrong)))
        .min_by_key(|(d, _)| *d)
        .map(|(_, name)| name)
}

/// 两个字串的编辑距离（增、删、换各算一步）。
fn distance(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut last = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let next = (last + usize::from(ca != *cb))
                .min(row[j] + 1)
                .min(row[j + 1] + 1);
            last = row[j + 1];
            row[j + 1] = next;
        }
    }
    row[b.len()]
}
