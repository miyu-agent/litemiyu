//! 代码着色（蓝图 `tui.md`「代码着色」，照旧版 `render/code.rs` 起步）：词法上的近似，不做语法分析。认关键字、
//! 函数名、字符串、数字、注释、类型、常量、宏和属性、变量和参数、键、运算符、diff 的加减行。
//! 各语言认什么写在 `resources/code.json`；一个代码块从头到尾用一个 [`Highlighter`]，块注释、三引号字符串接到下一行。
//! 一行里怎么认在 `lexer.rs`。

mod lexer;

#[cfg(test)]
mod tests;

use std::collections::HashSet;
use std::sync::OnceLock;

use serde::Deserialize;
use serde_json::value::RawValue;

use super::inline::Piece;
use crate::theme;

/// 代码着色的表（`code.json`，一种语言一条）。拿到手还没读：第一次照名字找时才切成一条条、读出每条的名字，哪一种语言第一次
/// 用到才解哪一种（LM-43：原来第一次排回答就把整张表解出来留着，堆上约 45 kB，一段对话用到的只有一两种语言）。
#[derive(Debug, Clone)]
pub struct Languages {
    /// 整张表的原文（编进程序的那一份）。
    text: &'static str,
    /// 切好的一条条。
    entries: OnceLock<Vec<Entry>>,
}

/// 表里的一条：名字先读出来，认什么留着原文、第一次用到才解。
#[derive(Debug, Clone)]
struct Entry {
    /// 名字和别名，都转成了小写。
    names: Vec<String>,
    /// 这一条的原文。
    raw: &'static str,
    /// 解出来的。
    language: OnceLock<Language>,
}

/// `code.json` 最外面一层：每一条先不解。
#[derive(Deserialize)]
struct Shell<'a> {
    #[serde(borrow)]
    languages: Vec<&'a RawValue>,
}

/// 一条里只读名字。
#[derive(Deserialize)]
struct Names {
    names: Vec<String>,
}

/// 一种语言认什么。没写的都是不认。
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Language {
    /// 名字和别名，照代码块开头写的那个认，不分大小写。找的时候照切表时读好的那一份（[`Languages::find`]），这里只认下这个键。
    #[serde(rename = "names")]
    _names: serde::de::IgnoredAny,
    /// 关键字。
    #[serde(default)]
    pub keywords: Vec<String>,
    /// 关键字不分大小写（SQL、Dockerfile）。
    #[serde(default)]
    pub ignore_case: bool,
    /// 内建类型（`i32`、`str`、`int`）。
    #[serde(default)]
    pub types: Vec<String>,
    /// 常量（`true`、`None`、`nil`）。
    #[serde(default)]
    pub constants: Vec<String>,
    /// 大写开头、带小写的名字当类型（`Vec`、`String`）。
    #[serde(default)]
    pub capital_types: bool,
    /// 行注释的记号；空的是没有。
    #[serde(default)]
    pub comment: String,
    /// 块注释的头和尾（`/*`、`*/`），能跨行。
    #[serde(default)]
    pub block_comment: Option<[String; 2]>,
    /// 单引号括起来的也是字符串。
    #[serde(default)]
    pub single_quote_strings: bool,
    /// 反引号括起来的也是字符串。
    #[serde(default)]
    pub backtick_strings: bool,
    /// 三引号字符串（Python），能跨行。
    #[serde(default)]
    pub triple_quote_strings: bool,
    /// `名字!` 是宏（Rust）。
    #[serde(default)]
    pub macros: bool,
    /// `#[…]` 是属性（Rust）。
    #[serde(default)]
    pub attributes: bool,
    /// 一行打头的 `#名字` 是预处理指令（C/C++ 的 `#include`、`#define`）。
    #[serde(default)]
    pub preprocessor: bool,
    /// `@名字` 是装饰器、注解。
    #[serde(default)]
    pub decorators: bool,
    /// `$名字`、`${…}`、`$(…)` 是变量（shell、PHP、Makefile）。
    #[serde(default)]
    pub variables: bool,
    /// `-v`、`--force` 是命令的参数（shell）。
    #[serde(default)]
    pub flags: bool,
    /// 键和值之间的记号（`:`、`=`）：一行打头（或 `{`、`,`、`-` 后面）紧跟这个记号的名字、字符串是键。空的是不认键。
    #[serde(default)]
    pub keys: String,
    /// 一行只有 `[段]` 的是段名（TOML、INI）。
    #[serde(default)]
    pub sections: bool,
    /// `<标签 属性="…">`（HTML、XML）。
    #[serde(default)]
    pub tags: bool,
    /// diff：`+` 开头的行是加的、`-` 开头的是删的。
    #[serde(default)]
    pub diff: bool,
}

impl Languages {
    /// 拿着 `code.json` 的原文，这时不读。
    pub const fn new(text: &'static str) -> Self {
        Self {
            text,
            entries: OnceLock::new(),
        }
    }

    /// 照代码块开头写的语言找；没写语言、没登记的是 `None`。没写语言的不去读表。
    ///
    /// # Panics
    ///
    /// 编进程序的 `code.json` 读不懂：安装坏了才会有，测试把每一条都解一遍守着（`code/tests.rs`）。
    pub fn find(&self, name: &str) -> Option<&Language> {
        let name = name.trim().to_lowercase();
        if name.is_empty() {
            return None;
        }
        let entry = self
            .entries()
            .iter()
            .find(|entry| entry.names.contains(&name))?;
        Some(entry.language.get_or_init(|| {
            lm_kernel::kernel::json::from_str(entry.raw).expect("编进程序的 code.json 读得懂")
        }))
    }

    /// 切好的一条条：第一次要时切。
    fn entries(&self) -> &[Entry] {
        self.entries.get_or_init(|| {
            let shell: Shell<'static> =
                serde_json::from_str(self.text).expect("编进程序的 code.json 读得懂");
            shell
                .languages
                .into_iter()
                .map(|raw| {
                    let names: Names = lm_kernel::kernel::json::from_str(raw.get())
                        .expect("编进程序的 code.json 读得懂");
                    Entry {
                        names: names.names.iter().map(|n| n.to_lowercase()).collect(),
                        raw: raw.get(),
                        language: OnceLock::new(),
                    }
                })
                .collect()
        })
    }

    /// 表里每一条的头一个名字（测试用）。
    #[cfg(test)]
    pub(crate) fn firsts(&self) -> Vec<String> {
        self.entries()
            .iter()
            .filter_map(|entry| entry.names.first().cloned())
            .collect()
    }

    /// 解过的有几条（测试用）。
    #[cfg(test)]
    pub(crate) fn read(&self) -> usize {
        self.entries.get().map_or(0, |all| {
            all.iter().filter(|e| e.language.get().is_some()).count()
        })
    }

    /// 切过没有（测试用）。
    #[cfg(test)]
    pub(crate) fn opened(&self) -> bool {
        self.entries.get().is_some()
    }
}

/// 走到哪了：上一行留下的块注释、三引号字符串接到这一行。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// 平常。
    Code,
    /// 在块注释里。
    Comment,
    /// 在三引号字符串里，引号是哪种。
    Triple(char),
}

/// 一个代码块的着色：一行一行喂，记着上一行走到哪了。
pub struct Highlighter<'a> {
    language: Option<&'a Language>,
    keywords: HashSet<String>,
    types: HashSet<&'a str>,
    constants: HashSet<&'a str>,
    state: State,
}

impl<'a> Highlighter<'a> {
    /// 照这种语言着色；没登记的（`None`）不上色。
    pub fn new(language: Option<&'a Language>) -> Self {
        let fold = |w: &str| {
            if language.is_some_and(|l| l.ignore_case) {
                w.to_lowercase()
            } else {
                w.to_string()
            }
        };
        Self {
            language,
            keywords: language.map_or_else(HashSet::new, |l| {
                l.keywords.iter().map(|k| fold(k)).collect()
            }),
            types: language.map_or_else(HashSet::new, |l| {
                l.types.iter().map(String::as_str).collect()
            }),
            constants: language.map_or_else(HashSet::new, |l| {
                l.constants.iter().map(String::as_str).collect()
            }),
            state: State::Code,
        }
    }

    /// 一行着好色的片段，拼起来就是这一行。
    pub fn line(&mut self, line: &str) -> Vec<Piece> {
        // 没写语言、没登记的语言不上色（`tui.md`「代码着色」第 3 条）。
        if self.language.is_none() {
            return plain(line);
        }
        if self.language.is_some_and(|l| l.diff) {
            return diff_line(line);
        }
        self.scan(line)
    }

    /// 这个名字是不是关键字（照语言分不分大小写）。
    fn is_keyword(&self, word: &str) -> bool {
        if self.language.is_some_and(|l| l.ignore_case) {
            self.keywords.contains(&word.to_lowercase())
        } else {
            self.keywords.contains(word)
        }
    }
}

/// 不上色的一行：照原色。
fn plain(line: &str) -> Vec<Piece> {
    if line.is_empty() {
        return Vec::new();
    }
    vec![Piece {
        text: line.to_string(),
        style: ratatui::style::Style::new(),
        link: None,
    }]
}

/// diff 的一行：文件头和 `@@` 宏的颜色，`+` 加的颜色，`-` 删的颜色，别的原色。
fn diff_line(line: &str) -> Vec<Piece> {
    if line.is_empty() {
        return Vec::new();
    }
    let style = if line.starts_with("+++") || line.starts_with("---") || line.starts_with("@@") {
        theme::code_macro()
    } else if line.starts_with('+') {
        theme::added()
    } else if line.starts_with('-') {
        theme::removed()
    } else {
        ratatui::style::Style::new()
    };
    vec![Piece::new(line, style)]
}
