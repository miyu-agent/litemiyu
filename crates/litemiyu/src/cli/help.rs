//! 帮助页（照 Miyu `cli/main.md`「帮助页」）：从那张表生成，一条命令一页，跟着界面语言。用法一行，说明一段，子命令、选项
//! 各一节（说明从同一列开始，中文字算两列），末尾那一段（退出状态、例子）。

use unicode_width::UnicodeWidthStr;

use super::spec::{Cmd, HELP, Opt};
use super::words::Words;

/// 说明最多从第几列开始：前面那一格长过它的，说明另起一行从这一列开始。
const COLUMN: usize = 28;

/// 命令 `cmd`（全名 `path`，例如 `litemiyu ask`）的那一页，以一个换行结尾。
pub fn page(words: &Words, path: &str, cmd: &Cmd) -> String {
    let mut out = String::new();
    let usage = words.get(cmd.usage);
    out.push_str(&format!("{}{path}", words.get("usage")));
    if !cmd.usage.is_empty() && !usage.is_empty() {
        out.push(' ');
        out.push_str(usage);
    }
    out.push_str("\n\n");
    let about = if cmd.about.is_empty() {
        cmd.summary
    } else {
        cmd.about
    };
    out.push_str(words.get(about));
    out.push('\n');
    if !cmd.subs.is_empty() {
        out.push('\n');
        out.push_str(words.get("commands"));
        out.push('\n');
        let rows: Vec<(String, &str)> = cmd
            .subs
            .iter()
            .map(|sub| {
                let tail = words.get(sub.usage);
                let label = match sub.usage.is_empty() || tail.is_empty() {
                    true => sub.name.to_string(),
                    false => format!("{} {tail}", sub.name),
                };
                (label, words.get(sub.summary))
            })
            .collect();
        table(&mut out, &rows);
    }
    out.push('\n');
    out.push_str(words.get("options"));
    out.push('\n');
    let rows: Vec<(String, &str)> = cmd
        .opts
        .iter()
        .chain(std::iter::once(&HELP))
        .map(|opt| (label(words, opt), words.get(opt.help)))
        .collect();
    table(&mut out, &rows);
    if !cmd.extra.is_empty() {
        out.push('\n');
        out.push_str(words.get(cmd.extra));
        out.push('\n');
    }
    out
}

/// 一个选项那一格：`-s, --session <编号>`，没有短的空四格。
pub fn label(words: &Words, opt: &Opt) -> String {
    let short = opt
        .short
        .map_or_else(|| "    ".to_string(), |short| format!("-{short}, "));
    let value = opt
        .value
        .map(|value| format!(" {}", words.get(value)))
        .unwrap_or_default();
    format!("{short}--{}{value}", opt.long)
}

/// 两列的表：缩进两格，说明从同一列开始；前一格太长的说明另起一行。
fn table(out: &mut String, rows: &[(String, &str)]) {
    let widest = rows
        .iter()
        .map(|(label, _)| label.width())
        .filter(|width| width + 4 <= COLUMN)
        .max()
        .unwrap_or(0);
    let column = widest + 4;
    for (label, text) in rows {
        let width = label.width() + 2;
        out.push_str("  ");
        out.push_str(label);
        if width + 2 > column {
            out.push('\n');
            out.push_str(&" ".repeat(column));
        } else {
            out.push_str(&" ".repeat(column - width));
        }
        out.push_str(text);
        out.push('\n');
    }
}
