//! 块级公式在不能显示图的终端里排成多行（蓝图 `tui.md`「图片、公式和 mermaid 图」，照旧版 `render/math` 的做法重写）：
//! 分式上下摞（分子、分数线、分母），前后的字照分数线那一行对齐；环境（矩阵、`aligned`、`cases`）分行分列排，
//! `\left`、`\right` 的定界符跟着里面的高度画，`\overbrace`、`\underbrace` 画一条括线、标注写在上面或下面；别的照
//! `math.rs` 转成一行。没有这几样的和一行的写法一样。
//!
//! 公式是模型写的，不可信：往里排有层数上限（[`MAX_NESTING`]），排出来比 `room` 宽的当场放弃（不然几万个 `&` 能排出
//! 几万列宽、几万行高的一块），放弃了的照一行的写法。

use std::ops::Range;

use unicode_width::UnicodeWidthStr;

use super::math::{Env, MAX_NESTING, Math, braced, env, env_name, line, row_break, unicode};

/// 排成的一块：几行字（每一行都补到 `width` 宽）、基线在第几行、多宽。
#[derive(Debug, Clone)]
struct Block {
    lines: Vec<String>,
    base: usize,
    width: usize,
}

/// 一格里的字靠哪边。
#[derive(Debug, Clone, Copy)]
enum Align {
    Left,
    Center,
    Right,
}

impl Block {
    fn text(text: &str) -> Self {
        Self {
            lines: vec![text.to_string()],
            base: 0,
            width: text.width(),
        }
    }

    fn empty() -> Self {
        Self::text("")
    }

    /// 各行的字，行尾的空白去掉。
    fn trimmed(self) -> Vec<String> {
        self.lines
            .into_iter()
            .map(|l| l.trim_end().to_string())
            .collect()
    }

    /// 第 `row` 行（从这一块的基线往上 `above` 行算第 0 行）；这一块没有这一行的是空白。
    fn row(&self, row: usize, above: usize, out: &mut String) {
        match (row + self.base)
            .checked_sub(above)
            .and_then(|i| self.lines.get(i))
        {
            Some(line) => out.push_str(line),
            None => pad(out, self.width),
        }
    }

    /// 上下摞起来，各自居中；基线是 `base` 那一行。
    fn pile(parts: &[Block], base: usize) -> Block {
        let width = parts.iter().map(|b| b.width).max().unwrap_or(0);
        let mut lines = Vec::new();
        for part in parts {
            let left = (width - part.width) / 2;
            for text in &part.lines {
                let mut line = String::with_capacity(width);
                pad(&mut line, left);
                line.push_str(text);
                pad(&mut line, width - part.width - left);
                lines.push(line);
            }
        }
        Block { lines, base, width }
    }

    /// 分式：分子、分数线、分母，各自居中，分数线比宽的那一个两边各多一格。
    fn fraction(top: Block, bottom: Block) -> Block {
        let width = top.width.max(bottom.width).max(1) + 2;
        let rule = Block::text(&crate::sheet::mark(|m| &m.rule).repeat(width));
        let base = top.lines.len();
        Block::pile(&[top, rule, bottom], base)
    }

    /// 括线和标注：`over` 的标注、括线在上面，不然在下面；基线照括住的那一块。
    fn braced(body: Block, label: Option<Block>, over: bool) -> Block {
        let (ends, peak) = if over {
            (['╭', '╮'], '┴')
        } else {
            (['╰', '╯'], '┬')
        };
        let brace: String = (0..body.width)
            .map(|i| match i {
                0 if body.width > 1 => ends[0],
                i if i + 1 == body.width && i > 0 => ends[1],
                i if i == body.width / 2 && body.width != 2 => peak,
                _ => '─',
            })
            .collect();
        let brace = Block::text(&brace);
        let label = label.unwrap_or_else(Block::empty);
        if over {
            let base = label.lines.len() + 1 + body.base;
            Block::pile(&[label, brace, body], base)
        } else {
            let base = body.base;
            Block::pile(&[body, brace, label], base)
        }
    }

    /// 一个定界符，`height` 行高：一行的照原字；高的用拼括号的那几个字（`⎛⎜⎝`），没有拼法的写在基线那一行。空的
    /// （`\left.`）交回 `None`。
    fn fence(delimiter: &str, height: usize, base: usize) -> Option<Block> {
        if delimiter.is_empty() {
            return None;
        }
        let pieces = match delimiter {
            "(" => Some(['⎛', '⎜', '⎝', '⎜']),
            ")" => Some(['⎞', '⎟', '⎠', '⎟']),
            "[" => Some(['⎡', '⎢', '⎣', '⎢']),
            "]" => Some(['⎤', '⎥', '⎦', '⎥']),
            "{" => Some(['⎧', '⎪', '⎩', '⎨']),
            "}" => Some(['⎫', '⎪', '⎭', '⎬']),
            "|" => Some(['│'; 4]),
            "‖" => Some(['║'; 4]),
            "⌊" => Some(['⎢', '⎢', '⎣', '⎢']),
            "⌋" => Some(['⎥', '⎥', '⎦', '⎥']),
            "⌈" => Some(['⎡', '⎢', '⎢', '⎢']),
            "⌉" => Some(['⎤', '⎥', '⎥', '⎥']),
            _ => None,
        };
        let width = delimiter.width();
        let lines = (0..height)
            .map(|row| match pieces {
                _ if height == 1 => delimiter.to_string(),
                Some([top, ..]) if row == 0 => top.to_string(),
                Some([_, _, bottom, _]) if row + 1 == height => bottom.to_string(),
                // 大括号中间的那个尖：三行以上才有地方画。
                Some([_, _, _, middle]) if row == height / 2 && height > 2 => middle.to_string(),
                Some([_, filler, ..]) => filler.to_string(),
                None if row == base => delimiter.to_string(),
                None => " ".repeat(width),
            })
            .collect();
        Some(Block { lines, base, width })
    }
}

fn pad(out: &mut String, n: usize) {
    out.extend(std::iter::repeat_n(' ', n));
}

/// 排成几行（行尾空白去掉），每一行不超过 `room` 列。没有分式、环境、括线的是一行，和 [`unicode`] 一样。排不下的：
/// 有几行的（`aligned`、`\\`）一行一行分开排；一行还是排不下的在最外面的等号、`\quad` 处断成几截，一截一截接着排；
/// 哪一截还是排不下，那一截照一行的写法（由调的一方折行）。嵌套太深的（套过 `MAX_NESTING` 层）也照一行的写法
/// （审计 #052：原来往里排没有上限，几万层花括号把栈撑爆）。
pub fn stacked(tex: &str, math: &Math, room: usize) -> Vec<String> {
    let plain = !["frac", "\\begin", "\\\\", "brace"]
        .iter()
        .any(|k| tex.contains(k));
    if plain {
        let text = unicode(tex, math);
        if text.width() <= room || !tex.contains('=') {
            return vec![text];
        }
    }
    let chars: Vec<char> = tex.chars().collect();
    let layout = Layout { math, room };
    if let Some(block) = layout.whole(&chars) {
        return block.trimmed();
    }
    let mut rows = rows(&chars);
    if rows.is_empty() {
        rows.push(chars);
    }
    let mut out: Vec<String> = Vec::new();
    let mut tall = false;
    for lines in rows.iter().flat_map(|row| layout.broken(row)) {
        // 上下摞着的两行之间空一行，不然上一行的分母和下一行的分子挨在一起。
        if !out.is_empty() && (tall || lines.len() > 1) {
            out.push(String::new());
        }
        tall = lines.len() > 1;
        out.extend(lines);
    }
    out
}

/// 一行里能断开的地方（每一截从哪里开始、是不是从 `\quad` 开始，头一截从 0）：最外面的 `=`、`\quad`、`\qquad`，
/// 花括号、圆括号、方括号、`\left…\right`、环境里面的不算。
fn cuts(chars: &[char]) -> Vec<(usize, bool)> {
    let mut cuts = vec![(0, false)];
    let (mut braces, mut round, mut fences) = (0usize, 0usize, 0usize);
    let mut at = 0;
    let mut seen = false;
    while at < chars.len() {
        let outer = braces == 0 && round == 0 && fences == 0;
        let c = chars[at];
        if c == '\\' {
            let len = chars[at + 1..]
                .iter()
                .take_while(|c| c.is_ascii_alphabetic())
                .count();
            let name: String = chars[at + 1..at + 1 + len.min(8)].iter().collect();
            match name.as_str() {
                "left" | "begin" => fences += 1,
                "right" | "end" => fences = fences.saturating_sub(1),
                "quad" | "qquad" if outer && seen => cuts.push((at, true)),
                // `\not=` 不从中间断。
                "not" if chars.get(at + 4) == Some(&'=') => at += 1,
                _ => {}
            }
            at += 1 + len.max(1);
            seen = true;
            continue;
        }
        match c {
            '{' => braces += 1,
            '}' => braces = braces.saturating_sub(1),
            '(' | '[' => round += 1,
            ')' | ']' => round = round.saturating_sub(1),
            '=' if outer && seen => cuts.push((at, false)),
            _ => {}
        }
        seen |= !c.is_whitespace();
        at += 1;
    }
    cuts
}

/// 整个公式的几行（排不下时分开排用）：整个公式就是一个 `aligned`、`gathered` 这类环境的，是它的各行；不然照最外面的
/// `\\` 分。每一行里的 `&` 去掉。
fn rows(chars: &[char]) -> Vec<Vec<char>> {
    let start = chars.iter().take_while(|c| c.is_whitespace()).count();
    let begin: Vec<char> = "\\begin".chars().collect();
    let inside = chars[start..]
        .strip_prefix(begin.as_slice())
        .and_then(|_| env_name(chars, start + begin.len()))
        .filter(|(name, _)| matches!(env(name), Env::Lines | Env::Centered))
        .map(|(_, after)| table(chars, after))
        .filter(|t| chars[t.end..].iter().all(|c| c.is_whitespace()));
    let table = inside.unwrap_or_else(|| table(chars, 0));
    if table.end < chars.len() {
        return Vec::new();
    }
    table
        .rows
        .into_iter()
        .map(|cells| cells.into_iter().flat_map(|c| &chars[c]).copied().collect())
        .filter(|row: &Vec<char>| row.iter().any(|c| !c.is_whitespace()))
        .collect()
}

/// 一个环境的正文切成的行和格（各是原文里的一截），和 `\end{…}` 后面的位置。
struct Table {
    rows: Vec<Vec<Range<usize>>>,
    end: usize,
}

/// 从 `at` 起切到配对的 `\end{…}`（没有的切到头）：`\\` 分行、`&` 分格，花括号里的、套着的环境里的不算。只往前扫一遍。
fn table(chars: &[char], mut at: usize) -> Table {
    let (mut braces, mut envs) = (0usize, 0usize);
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut start = at;
    let mut end = chars.len();
    let is = |name: &[char], word: &str| name.iter().copied().eq(word.chars());
    while at < chars.len() {
        let outer = braces == 0 && envs == 0;
        match chars[at] {
            '\\' if chars.get(at + 1) == Some(&'\\') => {
                let after = row_break(chars, at + 2);
                if outer {
                    row.push(start..at);
                    rows.push(std::mem::take(&mut row));
                    start = after;
                }
                at = after;
            }
            '\\' => {
                let len = chars[at + 1..]
                    .iter()
                    .take_while(|c| c.is_ascii_alphabetic())
                    .count();
                let name = &chars[at + 1..at + 1 + len];
                // 花括号里的 `\begin`、`\end` 自成一对，不数。
                if is(name, "begin") && braces == 0 {
                    envs += 1;
                } else if is(name, "end") && braces == 0 {
                    if envs == 0 {
                        end =
                            env_name(chars, at + 1 + len).map_or(at + 1 + len, |(_, after)| after);
                        break;
                    }
                    envs -= 1;
                }
                // 转义的字（`\{`、`\&`）连反斜杠跳过。
                at += 1 + len.max(1);
            }
            c => {
                match c {
                    '{' => braces += 1,
                    '}' => braces = braces.saturating_sub(1),
                    '&' if outer => {
                        row.push(start..at);
                        start = at + 1;
                    }
                    _ => {}
                }
                at += 1;
            }
        }
    }
    let at = at.min(chars.len());
    row.push(start..at);
    rows.push(row);
    Table { rows, end }
}

/// 一串在哪里停。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stop {
    End,
    /// 组的 `}`（吃掉）。
    Brace,
    /// `\right`（留着，由 `\left` 那一头接着读它的定界符）。
    Right,
}

struct Layout<'a> {
    math: &'a Math,
    room: usize,
}

impl Layout<'_> {
    /// 比 `room` 宽的不要。
    fn fit(&self, block: Block) -> Option<Block> {
        (block.width <= self.room).then_some(block)
    }

    /// 整个公式：最外面有 `\\` 的照 `aligned` 分行排，不然是一串。
    fn whole(&self, chars: &[char]) -> Option<Block> {
        let table = table(chars, 0);
        if table.rows.len() > 1 && table.end == chars.len() {
            self.grid(chars, &table, Env::Lines, &[], 0)
        } else {
            self.sequence(chars, &mut 0, Stop::End, 0)
        }
    }

    /// 一行排不下时的排法：在能断开的地方（[`cuts`]）断成几截，从头起能接几截接几截排成一块，再排后面的；单独一截
    /// 还是排不下的照一行的写法。交回一块一块的行。断的地方太多的（多半不是正经公式）不试，整个照一行的写法。
    fn broken(&self, chars: &[char]) -> Vec<Vec<String>> {
        if let Some(block) = self.sequence(chars, &mut 0, Stop::End, 1) {
            return vec![block.trimmed()];
        }
        let one = |part: &[char]| vec![unicode(&part.iter().collect::<String>(), self.math)];
        let cuts = cuts(chars);
        if cuts.len() < 2 || cuts.len() > 32 {
            return vec![one(chars)];
        }
        let end = |piece: usize| cuts.get(piece).map_or(chars.len(), |(at, _)| *at);
        let laid = |from: usize, to: usize| {
            self.sequence(&chars[cuts[from].0..end(to)], &mut 0, Stop::End, 1)
        };
        let mut out = Vec::new();
        let mut from = 0;
        while from < cuts.len() {
            let mut fits = None;
            for to in from + 1..=cuts.len() {
                match laid(from, to) {
                    Some(block) => fits = Some((to, block)),
                    None => break,
                }
            }
            // 能在 `\quad` 处断的就在那里断（两个式子之间），不把后一个式子的等号左边留在这一行的末尾。
            if let Some((to, _)) = fits
                && cuts.get(to).is_some_and(|(_, quad)| !quad)
                && let Some(quad) = (from + 1..to).rev().find(|piece| cuts[*piece].1)
            {
                fits = laid(from, quad).map(|block| (quad, block));
            }
            match fits {
                Some((to, block)) => {
                    out.push(block.trimmed());
                    from = to;
                }
                None => {
                    out.push(one(&chars[cuts[from].0..end(from + 1)]));
                    from += 1;
                }
            }
        }
        out
    }

    /// 横着接起来，照基线对齐。
    fn joined(&self, parts: Vec<Block>) -> Option<Block> {
        let width = parts.iter().map(|b| b.width).sum();
        if width > self.room {
            return None;
        }
        if parts.len() < 2 {
            return Some(parts.into_iter().next().unwrap_or_else(Block::empty));
        }
        let above = parts.iter().map(|b| b.base).max().unwrap_or(0);
        let below = parts.iter().map(|b| b.lines.len() - b.base).max()?;
        let lines = (0..above + below)
            .map(|row| {
                let mut line = String::with_capacity(width);
                for part in &parts {
                    part.row(row, above, &mut line);
                }
                line
            })
            .collect();
        Some(Block {
            lines,
            base: above,
            width,
        })
    }

    /// 一串，照 `stop` 停：分式、环境、`\left…\right`、括线各自排成一块，`{…}` 往里排，别的字（连命令和它的参数、
    /// 上下标）攒成一段交给 [`line`]。套得超过 [`MAX_NESTING`] 层的、比 `room` 宽的交回 `None`。
    fn sequence(&self, chars: &[char], at: &mut usize, stop: Stop, depth: usize) -> Option<Block> {
        if depth > MAX_NESTING {
            return None;
        }
        let mut parts: Vec<Block> = Vec::new();
        let mut run = String::new();
        // 攒着的那一段转好接上。`tight`：后面紧跟着定界符或者这一串到头了，段尾的空格不要。
        let flush = |parts: &mut Vec<Block>, run: &mut String, tight: bool| {
            if run.is_empty() {
                return;
            }
            let mut text = line(run, self.math);
            // 原文里段头段尾的空白各留一个（命令后面的那一个空格转的时候当分隔吃掉了）：`I \propto \frac…` 的 `∝` 和
            // 分式之间照样空一格。
            if run.starts_with(char::is_whitespace) && !text.starts_with(' ') {
                text.insert(0, ' ');
            }
            if run.ends_with(char::is_whitespace) && !text.ends_with(' ') {
                text.push(' ');
            }
            run.clear();
            let mut text = text.as_str();
            if parts.is_empty() {
                text = text.trim_start();
            }
            if tight {
                text = text.trim_end();
            }
            if !text.is_empty() {
                parts.push(Block::text(text));
            }
        };
        while *at < chars.len() {
            let c = chars[*at];
            match c {
                '}' if stop == Stop::Brace => {
                    *at += 1;
                    break;
                }
                '\\' => {
                    let name: String = chars[*at + 1..]
                        .iter()
                        .take_while(|c| c.is_ascii_alphabetic())
                        .collect();
                    let after = *at + 1 + name.len();
                    match name.as_str() {
                        "frac" | "dfrac" | "tfrac" | "cfrac" if depth < 8 => {
                            flush(&mut parts, &mut run, false);
                            *at = after;
                            let top = self.group(chars, at, depth + 1)?;
                            let bottom = self.group(chars, at, depth + 1)?;
                            parts.push(self.fit(Block::fraction(top, bottom))?);
                            continue;
                        }
                        "overbrace" | "underbrace" => {
                            flush(&mut parts, &mut run, false);
                            *at = after;
                            let body = self.group(chars, at, depth + 1)?;
                            let over = name == "overbrace";
                            let next = *at
                                + chars[*at..]
                                    .iter()
                                    .take_while(|c| c.is_whitespace())
                                    .count();
                            let label = match chars.get(next) {
                                Some('^' | '_') => {
                                    *at = next + 1;
                                    Some(self.group(chars, at, depth + 1)?)
                                }
                                _ => None,
                            };
                            parts.push(self.fit(Block::braced(body, label, over))?);
                            continue;
                        }
                        "begin" => {
                            if let Some((name, body)) = env_name(chars, after)
                                && env(&name) != Env::Unknown
                            {
                                flush(&mut parts, &mut run, false);
                                *at = body;
                                parts.push(self.environment(chars, at, &name, depth)?);
                                continue;
                            }
                        }
                        "left" => {
                            flush(&mut parts, &mut run, true);
                            *at = after;
                            let open = self.delimiter(chars, at);
                            let inner = self.sequence(chars, at, Stop::Right, depth + 1)?;
                            let close = if chars[(*at).min(chars.len())..].starts_with(&['\\', 'r'])
                            {
                                *at += "\\right".len();
                                self.delimiter(chars, at)
                            } else {
                                String::new()
                            };
                            let tall = inner.lines.len() > 1;
                            let (height, base) = (inner.lines.len(), inner.base);
                            parts.push(self.fenced(&open, inner, &close)?);
                            if tall {
                                parts.extend(self.raised(chars, at, height, base));
                            }
                            continue;
                        }
                        "right" if stop == Stop::Right => break,
                        _ => {}
                    }
                    // 别的命令连同后面紧跟的参数原样攒着，由 `line` 一起转（`\sqrt{…}` 里的分式写成一行）。
                    let len = 1 + name.len().max(usize::from(name.is_empty()));
                    run.extend(&chars[*at..(*at + len).min(chars.len())]);
                    *at += len;
                    take_args(chars, at, &mut run, name == "sqrt");
                }
                '^' | '_' => {
                    run.push(c);
                    *at += 1;
                    take_args(chars, at, &mut run, false);
                }
                '{' => {
                    flush(&mut parts, &mut run, false);
                    *at += 1;
                    parts.push(self.sequence(chars, at, Stop::Brace, depth + 1)?);
                }
                _ => {
                    run.push(c);
                    *at += 1;
                }
            }
        }
        flush(&mut parts, &mut run, true);
        self.joined(parts)
    }

    /// 分式的一格：`{…}` 往里排，别的是一个字或一个命令。套得太深的交回 `None`（同 [`Self::sequence`]）。
    fn group(&self, chars: &[char], at: &mut usize, depth: usize) -> Option<Block> {
        while chars.get(*at).is_some_and(|c| c.is_whitespace()) {
            *at += 1;
        }
        match chars.get(*at) {
            Some('{') => {
                *at += 1;
                self.sequence(chars, at, Stop::Brace, depth)
            }
            Some('\\') => {
                let name: String = chars[*at + 1..]
                    .iter()
                    .take_while(|c| c.is_ascii_alphabetic())
                    .collect();
                let len = 1 + name.len().max(1);
                let source: String = chars[*at..(*at + len).min(chars.len())].iter().collect();
                *at += len;
                self.fit(Block::text(&unicode(&source, self.math)))
            }
            Some(c) => {
                *at += 1;
                Some(Block::text(&c.to_string()))
            }
            None => Some(Block::empty()),
        }
    }

    /// `\left`、`\right` 后面的定界符，转好交回；`.`（这一边没有）是空的。
    fn delimiter(&self, chars: &[char], at: &mut usize) -> String {
        while chars.get(*at).is_some_and(|c| c.is_whitespace()) {
            *at += 1;
        }
        let len = match (chars.get(*at), chars.get(*at + 1)) {
            (None, _) => return String::new(),
            (Some('\\'), Some(c)) if c.is_ascii_alphabetic() => {
                1 + chars[*at + 1..]
                    .iter()
                    .take_while(|c| c.is_ascii_alphabetic())
                    .count()
            }
            (Some('\\'), Some(_)) => 2,
            _ => 1,
        };
        let source: String = chars[*at..*at + len].iter().collect();
        *at += len;
        if source == "." {
            String::new()
        } else {
            unicode(&source, self.math)
        }
    }

    /// 两边加上定界符，跟着里面的高度画。
    fn fenced(&self, open: &str, inner: Block, close: &str) -> Option<Block> {
        let (height, base) = (inner.lines.len(), inner.base);
        let parts = [
            Block::fence(open, height, base),
            Some(inner),
            Block::fence(close, height, base),
        ];
        self.joined(parts.into_iter().flatten().collect())
    }

    /// 高的定界符后面紧跟的上下标：上标写在最上面一行、下标写在最下面一行（`\right|_0^1`、`\right)^k`）。
    fn raised(&self, chars: &[char], at: &mut usize, height: usize, base: usize) -> Option<Block> {
        let (mut upper, mut lower) = (None, None);
        while let Some(&marker) = chars.get(*at).filter(|c| matches!(c, '^' | '_')) {
            let slot = if marker == '^' {
                &mut upper
            } else {
                &mut lower
            };
            if slot.is_some() {
                break;
            }
            let mut source = marker.to_string();
            *at += 1;
            take_args(chars, at, &mut source, false);
            *slot = Some(unicode(&source, self.math));
        }
        let width = [&upper, &lower]
            .into_iter()
            .flatten()
            .map(|s| s.width())
            .max()?;
        let lines = (0..height)
            .map(|row| {
                let text = match row {
                    0 => upper.as_deref(),
                    row if row + 1 == height => lower.as_deref(),
                    _ => None,
                }
                .unwrap_or("");
                format!("{text}{}", " ".repeat(width - text.width()))
            })
            .collect();
        Some(Block { lines, base, width })
    }

    /// 一个环境：`at` 在 `\begin{name}` 后面，读到配对的 `\end{…}` 后面。
    fn environment(
        &self,
        chars: &[char],
        at: &mut usize,
        name: &str,
        depth: usize,
    ) -> Option<Block> {
        let kind = env(name);
        // `array` 写各列怎么对齐的那个参数（`{lcr}`）。
        let mut columns = Vec::new();
        if matches!(name, "array" | "alignat" | "alignedat")
            && let Some(after) = braced(chars, *at)
        {
            // 套着花括号的那几截（`{@{}lr}` 的 `@{}`）不是列。
            let mut level = 0usize;
            for c in &chars[*at..after] {
                match c {
                    '{' => level += 1,
                    '}' => level -= 1,
                    'l' if level == 1 => columns.push(Align::Left),
                    'c' if level == 1 => columns.push(Align::Center),
                    'r' if level == 1 => columns.push(Align::Right),
                    _ => {}
                }
            }
            *at = after;
        }
        let table = table(chars, *at);
        *at = table.end;
        let grid = self.grid(chars, &table, kind, &columns, depth)?;
        let (open, close) = match kind {
            Env::Matrix(open, close) => (open, close),
            Env::Cases => ("{", ""),
            _ => ("", ""),
        };
        self.fenced(open, grid, close)
    }

    /// 环境的正文排成格子：各列一样宽，照这一类环境的规矩对齐；上下摞着的行之间空一行。`aligned` 这类的格里分式
    /// 照常上下摞；矩阵、`cases` 的格照一行的写法（TeX 里它们也是小号的行内写法）。
    fn grid(
        &self,
        chars: &[char],
        table: &Table,
        kind: Env,
        columns: &[Align],
        depth: usize,
    ) -> Option<Block> {
        let stacked = matches!(kind, Env::Lines | Env::Centered);
        let mut rows = Vec::with_capacity(table.rows.len());
        for cells in &table.rows {
            let mut row = Vec::with_capacity(cells.len());
            for cell in cells {
                let source = &chars[cell.clone()];
                row.push(if stacked {
                    self.sequence(source, &mut 0, Stop::End, depth + 1)?
                } else {
                    let source: String = source.iter().collect();
                    self.fit(Block::text(&unicode(&source, self.math)))?
                });
            }
            rows.push(row);
        }
        // 最后一个 `\\` 后面没有字的那一行不算。
        if rows.len() > 1 && rows.last().is_some_and(|r| r.iter().all(|c| c.width == 0)) {
            rows.pop();
        }
        let count = rows.iter().map(Vec::len).max().unwrap_or(0);
        let mut widths = vec![0usize; count];
        for row in &rows {
            for (width, cell) in widths.iter_mut().zip(row) {
                *width = (*width).max(cell.width);
            }
        }
        let gap = if stacked { 0 } else { 2 };
        let width = widths.iter().sum::<usize>() + gap * count.saturating_sub(1);
        if width > self.room {
            return None;
        }
        let align = |column: usize| match kind {
            Env::Lines if count > 1 && column.is_multiple_of(2) => Align::Right,
            Env::Lines | Env::Cases => Align::Left,
            Env::Array => columns.get(column).copied().unwrap_or(Align::Center),
            _ => Align::Center,
        };
        let mut lines = Vec::new();
        let mut tall = false;
        for row in &rows {
            let above = row.iter().map(|c| c.base).max().unwrap_or(0);
            let below = row
                .iter()
                .map(|c| c.lines.len() - c.base)
                .max()
                .unwrap_or(1);
            if !lines.is_empty() && (tall || above + below > 1) {
                lines.push(" ".repeat(width));
            }
            tall = above + below > 1;
            for r in 0..above + below {
                let mut line = String::with_capacity(width);
                let mut used = 0;
                for (column, cell) in row.iter().enumerate() {
                    let free = widths[column] - cell.width;
                    let left = match align(column) {
                        Align::Left => 0,
                        Align::Center => free / 2,
                        Align::Right => free,
                    };
                    pad(&mut line, left + if column > 0 { gap } else { 0 });
                    cell.row(r, above, &mut line);
                    pad(&mut line, free - left);
                    used += widths[column] + if column > 0 { gap } else { 0 };
                }
                pad(&mut line, width - used);
                lines.push(line);
            }
        }
        if lines.is_empty() {
            return Some(Block::empty());
        }
        let base = (lines.len() - 1) / 2;
        Some(Block { lines, base, width })
    }
}

/// 命令、上下标后面紧跟着的参数（`{…}`，`brackets` 时还有 `[…]`；套着的照括号配对）原样接进 `run`；上下标没有括号的
/// 接一个字或者一个命令（`x^2`、`x^\infty`）。转义的 `\{`、`\}` 不算括号（同 [`unicode`]；原来算，`\text{\{}` 吞掉了
/// 后面整段）。方括号只有 `\sqrt[3]{x}` 是参数：原来别的命令后面的也算，`\left[…\right]`、`\bigl[…\bigr]` 里的分式
/// 就排不成上下摞的。
fn take_args(chars: &[char], at: &mut usize, run: &mut String, brackets: bool) {
    let mut took = false;
    while let Some(&open) = chars
        .get(*at)
        .filter(|c| **c == '{' || (brackets && **c == '['))
    {
        let close = if open == '{' { '}' } else { ']' };
        let mut level = 0;
        while let Some(&c) = chars.get(*at) {
            run.push(c);
            *at += 1;
            if c == '\\' {
                if let Some(&escaped) = chars.get(*at) {
                    run.push(escaped);
                    *at += 1;
                }
            } else if c == open {
                level += 1;
            } else if c == close {
                level -= 1;
                if level == 0 {
                    break;
                }
            }
        }
        took = true;
    }
    let last = run.chars().last();
    if !took
        && matches!(last, Some('^' | '_'))
        && let Some(&c) = chars.get(*at)
    {
        run.push(c);
        *at += 1;
        if c == '\\' {
            let len = chars[*at..]
                .iter()
                .take_while(|c| c.is_ascii_alphabetic())
                .count()
                .max(1);
            run.extend(&chars[*at..(*at + len).min(chars.len())]);
            *at += len;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::stacked;
    use crate::markdown::math::{Math, unicode};

    fn math() -> Math {
        serde_json::from_str(include_str!("../../../../resources/tui/math.json")).unwrap()
    }

    #[test]
    fn a_fraction_stacks_with_a_rule_and_the_rest_sits_on_the_rule_line() {
        let m = math();
        let lines = stacked(r"\frac{\partial f}{\partial x} = 0", &m, 100_000);
        assert_eq!(lines.len(), 3, "{lines:?}");
        assert!(lines[0].contains("∂f"), "{lines:?}");
        assert!(
            lines[1].starts_with('─') && lines[1].ends_with("= 0"),
            "{lines:?}"
        );
        assert!(lines[2].contains("∂x"), "{lines:?}");
    }

    #[test]
    fn the_quadratic_formula_and_a_nested_fraction() {
        let m = math();
        let lines = stacked(r"x=\frac{-b\pm\sqrt{b^2-4ac}}{2a}", &m, 100_000);
        assert_eq!(lines.len(), 3, "{lines:?}");
        assert!(
            lines[0].contains("-b±√(b²-4ac)") || lines[0].contains("-b±√b²-4ac"),
            "{lines:?}"
        );
        assert!(lines[1].starts_with("x=─"), "{lines:?}");
        assert!(lines[2].contains("2a"), "{lines:?}");
        let nested = stacked(r"\frac{1}{1+\frac{1}{x}}", &m, 100_000);
        assert_eq!(nested.len(), 5, "分母里的分式也摞：{nested:?}");
    }

    #[test]
    fn a_formula_without_fractions_is_the_one_line_version() {
        let m = math();
        assert_eq!(stacked(r"E=mc^2", &m, 100_000), [unicode(r"E=mc^2", &m)]);
        assert_eq!(
            stacked(r"\sum_{i=1}^n x_i", &m, 100_000),
            [unicode(r"\sum_{i=1}^n x_i", &m)]
        );
    }

    /// 审计 #052（2026-11-18）：`{…}` 往里排没有层数上限，模型写几万层花括号就把栈撑爆；栈溢出是 SIGSEGV，不走 panic 的钩子，
    /// 终端留在备用屏和原始模式里。在一条小栈的线程上排几种深套的写法：超了上限的整段照一行的写法（[`unicode`] 再超了是
    /// 原样），不崩。
    #[test]
    fn deep_nesting_stays_on_one_line_and_fits_a_small_stack() {
        crate::markdown::tests::on_a_small_stack(
            "markdown::stack::tests::deep_nesting_stays_on_one_line_and_fits_a_small_stack",
            || {
                let m = math();
                let braces = format!(r"\frac{}", "{".repeat(100_000));
                assert_eq!(
                    stacked(&braces, &m, 100_000),
                    std::slice::from_ref(&braces),
                    "原样一行"
                );
                let closed = format!(
                    r"\frac{{a}}{{b}} {}x{}",
                    "{".repeat(50_000),
                    "}".repeat(50_000)
                );
                assert_eq!(stacked(&closed, &m, 100_000), std::slice::from_ref(&closed));
                // 参数里的 `\{` 原来当成开括号数，吞掉了外面那个 `}`：括号数着只有两层，排的时候一层层往里套。
                let escaped = format!(r"\frac{{1}}{{2}} {}", r"{\sqrt{\{}}".repeat(2_000));
                assert_eq!(
                    stacked(&escaped, &m, 100_000).len(),
                    3,
                    "分式照摞，后面的照一行"
                );
            },
        );
    }

    /// 命令参数里转义的花括号（`\{`、`\}`）不算括号，和 [`unicode`] 一样：`\text{\{}` 之后的分式照样摞。
    #[test]
    fn escaped_braces_in_an_argument_do_not_count() {
        let m = math();
        let lines = stacked(r"\text{\{} \frac{a}{b}", &m, 100_000);
        assert_eq!(lines.len(), 3, "{lines:?}");
        let lines = stacked(r"\text{\}} = \frac{a}{b}", &m, 100_000);
        assert_eq!(lines.len(), 3, "{lines:?}");
    }
}
