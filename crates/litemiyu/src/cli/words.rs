//! 命令行给人看的字：`resources/tui/cli/<语言>.json`（只有 `cli` 一格），一句一个编号。
//! 单放一份、不和全屏的 `text/<语言>.json` 放在一起：全屏启动读它自己那一份时不扫过命令行的字。只在命令行要说话时读，读选中的
//! 那一种；表里没有的语言照英文。报错、警告、提示的写法照 Miyu 设计 47：`litemiyu: ` 打头，警告多「警告：」，建议另起一行「提示：」。

use std::collections::HashMap;
use std::io::Write;

use crate::config::read::{parse, texts};

texts! {
    @shared
    /// 只有 `cli` 那一格。
    #[derive(Debug)]
    pub struct CliTexts {
        /// 编号到那一句；`{名字}` 是要填的。
        pub cli: HashMap<String, String>,
    }
}

/// 编进程序的两种语言（同 `config/localize.rs` 的那两份，编译器合成一份）。
const TEXTS: &[(&str, &str)] = &[
    ("zh", tui_json!("cli/zh.json")),
    ("en", tui_json!("cli/en.json")),
];

/// 读好的一种语言的字。
#[derive(Debug)]
pub struct Words {
    /// 语言的写法：`zh`、`en`。
    code: &'static str,
    /// 编号到那一句。
    map: HashMap<String, String>,
}

impl Words {
    /// 读 `code` 那一种；没有的照英文。编进程序的读不懂的（测试守着）是空的，说话时照编号原样写。
    pub fn load(code: &str) -> Words {
        let (code, json) = TEXTS
            .iter()
            .find(|(c, _)| *c == code)
            .or_else(|| TEXTS.iter().find(|(c, _)| *c == "en"))
            .copied()
            .unwrap_or(("en", "{}"));
        let map = parse::<CliTexts>(&format!("cli/{code}.json"), json)
            .map(|texts| texts.cli)
            .unwrap_or_default();
        Words { code, map }
    }

    /// 照界面语言读：`ui.language` 手动选了的照它，没选的照环境变量（`settings/startup.rs`）。
    pub fn current() -> Words {
        let code = match crate::language::LanguageTable::builtin() {
            Ok(table) => {
                let startup =
                    crate::settings::startup::read(&table, |name| std::env::var(name).ok());
                startup.language.code().to_string()
            }
            Err(_) => "en".to_string(),
        };
        Words::load(&code)
    }

    /// 语言的写法。
    pub fn code(&self) -> &'static str {
        self.code
    }

    /// 那一句；表里没有的照编号原样（测试查每个编号两种语言都有）。
    pub fn get<'a>(&'a self, key: &'a str) -> &'a str {
        self.map.get(key).map_or(key, String::as_str)
    }

    /// 那一句，填上 `fields`（`{名字}` 换成值）。
    pub fn say(&self, key: &str, fields: &[(&str, &str)]) -> String {
        let mut text = self.get(key).to_string();
        for (name, value) in fields {
            text = text.replace(&format!("{{{name}}}"), value);
        }
        text
    }

    /// 标准错误上报一句错：`litemiyu: <那一句>`，有建议的另起一行。
    pub fn error(&self, message: &str, hint: Option<&str>) {
        let mut err = std::io::stderr().lock();
        // 标准错误写不进的没处可报。
        drop(writeln!(err, "litemiyu: {}", clean(message)));
        if let Some(hint) = hint {
            drop(writeln!(err, "{}{}", self.get("hint"), clean(hint)));
        }
    }

    /// 标准错误上报一句警告：`litemiyu: 警告：<那一句>`，有建议的另起一行。
    pub fn warn(&self, message: &str, hint: Option<&str>) {
        let warning = format!("{}{}", self.get("warning"), message);
        self.error(&warning, hint);
    }
}

/// 报错里控制字符换成 `�`：敲的参数可能混着终端的控制序列（Miyu `misuse`）；制表符照留。
pub fn clean(text: &str) -> String {
    text.chars()
        .map(|c| match c {
            '\t' => c,
            c if c.is_control() => '\u{fffd}',
            c => c,
        })
        .collect()
}
