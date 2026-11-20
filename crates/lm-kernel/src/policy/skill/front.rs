//! 技能开头 `---` 之间那一段的读法（施工 LM-1，2026-11-18 换掉 yaml-rust2）：只认技能开头用得到的那一小块 YAML。
//!
//! - 顶格的 `键: 值`；往里缩的表（`metadata` 这种，最多套 [`MAX_DEPTH`] 层）；`- ` 开头的列表（和键对齐或往里缩都行）；
//!   一行写完的 `[…]` 列表。
//! - 值：不加引号的一句（往里缩的下一行接着写的并成一句）、单引号（`''` 是一个 `'`）或双引号（认 `\n`、`\"`、`\uXXXX`
//!   这些转义）的一句、`|` 照原样的一段、`>` 折成一行的一段（都可以带 `-`、`+`）。`#` 前面是空白的是注释。
//! - 不加引号的值照 YAML 1.2 核心的规矩认成字、是非、数、空，同 yaml-rust2 0.13 的 `Yaml::from_str`。
//!
//! 认不得的写法（锚点、别名、标签、`{…}`、跨行的引号、`?` 复杂的键、列表里套表、制表符缩进、重复的键……）报读不了、说出
//! 是 `SKILL.md` 的第几行，不猜着读。和 yaml-rust2 对照的测试在 `skill/tests.rs`。

use std::fmt;

/// 表最多套几层（`metadata` 是第二层）。
const MAX_DEPTH: usize = 4;

/// 开头那一段在 `SKILL.md` 里从第几行起：第 1 行是 `---`。
const FIRST_LINE: usize = 2;

/// 读出来的一个值。
#[derive(Debug, Clone, PartialEq)]
pub(super) enum Value {
    /// 字。
    Text(String),
    /// 是非：`true`、`false`（也认首字母大写、全大写的）。
    Bool(bool),
    /// 整数、小数：技能开头不用它的值。
    Number,
    /// 空：没写值、`~`、`null`。
    Null,
    /// 列表。
    List(Vec<Value>),
    /// 表：键也是值（`1: x` 的键是数）。
    Map(Vec<(Value, Value)>),
}

impl Value {
    /// 表里键是 `key`（字）的那一格；不是表、没有这一格的没有。
    pub(super) fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Map(entries) => entries
                .iter()
                .find(|(k, _)| matches!(k, Value::Text(text) if text == key))
                .map(|(_, value)| value),
            _ => None,
        }
    }

    /// 是字的：那句字。
    pub(super) fn as_str(&self) -> Option<&str> {
        match self {
            Value::Text(text) => Some(text),
            _ => None,
        }
    }

    /// 是是非的：那个是非。
    pub(super) fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(on) => Some(*on),
            _ => None,
        }
    }
}

/// 读不了：`SKILL.md` 的第几行、为什么。
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Unreadable {
    /// `SKILL.md` 的第几行（1 起，第 1 行是开头的 `---`）。
    pub line: usize,
    /// 为什么。
    pub why: &'static str,
}

impl fmt::Display for Unreadable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.why)
    }
}

/// 读开头那一段（`---` 之间的字）：顶层是表；什么都没写（只有空行、注释）的是 [`Value::Null`]。
///
/// # Errors
///
/// 认不得的写法、不合 YAML 的写法：[`Unreadable`]。
pub(super) fn parse(front: &str) -> Result<Value, Unreadable> {
    let lines: Vec<Line<'_>> = front
        .lines()
        .enumerate()
        .map(|(at, text)| Line::new(at + FIRST_LINE, text))
        .collect::<Result<_, _>>()?;
    let mut reader = Reader { lines, at: 0 };
    reader.skip_blank();
    if reader.at == reader.lines.len() {
        return Ok(Value::Null);
    }
    let first = &reader.lines[reader.at];
    if first.indent != 0 {
        return Err(first.unreadable("the first key is indented"));
    }
    let map = reader.map(0, 1)?;
    reader.skip_blank();
    match reader.lines.get(reader.at) {
        Some(line) => Err(line.unreadable("unexpected indentation")),
        None => Ok(map),
    }
}

/// 一行：第几行、缩进几格、缩进后面的字、整行。
struct Line<'a> {
    number: usize,
    indent: usize,
    text: &'a str,
    raw: &'a str,
}

impl<'a> Line<'a> {
    fn new(number: usize, raw: &'a str) -> Result<Line<'a>, Unreadable> {
        let text = raw.trim_start_matches(' ');
        let line = Line {
            number,
            indent: raw.len() - text.len(),
            text,
            raw,
        };
        // 缩进里的制表符 YAML 不认；只有空白的行不管。
        if text.starts_with('\t') && !text.trim().is_empty() {
            return Err(line.unreadable("tab in indentation"));
        }
        Ok(line)
    }

    /// 空行、只有注释的行。
    fn blank(&self) -> bool {
        let text = self.text.trim_start();
        text.is_empty() || text.starts_with('#')
    }

    /// 是列表的一项：`-` 后面是空白或者行尾。
    fn item(&self) -> bool {
        self.text == "-" || self.text.starts_with("- ") || self.text.starts_with("-\t")
    }

    fn unreadable(&self, why: &'static str) -> Unreadable {
        Unreadable {
            line: self.number,
            why,
        }
    }
}

struct Reader<'a> {
    lines: Vec<Line<'a>>,
    at: usize,
}

impl<'a> Reader<'a> {
    fn skip_blank(&mut self) {
        while self.lines.get(self.at).is_some_and(Line::blank) {
            self.at += 1;
        }
    }

    /// 下一个不是空行、注释的行（不挪）。
    fn peek(&mut self) -> Option<&Line<'a>> {
        self.skip_blank();
        self.lines.get(self.at)
    }

    /// 缩进 `indent` 格的一张表，第 `depth` 层。
    fn map(&mut self, indent: usize, depth: usize) -> Result<Value, Unreadable> {
        if depth > MAX_DEPTH {
            let line = &self.lines[self.at];
            return Err(line.unreadable("maps nested too deep"));
        }
        let mut entries: Vec<(Value, Value)> = Vec::new();
        while let Some(line) = self.peek() {
            if line.indent < indent {
                break;
            }
            if line.indent > indent {
                return Err(line.unreadable("unexpected indentation"));
            }
            if line.item() {
                return Err(line.unreadable("a list item where a key was expected"));
            }
            let number = line.number;
            let (key, rest) = split_key(line)?;
            let key = resolve(key);
            if entries.iter().any(|(k, _)| *k == key) {
                return Err(Unreadable {
                    line: number,
                    why: "duplicated key",
                });
            }
            self.at += 1;
            let value = if rest.is_empty() || rest.starts_with('#') {
                self.nested(indent, depth, number)?
            } else {
                self.scalar(rest, indent, number)?
            };
            entries.push((key, value));
        }
        Ok(Value::Map(entries))
    }

    /// `键:` 后面没写值的：下面缩进的表、列表（往里缩或者和键对齐），都没有的是空。
    fn nested(&mut self, indent: usize, depth: usize, _number: usize) -> Result<Value, Unreadable> {
        let Some(next) = self.peek() else {
            return Ok(Value::Null);
        };
        let (next_indent, item) = (next.indent, next.item());
        if item && next_indent >= indent {
            self.list(next_indent)
        } else if next_indent > indent {
            self.map(next_indent, depth + 1)
        } else {
            Ok(Value::Null)
        }
    }

    /// 缩进 `indent` 格的 `- ` 列表：每一项是一句（不认列表里套表、套列表）。
    fn list(&mut self, indent: usize) -> Result<Value, Unreadable> {
        let mut items = Vec::new();
        while let Some(line) = self.peek() {
            if line.indent != indent || !line.item() {
                if line.indent > indent {
                    return Err(line.unreadable("unexpected indentation"));
                }
                break;
            }
            let number = line.number;
            let rest = line.text[1..].trim_start();
            if rest.is_empty() || rest.starts_with('#') {
                return Err(line.unreadable("nested lists and maps in a list are not supported"));
            }
            if split_key_at(rest).is_some() && !opens_quote(rest) {
                return Err(line.unreadable("maps in a list are not supported"));
            }
            self.at += 1;
            items.push(self.scalar(rest, indent, number)?);
        }
        Ok(Value::List(items))
    }

    /// 写在 `键: ` 后面（或 `- ` 后面）的值 `rest`，那一行缩进 `indent` 格、是第 `number` 行；往下接着写的行也读进来。
    fn scalar(&mut self, rest: &str, indent: usize, number: usize) -> Result<Value, Unreadable> {
        let at = Unreadable {
            line: number,
            why: "",
        };
        let fail = |why| Unreadable { why, ..at.clone() };
        match rest.chars().next() {
            Some('"') => {
                let (text, after) = double_quoted(&rest[1..])
                    .ok_or_else(|| fail("quoted text must end on the same line"))?;
                only_comment(after)
                    .then_some(Value::Text(text))
                    .ok_or_else(|| fail("text after the closing quote"))
            }
            Some('\'') => {
                let (text, after) = single_quoted(&rest[1..])
                    .ok_or_else(|| fail("quoted text must end on the same line"))?;
                only_comment(after)
                    .then_some(Value::Text(text))
                    .ok_or_else(|| fail("text after the closing quote"))
            }
            Some('|' | '>') => self.block(rest, indent).map_err(fail).map(Value::Text),
            Some('[') => flow_list(&rest[1..]).ok_or_else(|| {
                fail("only one-line [a, b] lists of plain or quoted items are supported")
            }),
            Some('{') => Err(fail("{…} maps are not supported")),
            Some('&' | '*' | '!' | '%' | '@' | '`') => Err(fail(
                "anchors, aliases, tags and reserved characters are not supported",
            )),
            Some('-') if rest == "-" || rest.starts_with("- ") => {
                Err(fail("a list cannot start after a key on the same line"))
            }
            Some('?') if rest == "?" || rest.starts_with("? ") => {
                Err(fail("complex keys are not supported"))
            }
            _ => {
                let mut text = plain(rest).map_err(fail)?.to_string();
                // 往里缩的下一行接着写的：并进来，中间空行算一个换行。
                let mut breaks = 0;
                while let Some(line) = self.lines.get(self.at) {
                    if line.text.trim().is_empty() {
                        breaks += 1;
                        self.at += 1;
                        continue;
                    }
                    if line.indent <= indent || line.text.starts_with('#') {
                        break;
                    }
                    let more = plain(line.text).map_err(|why| line.unreadable(why))?;
                    if more.is_empty() {
                        // 只剩注释：到这里为止。
                        self.at += 1;
                        break;
                    }
                    text.push_str(&if breaks == 0 {
                        " ".to_string()
                    } else {
                        "\n".repeat(breaks)
                    });
                    text.push_str(more);
                    breaks = 0;
                    self.at += 1;
                }
                // 末尾跳过的空行不吃：退回去，交给下一个键当空行跳过。
                self.at -= breaks;
                Ok(resolve(&text))
            }
        }
    }

    /// `|`、`>` 的一段：`header` 是那个记号和后面的 `-`、`+`；内容是往下缩进比 `indent` 多的行。
    fn block(&mut self, header: &str, indent: usize) -> Result<String, &'static str> {
        let folded = header.starts_with('>');
        let mut chomp = None;
        let mut tail = &header[1..];
        if let Some(c) = tail.chars().next().filter(|c| matches!(c, '-' | '+')) {
            chomp = Some(c);
            tail = &tail[1..];
        }
        if tail.starts_with(|c: char| c.is_ascii_digit()) {
            return Err("indentation indicators in | and > are not supported");
        }
        if !only_comment(tail) {
            return Err("text after | or >");
        }
        // 收进来的行：去掉段的缩进（第一个有字的行定段缩进几格）；只有空白的行算空行。
        let mut body: Vec<&str> = Vec::new();
        let mut block_indent = None;
        while let Some(line) = self.lines.get(self.at) {
            let empty = line.text.trim().is_empty();
            match block_indent {
                _ if empty => body.push(""),
                None if line.indent > indent => {
                    block_indent = Some(line.indent);
                    body.push(line.text);
                }
                Some(n) if line.indent >= n => body.push(&line.raw[n..]),
                _ => break,
            }
            self.at += 1;
        }
        // 段后面的空行：不属于这一段的退回去（`+` 的照留）。
        let content_end = body
            .iter()
            .rposition(|l| !l.trim().is_empty())
            .map_or(0, |i| i + 1);
        let trailing = body.len() - content_end;
        let lines = &body[..content_end];
        let mut text = if folded {
            fold(lines)
        } else {
            lines.join("\n")
        };
        if content_end > 0 {
            match chomp {
                Some('-') => {}
                Some('+') => text.push_str(&"\n".repeat(trailing + 1)),
                _ => text.push('\n'),
            }
        } else if chomp == Some('+') {
            text.push_str(&"\n".repeat(trailing));
        }
        Ok(text)
    }
}

/// 把 `>` 的行折起来：相邻两行都是平常的行（不往里多缩）中间是一个空格；空行是一个换行；多缩进的行前后的换行照留。
fn fold(lines: &[&str]) -> String {
    let mut out = String::new();
    let mut previous: Option<&str> = None;
    let mut empties = 0;
    for line in lines {
        if line.is_empty() {
            empties += 1;
            continue;
        }
        if let Some(before) = previous {
            let more = |l: &str| l.starts_with([' ', '\t']);
            if more(before) || more(line) {
                out.push_str(&"\n".repeat(empties + 1));
            } else if empties == 0 {
                out.push(' ');
            } else {
                out.push_str(&"\n".repeat(empties));
            }
        } else {
            out.push_str(&"\n".repeat(empties));
        }
        out.push_str(line);
        previous = Some(line);
        empties = 0;
    }
    out
}

/// 一行里 `键:` 的位置：冒号后面是空白或者行尾；键不加引号。
fn split_key<'a>(line: &Line<'a>) -> Result<(&'a str, &'a str), Unreadable> {
    let text = line.text;
    if opens_quote(text) {
        return Err(line.unreadable("quoted keys are not supported"));
    }
    if text.starts_with(['?', '&', '*', '!', '|', '>', '%', '@', '`', '{', '[', ',']) {
        return Err(
            line.unreadable("anchors, aliases, tags, flow maps and complex keys are not supported")
        );
    }
    let at = split_key_at(text).ok_or_else(|| line.unreadable("expected `key: value`"))?;
    let key = text[..at].trim_end();
    if key.is_empty() {
        return Err(line.unreadable("empty key"));
    }
    Ok((key, text[at + 1..].trim_start()))
}

/// `键:` 的冒号在哪：后面是空白或行尾的第一个冒号，注释（空白后面的 `#`）以前。
fn split_key_at(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'#' && i > 0 && matches!(bytes[i - 1], b' ' | b'\t') {
            return None;
        }
        if b == b':' && matches!(bytes.get(i + 1), None | Some(b' ' | b'\t')) {
            return Some(i);
        }
    }
    None
}

fn opens_quote(text: &str) -> bool {
    text.starts_with(['"', '\''])
}

/// 收尾的引号后面只能是空白或注释。
fn only_comment(after: &str) -> bool {
    let after = after.trim_start();
    after.is_empty() || after.starts_with('#')
}

/// 不加引号的一句：去掉注释和行尾空白。里面有 `: ` 或者以 `:` 结尾的 YAML 当成又一层键值，读不了（宽松重读会给它加引号）。
fn plain(text: &str) -> Result<&str, &'static str> {
    let bytes = text.as_bytes();
    let mut end = text.len();
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'#' && (i == 0 || matches!(bytes[i - 1], b' ' | b'\t')) {
            end = i;
            break;
        }
    }
    let text = text[..end].trim_end();
    if split_key_at(text).is_some() {
        return Err("mapping values are not allowed here (a `: ` inside unquoted text)");
    }
    Ok(text)
}

/// 双引号里的（开头的引号已经去掉）：交回转义好的字和收尾引号后面的；这一行没收尾的没有。
fn double_quoted(text: &str) -> Option<(String, &str)> {
    let mut out = String::new();
    let mut chars = text.char_indices();
    while let Some((i, c)) = chars.next() {
        match c {
            '"' => return Some((out, &text[i + 1..])),
            '\\' => {
                let (_, e) = chars.next()?;
                let simple = match e {
                    '0' => '\0',
                    'a' => '\u{7}',
                    'b' => '\u{8}',
                    't' | '\t' => '\t',
                    'n' => '\n',
                    'v' => '\u{b}',
                    'f' => '\u{c}',
                    'r' => '\r',
                    'e' => '\u{1b}',
                    ' ' => ' ',
                    '"' => '"',
                    '/' => '/',
                    '\\' => '\\',
                    'N' => '\u{85}',
                    '_' => '\u{a0}',
                    'L' => '\u{2028}',
                    'P' => '\u{2029}',
                    'x' | 'u' | 'U' => {
                        let len = match e {
                            'x' => 2,
                            'u' => 4,
                            _ => 8,
                        };
                        let mut code = 0u32;
                        for _ in 0..len {
                            let (_, d) = chars.next()?;
                            code = code * 16 + d.to_digit(16)?;
                        }
                        char::from_u32(code)?
                    }
                    _ => return None,
                };
                out.push(simple);
            }
            c => out.push(c),
        }
    }
    None
}

/// 单引号里的（开头的引号已经去掉）：`''` 是一个 `'`。
fn single_quoted(text: &str) -> Option<(String, &str)> {
    let mut out = String::new();
    let mut chars = text.char_indices().peekable();
    while let Some((i, c)) = chars.next() {
        if c == '\'' {
            if chars.peek().is_some_and(|(_, n)| *n == '\'') {
                chars.next();
                out.push('\'');
                continue;
            }
            return Some((out, &text[i + 1..]));
        }
        out.push(c);
    }
    None
}

/// 一行写完的 `[…]`（开头的 `[` 已经去掉）：每一项是不加引号的或加引号的一句。
fn flow_list(text: &str) -> Option<Value> {
    let mut items = Vec::new();
    let mut rest = text.trim_start();
    loop {
        if let Some(after) = rest.strip_prefix(']') {
            return only_comment(after).then_some(Value::List(items));
        }
        let (item, after) = match rest.chars().next()? {
            '"' => {
                let (text, after) = double_quoted(&rest[1..])?;
                (Value::Text(text), after)
            }
            '\'' => {
                let (text, after) = single_quoted(&rest[1..])?;
                (Value::Text(text), after)
            }
            '[' | '{' | '&' | '*' | '!' | '#' => return None,
            _ => {
                let end = rest.find([',', ']'])?;
                let text = rest[..end].trim_end();
                if text.contains(['[', '{', '}', '#']) || split_key_at(text).is_some() {
                    return None;
                }
                (resolve(text), &rest[end..])
            }
        };
        items.push(item);
        rest = after.trim_start();
        if let Some(after) = rest.strip_prefix(',') {
            rest = after.trim_start();
        } else if !rest.starts_with(']') {
            return None;
        }
    }
}

/// 不加引号的一句认成什么（同 yaml-rust2 0.13 的 `Yaml::from_str`）。
fn resolve(text: &str) -> Value {
    let integer = |digits: &str, radix| i64::from_str_radix(digits, radix).is_ok();
    if let Some(digits) = text.strip_prefix("0x") {
        if integer(digits, 16) {
            return Value::Number;
        }
    } else if let Some(digits) = text.strip_prefix("0o") {
        if integer(digits, 8) {
            return Value::Number;
        }
    } else if let Some(digits) = text.strip_prefix('+')
        && digits.parse::<i64>().is_ok()
    {
        return Value::Number;
    }
    match text {
        "" | "~" | "null" => Value::Null,
        "true" | "True" | "TRUE" => Value::Bool(true),
        "false" | "False" | "FALSE" => Value::Bool(false),
        ".inf" | ".Inf" | ".INF" | "+.inf" | "+.Inf" | "+.INF" | "-.inf" | "-.Inf" | "-.INF"
        | ".nan" | ".NaN" | ".NAN" => Value::Number,
        _ if text.parse::<i64>().is_ok() => Value::Number,
        _ if text.bytes().any(|b| b.is_ascii_digit()) && text.parse::<f64>().is_ok() => {
            Value::Number
        }
        _ => Value::Text(text.to_string()),
    }
}

#[cfg(test)]
mod tests;
