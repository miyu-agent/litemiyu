//! 启动时从配置里要的几样（设计第五节第 12、13 条）：界面语言（`ui.language`），终端自己的几项（`tui.*`，`prefs.rs` 换上），
//! 新会话默认的聊天模型（`models.chat`：还没开会话时底栏写它，Miyu 终端连上核心时要的那一项），要不要进第一次运行的开头
//! （没有配置文件，或者上次的开头没走完）。只读 `config.toml` 这一份、只认这几项；配置界面的清单、字表、目录都不碰。
//!
//! 文件里没写 `language`、`tui` 这几个字的只读一次文件、不解析：默认的聊天模型照字面找（[`scan_chat`]）。读不了、读不懂的照出厂的办，不在这里
//! 报：第一次发话时引擎整份读配置，读不了的照原话说给人听（`core/engine/setup.rs`）。

use lm_engine::settings::declared::{AUTO, TuiSettings, UiSettings};
use lm_engine::store::paths::{Env, Paths};
use lm_kernel::config::parse::parse;
use lm_kernel::config::{Layer, Value};
use lm_kernel::models::settings::UseSettings;

use crate::language::{Language, LanguageTable};

/// 启动时读到的。
#[derive(Debug, Clone, PartialEq)]
pub struct Startup {
    /// 界面语言：配置里手动选了的照它，`auto`、没写、表里没有的照系统语言。
    pub language: Language,
    /// 有没有 `config.toml`。
    pub configured: bool,
    /// 进第一次运行的开头（`opening.rs`）：没有 `config.toml`，或者状态目录里还留着开头没走完的记号。
    pub opening: bool,
    /// 配置里写了的终端自己的几项（键、值）。
    pub tui: Vec<(String, Value)>,
    /// 新会话默认的聊天模型（`models.chat`）；没写的是 `None`。
    pub chat: Option<String>,
    /// 默认预设不是全部功能时它的名字（[`scan_default_preset`]，LM-29）：首页照它写新会话会用哪个预设。
    pub preset: Option<String>,
}

impl Startup {
    /// 写了的 `key` 那一项。
    pub fn tui(&self, key: &str) -> Option<Value> {
        self.tui
            .iter()
            .find(|(written, _)| written == key)
            .map(|(_, value)| value.clone())
    }
}

/// 第一次运行的开头没走完的记号：状态目录里的这个文件（开头一起来就写下，走完删掉）。
pub fn opening_mark(paths: &Paths) -> std::path::PathBuf {
    paths.state.join("opening")
}

/// 照环境 `var` 找数据放哪、读配置。
pub fn read(table: &LanguageTable, var: impl Fn(&str) -> Option<String>) -> Startup {
    let auto = table.detect(&var);
    let Ok(paths) = Paths::locate(&Env::current()) else {
        return Startup {
            language: auto,
            configured: false,
            opening: true,
            tui: Vec::new(),
            chat: None,
            preset: None,
        };
    };
    read_at(&paths, table, auto)
}

/// 同 [`read`]，数据在 `paths` 那一处，跟系统时是 `auto`。
///
/// 照原样读字节，不经 `config_file::read`：那一份还要算整份的 SHA-256 当版本（配置界面存的时候比对用），启动时用不上，
/// 算它的代码页就一直在内存里。
pub fn read_at(paths: &Paths, table: &LanguageTable, auto: Language) -> Startup {
    let file = std::fs::read(paths.config_file());
    let configured = !matches!(&file, Err(error) if error.kind() == std::io::ErrorKind::NotFound);
    let opening = !configured || opening_mark(paths).exists();
    let has = |bytes: &[u8], word: &[u8]| bytes.windows(word.len()).any(|w| w == word);
    let text = file.ok().and_then(|bytes| String::from_utf8(bytes).ok());
    let text = text.as_deref().map(|text| text.trim_start_matches(BOM));
    let parsed = text
        // 没写 `language`、`tui` 这几个字的不解析（大多数人不改）：TOML 的解析不进启动的路（红线 1、4）。
        .filter(|text| has(text.as_bytes(), LANGUAGE) || has(text.as_bytes(), TUI))
        .and_then(|text| {
            let items = [UiSettings::ITEMS, TuiSettings::ITEMS, UseSettings::ITEMS].concat();
            parse(&items, Layer::System, text).ok()
        });
    let chosen = parsed.as_ref().and_then(|parsed| {
        let entry = parsed
            .entries
            .get("ui.language")
            .filter(|entry| entry.counts)?;
        match &entry.value {
            Value::Text(code) if code != AUTO => table.find(code),
            _ => None,
        }
    });
    // 默认的聊天模型：解析过的照解析的；没解析的照字面找（[`scan_chat`]），不为底栏这一格把 TOML 的解析拉进启动的路。
    let chat = match &parsed {
        Some(parsed) => parsed
            .entries
            .get("models.chat")
            .filter(|entry| entry.counts)
            .and_then(|entry| match &entry.value {
                Value::Text(chat) if !chat.is_empty() => Some(chat.to_string()),
                _ => None,
            }),
        None => text.and_then(scan_chat),
    };
    let tui = parsed
        .map(|parsed| {
            parsed
                .entries
                .into_iter()
                .filter(|(key, entry)| entry.counts && key.starts_with("tui."))
                .map(|(key, entry)| (key, entry.value))
                .collect()
        })
        .unwrap_or_default();
    Startup {
        language: chosen.unwrap_or(auto),
        configured,
        opening,
        tui,
        chat,
        preset: text.and_then(scan_default_preset),
    }
}

/// 照字面在 `config.toml` 里找默认的聊天模型（`models.chat`），不解析整份 TOML（第二十六趟：第二十五趟为底栏写模型让配了
/// 模型的都走 TOML 的解析，空闲时代码页多 72 kB，界面线第二十六趟）。认两种写法：`[models]` 表里的 `chat = "…"`
/// （配置界面、开头存的都是这种），表头以前的 `models.chat = "…"`；值是一行里的基本字符串（不带 `\` 转义）或字面字符串，
/// 后面可以跟注释。别的写法（行内表 `models = { chat = … }`、多行字符串、带转义的）认不出，交 `None`：首页底栏空着，开了会话
/// 照推来的写（Miyu 照核心读的整份配置，没有这一处）。`@` 开头的（LM-8 以前指着模型池）也交 `None`：整份解析照写法不对丢掉它，
/// 引擎说「没有可用的模型」，底栏不写一个用不了的名字。
pub fn scan_chat(text: &str) -> Option<String> {
    scan_string(text, "models", "chat").filter(|chat| !chat.is_empty() && !chat.starts_with('@'))
}

/// 照字面在 `config.toml` 里找默认预设（`sessions.default`，LM-29），不解析整份 TOML（同 [`scan_chat`]，认的写法也一样）：
/// 首页输入框下面那一行照它写新会话会用哪个预设。交回的是不是全部功能时的那个名字：没写的、写 `full` 的是 `None`；`bare`
/// 照交；自己定义的要配置里照字面找得到它那张表的表头（`[presets.<名字>]`、`[presets."<名字>"]`），找不到的（删掉了、写法
/// 认不出）当全部功能，同引擎开会话时的退法。
pub fn scan_default_preset(text: &str) -> Option<String> {
    use lm_engine::presets::{BARE, FULL};
    let name = scan_string(text, "sessions", "default")?;
    if name == FULL {
        return None;
    }
    if name == BARE {
        return Some(name);
    }
    let headers = [format!("[presets.{name}]"), format!("[presets.\"{name}\"]")];
    let defined = text.lines().map(str::trim).any(|line| {
        headers
            .iter()
            .any(|header| line.starts_with(header.as_str()))
    });
    (defined && lm_engine::presets::valid_name(&name)).then_some(name)
}

/// 照字面找 `[<table>]` 表里的 `<key> = "…"`，或者表头以前的 `<table>.<key> = "…"`（见 [`scan_chat`]）。
fn scan_string(text: &str, wanted: &str, wanted_key: &str) -> Option<String> {
    let dotted = format!("{wanted}.{wanted_key}");
    let quoted = format!("\"{wanted_key}\"");
    let mut table: Option<&str> = Some("");
    // 在多行字符串里（别处写的长段落）：里面像表头、像 `chat =` 的行不算。
    let mut open: Option<&str> = None;
    for line in text.lines() {
        if let Some(delimiter) = open {
            if line.matches(delimiter).count() % 2 == 1 {
                open = None;
            }
            continue;
        }
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        open = ["\"\"\"", "'''"]
            .into_iter()
            .find(|delimiter| line.matches(delimiter).count() % 2 == 1);
        if open.is_some() {
            continue;
        }
        if let Some(header) = line.strip_prefix('[') {
            // `[[…]]` 是表数组，不是要找的那张表。
            table = match header.starts_with('[') {
                true => None,
                false => header.split(']').next().map(str::trim),
            };
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let ours = match table {
            Some(table) if table == wanted => key == wanted_key || key == quoted,
            Some("") => key == dotted,
            _ => false,
        };
        if ours {
            return string_value(value.trim());
        }
    }
    None
}

/// 一行里的字符串值：`"…"`（不带转义）或 `'…'`，后面只能是空白、注释。
fn string_value(value: &str) -> Option<String> {
    let quote = value.chars().next().filter(|c| *c == '"' || *c == '\'')?;
    let rest = &value[1..];
    // 多行字符串（三个引号）不认。
    if rest.starts_with(quote) {
        return None;
    }
    let (inside, after) = rest.split_once(quote)?;
    if quote == '"' && inside.contains('\\') {
        return None;
    }
    let after = after.trim_start();
    (after.is_empty() || after.starts_with('#')).then(|| inside.to_string())
}

/// 认 `ui.language` 那一项的字。
const LANGUAGE: &[u8] = b"language";

/// 认终端自己那几项的字。
const TUI: &[u8] = b"tui";

/// 文件开头可能带的 BOM（同 `config_file`）。
const BOM: char = '\u{feff}';

#[cfg(test)]
mod tests {
    use lm_engine::store::paths::Paths;

    use super::read_at;
    use crate::language::LanguageTable;

    fn at(config: Option<&str>) -> super::Startup {
        let dir = std::env::temp_dir().join(format!(
            "litemiyu-startup-{}-{}",
            std::process::id(),
            config.map_or(0, str::len)
        ));
        let paths = Paths::under(&dir);
        std::fs::create_dir_all(&paths.config).unwrap();
        if let Some(text) = config {
            std::fs::write(paths.config_file(), text).unwrap();
        }
        let table = LanguageTable::builtin().unwrap();
        let auto = table.find("zh").unwrap();
        let read = read_at(&paths, &table, auto);
        std::fs::remove_dir_all(&dir).unwrap();
        read
    }

    #[test]
    fn no_config_follows_the_system_and_says_so() {
        let read = at(None);
        assert_eq!(read.language.code(), "zh");
        assert!(!read.configured);
    }

    #[test]
    fn a_chosen_language_wins_over_the_system() {
        let read = at(Some("[ui]\nlanguage = \"en\"\n"));
        assert_eq!(read.language.code(), "en");
        assert!(read.configured);
    }

    #[test]
    fn auto_and_broken_files_follow_the_system() {
        assert_eq!(
            at(Some("[ui]\nlanguage = \"auto\"\n")).language.code(),
            "zh"
        );
        let broken = at(Some("[ui\n"));
        assert_eq!(broken.language.code(), "zh");
        assert!(broken.configured, "文件在，读不懂也是配过");
    }

    #[test]
    fn the_terminal_items_written_are_read_and_a_missing_file_opens_the_opening() {
        let read = at(Some("[tui]\nkeys = \"vim\"\ntheme_background = false\n"));
        assert_eq!(
            read.tui("tui.keys"),
            Some(lm_kernel::config::Value::Text("vim".into()))
        );
        assert_eq!(
            read.tui("tui.theme_background"),
            Some(lm_kernel::config::Value::Bool(false))
        );
        assert_eq!(read.tui("tui.icons"), None, "没写的不算");
        assert!(!read.opening, "配过、没有记号的直接进对话");
        assert!(at(None).opening);
    }

    /// 还没开会话时底栏写默认的聊天模型：照 `config.toml` 的 `models.chat`，没写的、指着以前的模型池的（LM-8 砍了）是 `None`。
    #[test]
    fn the_default_chat_model_is_read_for_the_footer() {
        let read = at(Some(
            "[providers.dev]\ndriver = \"openai-chat\"\n\n[models]\nchat = \"dev/m1\"\n",
        ));
        assert_eq!(read.chat.as_deref(), Some("dev/m1"));
        assert_eq!(read.language.code(), "zh", "别的照旧");
        assert_eq!(
            at(Some("[models]\nchat = \"@daily\"\n")).chat,
            None,
            "指着以前的池的不写"
        );
        assert_eq!(
            at(Some(
                "[models]\nchat = \"@daily\"\n\n[ui]\nlanguage = \"zh\"\n"
            ))
            .chat,
            None,
            "整份解析的那一条路也一样"
        );
        assert_eq!(
            at(Some("[providers.chatty]\n")).chat,
            None,
            "没写 models.chat"
        );
        assert_eq!(at(None).chat, None);
    }

    /// 第二十六趟：没写 `language`、`tui` 的照字面找默认的聊天模型，不解析整份 TOML。认得出的几种和整份解析读出来的一样；
    /// 认不出的写法交 `None`（底栏首页空着），不认错。
    #[test]
    fn the_chat_model_is_found_without_parsing_the_whole_file() {
        use super::scan_chat;
        let found = [
            (
                "[providers.dev]\ndriver = \"openai-chat\"\n\n[models]\nchat = \"dev/m1\"\n",
                "dev/m1",
            ),
            ("models.chat = 'dev/m2' # 注释\n[providers.dev]\n", "dev/m2"),
            (
                "[ models ]\n\"chat\" = \"dev/daily\"\nfast = \"dev/x\"\n",
                "dev/daily",
            ),
            (
                "[providers.dev]\nnote = \"\"\"\n[models]\nchat = \"不是这个\"\n\"\"\"\n[models]\nchat = \"dev/m3\"\n",
                "dev/m3",
            ),
        ];
        for (text, want) in found {
            assert_eq!(scan_chat(text).as_deref(), Some(want), "{text}");
            let items = lm_kernel::models::settings::UseSettings::ITEMS;
            let parsed =
                lm_kernel::config::parse::parse(items, lm_kernel::config::Layer::System, text)
                    .expect("合 TOML");
            assert_eq!(
                parsed.entries.get("models.chat").map(|e| e.value.clone()),
                Some(lm_kernel::config::Value::Text(want.into())),
                "和整份解析读出来的一样：{text}"
            );
        }
        for text in [
            "[providers.chatty]\ndriver = \"openai-chat\"\n",
            "[models.extra]\nchat = \"dev/x\"\n",
            "[providers.dev]\nmodels.chat = \"dev/x\"\n",
            "models = { chat = \"dev/x\" }\n",
            "[models]\nchat = \"a\\\\b\"\n",
            "[[pools]]\nchat = \"dev/x\"\n",
            "[models]\nchat = \"\"\n",
            "[models]\nchat = \"@daily\"\n",
        ] {
            assert_eq!(scan_chat(text), None, "{text}");
        }
    }
    #[test]
    fn the_default_preset_is_found_literally_when_it_is_not_all_features() {
        use super::scan_default_preset;
        let web = "[presets.web]\nunlisted = \"off\"\n";
        for (text, want) in [
            ("", None),
            ("[sessions]\ndefault = \"full\"\n", None),
            ("[sessions]\ndefault = \"bare\"\n", Some("bare")),
            ("sessions.default = \"bare\" # 只聊天\n", Some("bare")),
            (
                &format!("[sessions]\ndefault = \"web\"\n\n{web}"),
                Some("web"),
            ),
            (
                "[sessions]\ndefault = \"只用命令\"\n[presets.\"只用命令\"]\nunlisted = \"off\"\n",
                Some("只用命令"),
            ),
            // 写的名字没有那张表（删掉了）：照全部功能，同引擎开会话时的退法。
            ("[sessions]\ndefault = \"web\"\n", None),
            (
                "[sessions]\ndefault = \"web\"\n[presets.web-lite]\nunlisted = \"on\"\n",
                None,
            ),
            // 别的表里的 default 不算。
            (&format!("[other]\ndefault = \"web\"\n{web}"), None),
        ] {
            assert_eq!(scan_default_preset(text).as_deref(), want, "{text}");
        }
    }
}
