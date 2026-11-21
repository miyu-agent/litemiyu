//! 公式转成 Unicode（蓝图 `tui.md`「图片、公式和 mermaid 图」第 5 条，照旧版 `render/math/unicode.rs`）：
//! `x^2` → `x²`，`\alpha` → `α`，`\frac{a}{b}` → `a/b`。尽力而为，转不动的命令原样留着，永不失败。
//!
//! 一行里画不出上下摞的分式、矩阵，写法照「不把意思改错」定（LM-27）：
//! - 分式紧挨着别的因子时括起来（`\frac{m}{2}\dot q^2` 是 `(m/2)q̇²`），分母不止一个因子的也括（`1/(2n)`）；
//! - 转不成上下标字符的上下标，不止一个字的括起来（`d^(3N)q`），一个字的直接接在记号后面（`g^μ`）；
//! - 环境（`\begin{…}`）的格用 `, ` 隔开、行用 `; ` 隔开，矩阵两边写上定界符（`[1, 2; 3, 4]`）；
//! - `\left`、`\right`、`\bigl` 这些只管大小，后面的定界符照常写，只有 `.` 表示没有。
//!
//! 对照表（命令到符号、上下标、附标）写在 `resources/tui/math.json`，排到公式那一刻才读（[`MathTable`]）。

use std::cell::Cell;

use std::collections::HashMap;
use std::sync::OnceLock;

use serde::Deserialize;

/// 嵌套的上限：公式是模型写的，递归没有上限就是一个爆栈的口子。超了的原样交回。
pub(super) const MAX_NESTING: usize = 64;

/// 公式的对照表（`resources/math.json`）。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Math {
    /// 命令名（不带 `\`）到写出来的字，例如 `alpha` → `α`，`sin` → ` sin `。
    pub symbols: HashMap<String, String>,
    /// 上标字符：`2` → `²`。有一个字没有上标写法的，整段写成 `^(…)`。
    pub superscripts: HashMap<char, char>,
    /// 下标字符：`n` → `ₙ`，规则同上标。
    pub subscripts: HashMap<char, char>,
    /// 附标命令到组合字符：`hat` → U+0302，接在参数的字后面。
    pub accents: HashMap<String, String>,
    /// 只留参数的命令：`\text{…}`、`\mathbf{…}` 这类。
    pub text: Vec<String>,
    /// 整个丢掉的命令：`\left`、`\displaystyle` 这类只管排版的。
    pub ignored: Vec<String>,
    /// 写成一个空格的命令：`\quad`、`\qquad`。
    pub spaces: Vec<String>,
}

/// 公式的对照表，排到公式（或 `<sup>`、`<sub>` 里的字）那一刻才读：没有公式的回答不读、不占内存（2026-11-17 加回公式排版时
/// 定的，界面线第二十七趟）。Miyu 是读配置时就读好（`Config::math` 是读好的表），这里是有意的差别。
#[derive(Debug, Clone, Default)]
pub struct MathTable(OnceLock<Math>);

impl MathTable {
    /// 对照表；第一次要时读编进程序的那一份。
    ///
    /// # Panics
    ///
    /// 编进程序的 `math.json` 读不懂：安装坏了才会有，测试守着（本文件的测试、`config/tests.rs`）。
    pub fn get(&self) -> &Math {
        self.0.get_or_init(|| {
            lm_kernel::kernel::json::from_str(tui_json!("math.json"))
                .expect("编进程序的 math.json 读得懂")
        })
    }

    /// 读过没有（测试看它守「没有公式不读」）。
    #[cfg(test)]
    pub fn read(&self) -> bool {
        self.0.get().is_some()
    }
}

/// 把一段 LaTeX 转成一行 Unicode；多个空白并成一个，两头的空白去掉。
pub fn unicode(tex: &str, math: &Math) -> String {
    let out = line(tex, math);
    if out.starts_with(' ') || out.ends_with(' ') {
        out.trim().to_string()
    } else {
        out
    }
}

/// 同 [`unicode`]，两头的空白各留一个：`stack.rs` 把公式切成几段分别转，段和段之间的空格（`\sin` 前后的、原文里写的）
/// 要留着。
pub(super) fn line(tex: &str, math: &Math) -> String {
    let chars: Vec<char> = tex.chars().collect();
    if depth(&chars) > MAX_NESTING {
        return tex.to_string();
    }
    let mut at = 0;
    let converter = Converter {
        chars: &chars,
        math,
        depth: Cell::new(0),
        too_deep: Cell::new(false),
    };
    let (out, _) = converter.sequence(&mut at, Stop::End);
    if converter.too_deep.get() {
        return tex.to_string();
    }
    let mut squeezed = String::with_capacity(out.len());
    for c in out.chars() {
        if !c.is_whitespace() {
            squeezed.push(c);
        } else if !squeezed.ends_with(' ') {
            squeezed.push(' ');
        }
    }
    squeezed
}

/// 没转义的 `{` 最深套了几层。
fn depth(chars: &[char]) -> usize {
    let (mut depth, mut max, mut escaped) = (0usize, 0usize, false);
    for &c in chars {
        if escaped {
            escaped = false;
            continue;
        }
        match c {
            '\\' => escaped = true,
            '{' => {
                depth += 1;
                max = max.max(depth);
            }
            '}' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    max
}

/// 环境（`\begin{…}`）是哪一类，一行的写法和叠行的排法（`stack.rs`）都照它。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Env {
    /// 矩阵：各列对齐，两边是这一对定界符（没有的是空串）。
    Matrix(&'static str, &'static str),
    /// `array`：同矩阵，后面跟着一个写各列怎么对齐的参数，定界符由外面的 `\left`、`\right` 给。
    Array,
    /// `cases`：左边一个大括号，各列靠左。
    Cases,
    /// `aligned` 这一类：一行一个式子，在 `&` 处对齐。
    Lines,
    /// `gathered` 这一类：一行一个式子，各自居中。
    Centered,
    /// 不认得的：`\begin{…}`、`\end{…}` 照原样写，里面的照常转。
    Unknown,
}

/// 照名字（不带 `*`）认环境。
pub(super) fn env(name: &str) -> Env {
    match name {
        "matrix" | "smallmatrix" => Env::Matrix("", ""),
        "pmatrix" => Env::Matrix("(", ")"),
        "bmatrix" => Env::Matrix("[", "]"),
        "Bmatrix" => Env::Matrix("{", "}"),
        "vmatrix" => Env::Matrix("|", "|"),
        "Vmatrix" => Env::Matrix("‖", "‖"),
        "array" => Env::Array,
        "cases" | "dcases" => Env::Cases,
        "aligned" | "align" | "split" | "eqnarray" | "flalign" | "alignat" | "alignedat" => {
            Env::Lines
        }
        "gathered" | "gather" | "multline" | "equation" => Env::Centered,
        _ => Env::Unknown,
    }
}

/// 只管定界符大小的命令：`\left`、`\right`、`\middle`、`\big`、`\Bigl`、`\biggr` 这些。后面的定界符照常写，只有 `.`
/// 表示这一边没有定界符。
pub(super) fn sizing(name: &str) -> bool {
    matches!(name, "left" | "right" | "middle")
        || ["bigg", "Bigg", "big", "Big"].iter().any(|size| {
            name.strip_prefix(size)
                .is_some_and(|side| matches!(side, "" | "l" | "r" | "m"))
        })
}

/// `\begin`、`\end` 后面的 `{名字}`：交回名字（去掉 `*`）和它后面的位置；不是这个写法的交回 `None`。名字最多看 32 个字。
pub(super) fn env_name(chars: &[char], at: usize) -> Option<(String, usize)> {
    if chars.get(at) != Some(&'{') {
        return None;
    }
    let len = chars[at + 1..]
        .iter()
        .take(32)
        .position(|c| !c.is_ascii_alphabetic() && *c != '*')?;
    let close = at + 1 + len;
    (chars[close] == '}').then(|| {
        let name: String = chars[at + 1..close].iter().filter(|c| **c != '*').collect();
        (name, close + 1)
    })
}

/// `at` 处的一对花括号 `{…}`（里面套着的照配对，转义的 `\{`、`\}` 不算）后面的位置；`at` 处不是 `{`、64 个字以内没有收尾的交回
/// `None`（只用来读 `array` 写各列怎么对齐的那个参数，它很短；不设上限的话一串没收尾的 `\begin{array}{` 每个都要扫到头）。
pub(super) fn braced(chars: &[char], at: usize) -> Option<usize> {
    if chars.get(at) != Some(&'{') {
        return None;
    }
    let (mut level, mut escaped) = (0usize, false);
    for (i, &c) in chars.iter().enumerate().skip(at).take(64) {
        match c {
            _ if escaped => escaped = false,
            '\\' => escaped = true,
            '{' => level += 1,
            '}' if level == 1 => return Some(i + 1),
            '}' => level -= 1,
            _ => {}
        }
    }
    None
}

/// 换行 `\\` 后面可以带的 `*` 和 `[长度]`（`\\[2pt]`）：交回它们后面的位置。方括号里不像长度的（`\\[x]` 是下一行开头的
/// 方括号）不算。
pub(super) fn row_break(chars: &[char], mut at: usize) -> usize {
    if chars.get(at) == Some(&'*') {
        at += 1;
    }
    if chars.get(at) == Some(&'[') {
        let inner = &chars[at + 1..chars.len().min(at + 17)];
        if let Some(len) = inner.iter().position(|c| *c == ']')
            && inner
                .first()
                .is_some_and(|c| c.is_ascii_digit() || matches!(c, '-' | '.'))
            && inner[..len]
                .iter()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.'))
        {
            return at + len + 2;
        }
    }
    at
}

/// 一串在哪里停。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stop {
    /// 写到头。
    End,
    /// 组的 `}`。
    Brace,
    /// 环境里的一格：`&`、`\\`、`\end{…}`。
    Cell,
}

/// 一串是怎么停的。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Ended {
    Input,
    Brace,
    /// `&`：这一行的下一格。
    Amp,
    /// `\\`：下一行。
    Row,
    /// `\end{…}`：环境收尾。
    Env,
}

/// 刚写下的是什么，后面紧跟的字照它定要不要空格、括号。
#[derive(Debug, Clone, Copy, Default)]
struct Prev {
    /// 求和、积分这类大算符（连同它的上下标）：后面的分式是它的被加、被积的式子，不用括起来。
    big: bool,
    /// 函数名（`\sin`，后面带着一个空格）：后面紧跟括号、上下标时这个空格去掉。
    func: bool,
}

struct Converter<'a> {
    chars: &'a [char],
    math: &'a Math,
    /// 参数套了几层：`\sqrt\sqrt…x` 这种不带括号的嵌套 `depth()` 数不到，在这里数（review A8）。
    depth: Cell<usize>,
    /// 套得超过上限了：整段照原样写，不再往下转。
    too_deep: Cell<bool>,
}

impl Converter<'_> {
    /// 一串，照 `stop` 停。
    fn sequence(&self, at: &mut usize, stop: Stop) -> (String, Ended) {
        let mut out = String::new();
        let mut prev = Prev::default();
        while let Some(&c) = self.chars.get(*at) {
            *at += 1;
            let was = std::mem::take(&mut prev);
            match c {
                '}' if stop == Stop::Brace => return (out, Ended::Brace),
                '&' if stop == Stop::Cell => return (out, Ended::Amp),
                '\\' => {
                    if let Some(ended) = self.command(at, &mut out, stop, was, &mut prev) {
                        return (out, ended);
                    }
                }
                '^' | '_' => {
                    let content = self.group(at);
                    tighten(&mut out, was.func);
                    self.script(&mut out, &content, c);
                    prev = was;
                }
                '{' => out.push_str(&self.sequence(at, Stop::Brace).0),
                '(' | '[' => {
                    tighten(&mut out, was.func);
                    out.push(c);
                }
                '\'' => out.push('′'),
                c if c == '~' || c.is_whitespace() => {
                    out.push(' ');
                    prev = was;
                }
                c => out.push(c),
            }
        }
        (out, Ended::Input)
    }

    /// 一个参数：`{…}`，或者一个命令、一个字。套得超过上限的不再往下转（review A8：原来几万层把栈撑爆）。
    fn group(&self, at: &mut usize) -> String {
        let depth = self.depth.get() + 1;
        if depth > MAX_NESTING || self.too_deep.get() {
            self.too_deep.set(true);
            return String::new();
        }
        self.depth.set(depth);
        let out = self.argument(at);
        self.depth.set(depth - 1);
        out
    }

    /// 参数本身：`{…}`，或者一个命令、一个字；前面的空白不算（TeX 的规矩）。
    fn argument(&self, at: &mut usize) -> String {
        while self.chars.get(*at).is_some_and(|c| c.is_whitespace()) {
            *at += 1;
        }
        match self.chars.get(*at) {
            Some('{') => {
                *at += 1;
                self.sequence(at, Stop::Brace).0
            }
            Some('\\') => {
                *at += 1;
                let mut out = String::new();
                self.command(
                    at,
                    &mut out,
                    Stop::End,
                    Prev::default(),
                    &mut Prev::default(),
                );
                out
            }
            Some(c) => {
                *at += 1;
                c.to_string()
            }
            None => String::new(),
        }
    }

    /// `\` 后面的命令，写进 `out`。让这一串停下的（`\\`、`\end{…}`）交回怎么停的。
    fn command(
        &self,
        at: &mut usize,
        out: &mut String,
        stop: Stop,
        was: Prev,
        prev: &mut Prev,
    ) -> Option<Ended> {
        // 单个字的转义：`\{`、`\,` 这些。
        if let Some(&c) = self.chars.get(*at)
            && !c.is_ascii_alphabetic()
        {
            *at += 1;
            match c {
                '\\' => {
                    *at = row_break(self.chars, *at);
                    if stop == Stop::Cell {
                        return Some(Ended::Row);
                    }
                    out.push_str("; ");
                }
                ',' | ';' | ':' | ' ' => {
                    out.push(' ');
                    *prev = was;
                }
                // 负的小间距：不占地方。
                '!' => *prev = was,
                '|' => out.push('‖'),
                c => out.push(c),
            }
            return None;
        }
        let start = *at;
        while self.chars.get(*at).is_some_and(char::is_ascii_alphabetic) {
            *at += 1;
        }
        let name: String = self.chars[start..*at].iter().collect();
        // 命令后面的一个空格是分隔，不算字（TeX 的规矩）。
        if self.chars.get(*at) == Some(&' ') {
            *at += 1;
        }
        let math = self.math;
        match name.as_str() {
            "frac" | "dfrac" | "tfrac" | "cfrac" => {
                let top = self.group(at);
                let bottom = self.group(at);
                let text = format!("{}/{}", self.term(&top, false), self.term(&bottom, true));
                // 一行的分式紧挨着别的因子（`\frac{m}{2}\dot q^2`、`\sin\frac{x}{2}`）要括起来，不然读成 `m/(2q̇²)`。
                if self.loose_before(out, was) && self.loose_after(*at) {
                    out.push_str(&text);
                } else {
                    tighten(out, was.func);
                    out.push('(');
                    out.push_str(&text);
                    out.push(')');
                }
            }
            "sqrt" => {
                let index = self.optional(at);
                let body = Self::radicand(&self.group(at));
                match index.as_deref().map(str::trim) {
                    None | Some("" | "2") => out.push('√'),
                    Some("3") => out.push('∛'),
                    Some("4") => out.push('∜'),
                    Some(n) => {
                        let table = &math.superscripts;
                        match n.chars().map(|c| table.get(&c).copied()).collect() {
                            Some::<String>(small) => out.push_str(&format!("{small}√")),
                            None => out.push_str(&format!("√[{n}]")),
                        }
                    }
                }
                out.push_str(&body);
            }
            "binom" | "dbinom" | "tbinom" => {
                let upper = self.group(at);
                let lower = self.group(at);
                out.push_str(&format!("C({},{})", upper.trim(), lower.trim()));
            }
            "begin" => self.environment(at, out),
            "end" => {
                let name = env_name(self.chars, *at);
                if stop == Stop::Cell {
                    *at = name.map_or(*at, |(_, after)| after);
                    return Some(Ended::Env);
                }
                // 没有开头的 `\end{…}`：照原样写。
                out.push_str("\\end");
                if let Some((name, after)) = name {
                    out.push_str(&format!("{{{name}}}"));
                    *at = after;
                }
            }
            name if sizing(name) => {
                if self.chars.get(*at) == Some(&'.') {
                    *at += 1;
                }
                *prev = was;
            }
            "mathbb" => out.extend(self.group(at).chars().map(blackboard)),
            "not" => {
                let target = self.group(at);
                out.push_str(&negated(target.trim()));
            }
            "pmod" => out.push_str(&format!(" (mod {})", self.group(at).trim())),
            // 式子的编号写在括号里；`\label{…}` 是给引用用的名字，不是式子里的字。
            "tag" => out.push_str(&format!(" ({})", self.group(at).trim())),
            "label" => {
                self.group(at);
            }
            "overbrace" | "underbrace" => {
                // 一行里画不出括线：括住的写在方括号里，标注照上下标接在后面，`\underbrace{a+b}_{n}` 写成 `[a+b]_(n)`。
                let body = self.group(at);
                let mut next = *at;
                while self.chars.get(next).is_some_and(|c| c.is_whitespace()) {
                    next += 1;
                }
                match self.chars.get(next) {
                    Some(&marker @ ('^' | '_')) => {
                        *at = next + 1;
                        let label = self.group(at);
                        out.push_str(&format!("[{}]{marker}({})", body.trim(), label.trim()));
                    }
                    _ => out.push_str(&body),
                }
            }
            "overset" | "stackrel" | "underset" => {
                let mark = self.group(at);
                let base = self.group(at);
                out.push_str(base.trim());
                let marker = if name == "underset" { '_' } else { '^' };
                self.script(out, &mark, marker);
            }
            "operatorname" => {
                // 自己起名的函数：和 `\sin` 一样两边空一格。`\operatorname*{…}` 的 `*` 不是字。
                if self.chars.get(*at) == Some(&'*') {
                    *at += 1;
                }
                let name = self.group(at);
                prev.func = spaced(out, &format!(" {} ", name.trim()));
            }
            "xrightarrow" | "xleftarrow" => {
                // 箭头上面写着字的：字照上标接在箭头后面。
                out.push(if name == "xrightarrow" { '→' } else { '←' });
                let label = self.group(at);
                plain_script(out, label.trim(), '^');
            }
            // 只管颜色、留空的命令：颜色名、长度不是式子里的字，`\textcolor{red}{x}` 留下 `x`。
            "textcolor" | "color" | "hspace" | "vspace" | "phantom" | "hphantom" | "vphantom" => {
                self.group(at);
                if name == "textcolor" {
                    out.push_str(&self.group(at));
                } else if name != "color" {
                    out.push(' ');
                }
            }
            name if math.text.iter().any(|t| t == name) => out.push_str(&self.group(at)),
            name if math.ignored.iter().any(|t| t == name) => *prev = was,
            name if math.spaces.iter().any(|t| t == name) => out.push(' '),
            name if math.accents.contains_key(name) => {
                let argument = self.group(at);
                let lined = matches!(name, "overline" | "underline" | "cancel");
                out.push_str(&accented(argument.trim(), &math.accents[name], lined));
            }
            name => match math.symbols.get(name) {
                Some(symbol) => {
                    prev.func = spaced(out, symbol);
                    prev.big = matches!(
                        name,
                        "sum"
                            | "prod"
                            | "coprod"
                            | "int"
                            | "iint"
                            | "iiint"
                            | "oint"
                            | "bigcup"
                            | "bigcap"
                    );
                }
                None => out.push_str(&format!("\\{name}")),
            },
        }
        None
    }

    /// 环境 `\begin{…}…\end{…}` 的一行写法：格用 `, ` 隔开、行用 `; ` 隔开，矩阵两边写上定界符（`[1, 2; 3, 4]`），
    /// `cases` 前面一个 `{`；`aligned` 这类一行一个式子，`&` 只管对齐，不写。没收尾的（回答还在流）有多少写多少。
    fn environment(&self, at: &mut usize, out: &mut String) {
        let Some((name, after)) = env_name(self.chars, *at) else {
            out.push_str("\\begin");
            return;
        };
        *at = after;
        let kind = env(&name);
        if matches!(name.as_str(), "array" | "alignat" | "alignedat") {
            // 写各列怎么对齐的那个参数，一行里用不上。
            *at = braced(self.chars, *at).unwrap_or(*at);
        }
        let depth = self.depth.get() + 1;
        if depth > MAX_NESTING || self.too_deep.get() {
            self.too_deep.set(true);
            return;
        }
        self.depth.set(depth);
        let mut rows = Vec::new();
        let mut row = Vec::new();
        let closed = loop {
            let (cell, ended) = self.sequence(at, Stop::Cell);
            row.push(cell.trim().to_string());
            match ended {
                Ended::Amp => {}
                Ended::Row => rows.push(std::mem::take(&mut row)),
                ended => break ended == Ended::Env,
            }
        };
        rows.push(row);
        self.depth.set(depth - 1);
        // 最后一个 `\\` 后面没有字的那一行不算。
        if rows.len() > 1 && rows.last().is_some_and(|r| r.iter().all(String::is_empty)) {
            rows.pop();
        }
        let joined = |cells: &str| {
            rows.iter()
                .map(|r| r.join(cells))
                .collect::<Vec<_>>()
                .join("; ")
        };
        match kind {
            Env::Matrix(open, close) => out.push_str(&format!("{open}{}{close}", joined(", "))),
            Env::Array => out.push_str(&joined(", ")),
            Env::Cases => {
                // 值和条件之间原文多半自己带着逗号（`x, & x\ge 0`），不再添一个。
                let rows: Vec<String> = rows
                    .iter()
                    .map(|cells| {
                        let mut row = String::new();
                        for cell in cells {
                            if !row.is_empty() {
                                row.push_str(if row.ends_with(',') { " " } else { ", " });
                            }
                            row.push_str(cell);
                        }
                        row
                    })
                    .collect();
                out.push_str(&format!("{{ {}", rows.join("; ")));
            }
            Env::Lines | Env::Centered => out.push_str(&joined("")),
            Env::Unknown => {
                out.push_str(&format!("\\begin{{{name}}} {}", joined("")));
                if closed {
                    out.push_str(&format!(" \\end{{{name}}}"));
                }
            }
        }
    }

    /// 可选参数 `[…]`（`\sqrt[3]{x}` 的 3）：32 个字以内找得到 `]` 的才算，转好交回。
    fn optional(&self, at: &mut usize) -> Option<String> {
        if self.chars.get(*at) != Some(&'[') {
            return None;
        }
        let inner = &self.chars[*at + 1..self.chars.len().min(*at + 33)];
        let len = inner.iter().position(|c| *c == ']')?;
        let source: String = inner[..len].iter().collect();
        *at += len + 2;
        Some(unicode(&source, self.math))
    }

    /// 上下标写进 `out`：每个字都有对应的才转成上下标的字；不然一个字的写成 `^μ`，不止一个字的括起来写成 `^(3N)`，
    /// 读的人才分得清上下标到哪里结束。
    fn script(&self, out: &mut String, content: &str, marker: char) {
        let trimmed = content.trim();
        let table = if marker == '^' {
            &self.math.superscripts
        } else {
            &self.math.subscripts
        };
        match trimmed.chars().map(|c| table.get(&c).copied()).collect() {
            Some::<String>(text) => out.push_str(&text),
            None => plain_script(out, trimmed, marker),
        }
    }

    /// 分子、分母：分子是几个因子连写（[`Self::factors`]）、分母是一个因子的不用括，别的括起来，不然优先级错了：
    /// `\frac{a+1}{b}` 写成 `(a+1)/b`，`\frac{1}{2n}` 写成 `1/(2n)`。
    fn term(&self, text: &str, bottom: bool) -> String {
        let trimmed = text.trim();
        if self.factors(trimmed, !bottom) {
            trimmed.to_string()
        } else {
            format!("({trimmed})")
        }
    }

    /// 是不是一个因子：一个字母、一个数或者一对括号括着的（前面可以有根号），后面可以带上下标、撇、阶乘号。
    /// `c⁴`、`(-1)ᵏ`、`e^(-x²)`、`√(ac-b²)`、`k!` 都是。`many` 时几个因子紧挨着连写的也算（`2πG`、`n(n+1)`）。
    fn factors(&self, text: &str, many: bool) -> bool {
        let chars: Vec<char> = text.chars().collect();
        let small = |c: char| {
            combining(c)
                || matches!(c, '′' | '!')
                || (!c.is_ascii()
                    && (self.math.superscripts.values().any(|v| *v == c)
                        || self.math.subscripts.values().any(|v| *v == c)))
        };
        // 一对括号：交回闭括号后面的位置。
        let group = |open: usize| {
            let mut depth = 0usize;
            for (i, c) in chars.iter().enumerate().skip(open) {
                match c {
                    '(' => depth += 1,
                    ')' if depth <= 1 => return Some(i + 1),
                    ')' => depth -= 1,
                    _ => {}
                }
            }
            None
        };
        let mut at = 0;
        let mut count = 0;
        while at < chars.len() {
            count += 1;
            if count > 1 && !many {
                return false;
            }
            at += usize::from(chars[at] == '√');
            at = match chars.get(at) {
                Some('(') => match group(at) {
                    Some(after) => after,
                    None => return false,
                },
                Some(c) if c.is_ascii_digit() => {
                    at + chars[at..]
                        .iter()
                        .take_while(|c| c.is_ascii_digit() || **c == '.')
                        .count()
                }
                // 一串汉字是一个词（`\text{时间}`）。
                Some(c) if cjk(*c) => at + chars[at..].iter().take_while(|c| cjk(**c)).count(),
                Some(c) if c.is_alphanumeric() => at + 1,
                _ => return false,
            };
            // 这个因子的上下标、撇、阶乘号。
            loop {
                at = match (chars.get(at), chars.get(at + 1)) {
                    (Some(&c), _) if small(c) => at + 1,
                    (Some('^' | '_'), Some('(')) => match group(at + 1) {
                        Some(after) => after,
                        None => return false,
                    },
                    (Some('^' | '_'), Some(c)) if !c.is_whitespace() => at + 2,
                    _ => break,
                };
            }
        }
        true
    }

    /// 根号下面的：一个字、一个数、已经整个括起来的不用再括，别的都括起来：`√(2πm)`、`√(x²)`。
    fn radicand(text: &str) -> String {
        let trimmed = text.trim();
        let simple = trimmed.chars().filter(|c| !combining(*c)).count() <= 1
            || trimmed.chars().all(|c| c.is_ascii_digit() || c == '.')
            || wrapped(trimmed);
        if simple {
            trimmed.to_string()
        } else {
            format!("({trimmed})")
        }
    }

    /// 分式前面是不是不挨着别的因子：什么都没有、大算符、运算符、关系符、开括号。
    fn loose_before(&self, out: &str, was: Prev) -> bool {
        was.big
            || out.trim_end().chars().last().is_none_or(|c| {
                loose(c) || matches!(c, '(' | '[' | '{' | '⟨' | '⌈' | '⌊' | '|' | '‖')
            })
    }

    /// 分式后面是不是不挨着别的因子：写到头了、运算符、关系符、闭括号、分隔的空白（`\quad`）、下一格。小间距
    /// （`\,`）和只管大小的命令（`\right`）跳过去看后面。
    fn loose_after(&self, mut at: usize) -> bool {
        loop {
            match (self.chars.get(at), self.chars.get(at + 1)) {
                (None, _) | (Some('\\'), None | Some('\\' | '}')) => return true,
                (Some(c), _) if *c == '~' || c.is_whitespace() => at += 1,
                (Some('\\'), Some(',' | ';' | ':' | '!' | ' ')) => at += 2,
                (Some('\\'), Some(c)) if c.is_ascii_alphabetic() => {
                    let name: String = self.chars[at + 1..]
                        .iter()
                        .take(32)
                        .take_while(|c| c.is_ascii_alphabetic())
                        .collect();
                    if sizing(&name) {
                        at += 1 + name.len();
                        continue;
                    }
                    let symbol = self.math.symbols.get(&name);
                    return matches!(name.as_str(), "end" | "quad" | "qquad")
                        || symbol
                            .and_then(|s| s.trim_start().chars().next())
                            .is_some_and(|c| loose(c) || closing(c));
                }
                (Some('\\'), Some(_)) => return false,
                (Some(&c), _) => return (loose(c) && c != '|') || closing(c) || c == '.',
            }
        }
    }
}

/// 运算符、关系符、标点：一行的分式挨着它们不会读错。
fn loose(c: char) -> bool {
    "=+-−±∓<>≤≥≪≫≈≠≡∼≃≅∝→←↔⇒⇐⇔↦,;:&∈∉∋⊂⊃⊆⊇∪∩∧∨".contains(c)
}

fn closing(c: char) -> bool {
    matches!(c, ')' | ']' | '}' | '⟩' | '⌉' | '⌋')
}

/// 写一个符号。函数名两边带着空格（` sin `）：前面已经是空白、开括号、运算符的，前面那个空格不要（`-sin θ`、
/// `(sin x)`）。交回它是不是函数名（后面带着空格）。
fn spaced(out: &mut String, symbol: &str) -> bool {
    let opened = out.ends_with([
        ' ', '(', '[', '{', '|', '+', '-', '−', '±', '∓', '=', '<', '>', '·', '×', '/',
    ]);
    out.push_str(match symbol.strip_prefix(' ') {
        Some(rest) if opened => rest,
        _ => symbol,
    });
    symbol.starts_with(' ') && symbol.ends_with(' ')
}

/// 函数名后面带着的空格去掉：后面紧跟的是括号、上下标（`\sin(x)`、`\sin^2 x`、`\lim_{x\to0}`）。
fn tighten(out: &mut String, func: bool) {
    while func && out.ends_with(' ') {
        out.pop();
    }
}

/// 转不成上下标字符的上下标：一个字的直接接在记号后面，不止一个字的括起来。
fn plain_script(out: &mut String, text: &str, marker: char) {
    if text.is_empty() {
        return;
    }
    out.push(marker);
    if text.chars().filter(|c| !combining(*c)).count() <= 1 || wrapped(text) {
        out.push_str(text);
    } else {
        out.push_str(&format!("({text})"));
    }
}

/// 汉字、假名这些（`\text{…}` 里的词）。
fn cjk(c: char) -> bool {
    matches!(c, '\u{2e80}'..='\u{9fff}' | '\u{ac00}'..='\u{d7af}' | '\u{f900}'..='\u{faff}')
}

/// 组合用的附标字符（接在前一个字上，不占地方）。
fn combining(c: char) -> bool {
    matches!(c, '\u{300}'..='\u{36f}' | '\u{20d0}'..='\u{20ff}')
}

/// 附标：一个字的接在它后面；不止一个字的，上划线、下划线每个字各接一个，别的（`\vec{AB}`）括起来再接，不然读成
/// 只有最后一个字带附标。
fn accented(argument: &str, mark: &str, lined: bool) -> String {
    if argument.chars().filter(|c| !combining(*c)).count() <= 1 {
        format!("{argument}{mark}")
    } else if lined {
        let mut out = String::new();
        let mut chars = argument.chars().peekable();
        while let Some(c) = chars.next() {
            out.push(c);
            // 这个字自己带着的附标先写完，再接这一个。
            while let Some(more) = chars.next_if(|c| combining(*c)) {
                out.push(more);
            }
            if !c.is_whitespace() {
                out.push_str(mark);
            }
        }
        out
    } else if wrapped(argument) {
        format!("{argument}{mark}")
    } else {
        format!("({argument}){mark}")
    }
}

/// `\mathbb{…}` 的双线字：大写字母和数字有，别的照原字。
fn blackboard(c: char) -> char {
    let shifted = |base: u32, from: char| char::from_u32(base + (c as u32 - from as u32));
    match c {
        'C' => 'ℂ',
        'H' => 'ℍ',
        'N' => 'ℕ',
        'P' => 'ℙ',
        'Q' => 'ℚ',
        'R' => 'ℝ',
        'Z' => 'ℤ',
        'A'..='Z' => shifted(0x1d538, 'A').unwrap_or(c),
        '0'..='9' => shifted(0x1d7d8, '0').unwrap_or(c),
        c => c,
    }
}

/// `\not` 后面的符号划掉：有现成的字用现成的（`\not=` 是 `≠`），没有的接一个组合用的斜线。
fn negated(target: &str) -> String {
    let ready = match target {
        "=" => "≠",
        "<" => "≮",
        ">" => "≯",
        "≤" => "≰",
        "≥" => "≱",
        "≡" => "≢",
        "∼" => "≁",
        "≈" => "≉",
        "∈" => "∉",
        "∋" => "∌",
        "⊂" => "⊄",
        "⊃" => "⊅",
        "⊆" => "⊈",
        "⊇" => "⊉",
        "∃" => "∄",
        "|" => "∤",
        "∥" => "∦",
        _ => "",
    };
    if !ready.is_empty() {
        return ready.to_string();
    }
    let mut chars = target.chars();
    match chars.next() {
        Some(first) => format!("{first}\u{338}{}", chars.as_str()),
        None => String::new(),
    }
}

/// 整个被同一对括号包着：`(a)+(b)` 两头虽是括号，头一个在中间就合上了，不算。
fn wrapped(text: &str) -> bool {
    if !text.starts_with('(') || !text.ends_with(')') {
        return false;
    }
    let mut depth = 0usize;
    for (i, c) in text.char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return i + c.len_utf8() == text.len();
                }
            }
            _ => {}
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::{Math, unicode};

    fn math() -> Math {
        serde_json::from_str(include_str!("../../../../resources/tui/math.json")).unwrap()
    }

    #[test]
    fn scripts_greek_and_fractions() {
        let m = math();
        assert_eq!(unicode("x^2 + y_n", &m), "x² + yₙ");
        // 命令后面的一个空格是分隔，吃掉。
        assert_eq!(unicode(r"\alpha \in (0,1)", &m), "α∈(0,1)");
        assert_eq!(unicode(r"\frac{a+1}{b}", &m), "(a+1)/b");
        assert_eq!(unicode(r"\frac{(a)+(b)}{c}", &m), "((a)+(b))/c");
        assert_eq!(unicode(r"\sqrt{\pi}", &m), "√π");
        assert_eq!(unicode(r"\hat{x}", &m), "x\u{302}");
        // 没有上标写法的字，整段写成 ^(…)。
        assert_eq!(unicode("e^{-x^2}", &m), "e^(-x²)");
        assert_eq!(unicode("2^{f+g}", &m), "2^(f+g)");
    }

    #[test]
    fn unknown_commands_and_deep_nesting_stay_as_written() {
        let m = math();
        assert_eq!(unicode(r"\foo{x}", &m), r"\foox");
        let deep = format!("{}x{}", "{".repeat(100), "}".repeat(100));
        assert_eq!(unicode(&deep, &m), deep);
    }

    #[test]
    fn deep_nesting_without_braces_falls_back_to_the_source() {
        // review A8（2026-10-11）：`\sqrt\sqrt…x` 这种不带括号的嵌套原来不数层（只数 `{`），几万层把界面线程的栈撑爆，
        // 进程直接退出、终端不复原。超过上限的照原样写。
        let m = math();
        let tex = format!("{}x", r"\sqrt".repeat(100_000));
        assert_eq!(unicode(&tex, &m), tex);
        assert_eq!(unicode(r"\sqrt\sqrt x", &m), "√(√x)", "浅的照常转");
    }
}
