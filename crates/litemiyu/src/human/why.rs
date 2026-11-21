//! 引擎、系统交上来的原因换成给人看的句子（LM-39，同 Miyu WD-6）：引擎的英文原话（`open elsewhere`、`session stopped`）、
//! 发请求那一层的原话（「连接断了：…」）、系统错误（`Permission denied (os error 13)`）、网络库的原话（`dns error`、证书）。
//! 字在 `text/<语言>.json` 的 `why` 一节，原因码的短话在同一份的 `refusal_hints`：中文界面不露英文原话和原因码，英文界面
//! 也换成完整的句子。认不出的照原话。
//!
//! 全屏界面照 [`Texts::why`](crate::config::Texts) 用读好的那一份；配置界面、命令行、工具结果的字用 [`table`]：第一次要时
//! 从同一份 JSON 里只取这两节，一种语言读一次。

use std::collections::HashMap;
use std::sync::OnceLock;

use crate::config::read::texts;

texts! {
    /// `text/<语言>.json` 的 `why`。
    #[derive(Debug, Clone)]
    pub struct Why {
        /// 整句对得上的原话 → 给人看的句子。
        pub exact: HashMap<String, String>,
        /// 带占位的原话 → 给人看的句子：`[原话, 句子]`，照先后试，`{名字}` 两边一样。名字是 `why` 的那一段里面再换一次。
        pub patterns: Vec<[String; 2]>,
        /// 系统错误、网络错误的种类（[`kind`]）→ 给人看的句子。
        pub os: HashMap<String, String>,
        /// 认出了系统错误、前面还带着文件名这类的：`{what}` 是前面那一段，`{why}` 是种类的句子。
        pub os_at: String,
    }
}

impl Why {
    /// 原因 `raw` 给人看的句子；认不出的照原话。
    pub fn say(&self, raw: &str) -> String {
        self.said(raw.trim(), 0).unwrap_or_else(|| raw.to_string())
    }

    /// 认得出的换成句子；认不出的是 `None`。`depth` 防着句子里套句子没完。
    fn said(&self, raw: &str, depth: u8) -> Option<String> {
        if raw.is_empty() {
            return None;
        }
        if let Some(text) = self.exact.get(raw) {
            return Some(text.clone());
        }
        if depth < 3 {
            for [from, to] in &self.patterns {
                if let Some(fields) = matched(from, raw) {
                    let mut text = to.clone();
                    for (name, value) in fields {
                        let value = match name {
                            "why" => self
                                .said(value, depth + 1)
                                .unwrap_or_else(|| value.to_string()),
                            _ => value.to_string(),
                        };
                        text = text.replace(&format!("{{{name}}}"), &value);
                    }
                    return Some(text);
                }
            }
        }
        self.system(raw)
    }

    /// 系统错误、网络库的原话：认出种类的换成那一种的句子。只有系统的原话、前面带的是文件名这一类（没有空格）的接上它，
    /// 前面是别的英文的只写种类。
    fn system(&self, raw: &str) -> Option<String> {
        if let Some((prefix, kind)) = os_error(raw) {
            let why = self.os.get(kind)?;
            let what = prefix.trim_end().trim_end_matches(':').trim_end();
            return Some(match what {
                "" => why.clone(),
                what if !what.contains(' ') => {
                    self.os_at.replace("{what}", what).replace("{why}", why)
                }
                _ => why.clone(),
            });
        }
        let kind = network(raw)?;
        self.os.get(kind).cloned()
    }
}

/// `raw` 照 `pattern` 对：字面的几段按先后对上，`{名字}` 那几段取出来（不能是空的）。对不上的是 `None`。
fn matched<'a, 'p>(pattern: &'p str, raw: &'a str) -> Option<Vec<(&'p str, &'a str)>> {
    let mut fields = Vec::new();
    let mut rest = raw;
    let mut pattern = pattern;
    // 头一段字面的要正好在开头。
    let lead = pattern.find('{').unwrap_or(pattern.len());
    rest = rest.strip_prefix(&pattern[..lead])?;
    pattern = &pattern[lead..];
    while let Some(after) = pattern.strip_prefix('{') {
        let close = after.find('}')?;
        let name = &after[..close];
        pattern = &after[close + 1..];
        let next = pattern.find('{').unwrap_or(pattern.len());
        let literal = &pattern[..next];
        pattern = &pattern[next..];
        let value = match (literal.is_empty(), pattern.is_empty()) {
            // 最后一格：剩下的全是它。
            (true, true) => std::mem::take(&mut rest),
            // 两格挨着，分不开。
            (true, false) => return None,
            // 后面还有字面的：最后一格要对到结尾，中间的取最近的一处。
            (false, true) => {
                let value = rest.strip_suffix(literal)?;
                rest = "";
                value
            }
            (false, false) => {
                let at = rest.find(literal)?;
                let value = &rest[..at];
                rest = &rest[at + literal.len()..];
                value
            }
        };
        if value.is_empty() {
            return None;
        }
        fields.push((name, value));
    }
    rest.is_empty().then_some(fields)
}

/// 系统错误：末尾是 `(os error <号>)` 的照这台机器的错误号认（同 Miyu WD-6 的 `os_error_kind`），交回它前面的那一段和种类；
/// 没有错误号、整句就是一种错误的说法（`io::Error::from(kind)` 印的，例如 `permission denied`）的也认，前面是空的。
fn os_error(raw: &str) -> Option<(&str, &'static str)> {
    if let Some((before, code)) = raw.rsplit_once("(os error ") {
        let code: i32 = code.strip_suffix(')')?.parse().ok()?;
        let error = std::io::Error::from_raw_os_error(code);
        let kind = kind(error.kind())?;
        // 系统的说法（`strerror`）在前面：去掉它，剩下的是前面带的那一段。
        let said = error.to_string();
        let said = said
            .rsplit_once(" (os error ")
            .map_or(said.as_str(), |(text, _)| text);
        let prefix = before.trim_end().strip_suffix(said).unwrap_or(before);
        return Some((prefix, kind));
    }
    KINDS
        .iter()
        .find(|kind| std::io::Error::from(**kind).to_string() == raw)
        .and_then(|found| kind(*found))
        .map(|kind| ("", kind))
}

/// 认得出的几种系统错误。
const KINDS: [std::io::ErrorKind; 22] = {
    use std::io::ErrorKind as K;
    [
        K::NotFound,
        K::PermissionDenied,
        K::ConnectionRefused,
        K::ConnectionReset,
        K::ConnectionAborted,
        K::NotConnected,
        K::AddrInUse,
        K::AddrNotAvailable,
        K::NetworkUnreachable,
        K::HostUnreachable,
        K::NetworkDown,
        K::BrokenPipe,
        K::AlreadyExists,
        K::NotADirectory,
        K::IsADirectory,
        K::DirectoryNotEmpty,
        K::ReadOnlyFilesystem,
        K::StorageFull,
        K::QuotaExceeded,
        K::FileTooLarge,
        K::TimedOut,
        K::OutOfMemory,
    ]
};

/// 一种系统错误在 `why.os` 里的名字；不在 [`KINDS`] 里的是 `None`。
fn kind(kind: std::io::ErrorKind) -> Option<&'static str> {
    use std::io::ErrorKind as K;
    Some(match kind {
        K::NotFound => "not_found",
        K::PermissionDenied => "permission_denied",
        K::ConnectionRefused => "connection_refused",
        K::ConnectionReset | K::ConnectionAborted | K::BrokenPipe => "connection_reset",
        K::NotConnected => "not_connected",
        K::AddrInUse => "addr_in_use",
        K::AddrNotAvailable => "addr_not_available",
        K::NetworkUnreachable | K::NetworkDown => "network_unreachable",
        K::HostUnreachable => "host_unreachable",
        K::AlreadyExists => "already_exists",
        K::NotADirectory => "not_a_directory",
        K::IsADirectory => "is_a_directory",
        K::DirectoryNotEmpty => "directory_not_empty",
        K::ReadOnlyFilesystem => "read_only",
        K::StorageFull | K::QuotaExceeded => "storage_full",
        K::FileTooLarge => "file_too_large",
        K::TimedOut => "timed_out",
        K::OutOfMemory => "out_of_memory",
        _ => return None,
    })
}

/// 网络库（reqwest、hyper、rustls）的原话里认得出的几种：只看发请求、读响应出错的那一类原话，免得把供应商自己的话认错。
fn network(raw: &str) -> Option<&'static str> {
    const FROM: [&str; 4] = [
        "error sending request",
        "error trying to connect",
        "error decoding response body",
        "request or response body error",
    ];
    const NEEDLES: [(&str, &str); 9] = [
        ("dns error", "dns"),
        ("failed to lookup address", "dns"),
        ("certificate", "tls"),
        ("received fatal alert", "tls"),
        ("tls handshake", "tls"),
        ("connection refused", "connection_refused"),
        ("connection reset", "connection_reset"),
        (
            "connection closed before message completed",
            "connection_reset",
        ),
        ("timed out", "timed_out"),
    ];
    let lower = raw.to_lowercase();
    if !FROM.iter().any(|from| lower.contains(from)) {
        return None;
    }
    NEEDLES
        .iter()
        .find(|(needle, _)| lower.contains(needle))
        .map(|(_, kind)| *kind)
}

/// 配置界面、命令行、工具结果用的一份：`why` 一节连同原因码的短话（`refusal_hints`）。
#[derive(Debug)]
pub struct Table {
    /// 原因的句子。
    pub why: Why,
    /// 原因码 → 短话。
    pub hints: HashMap<String, String>,
}

impl Table {
    /// 原因 `raw`（原因码、原话）给人看的句子：原因码先照短话，别的照 [`Why::say`]。
    pub fn say(&self, raw: &str) -> String {
        match self.hints.get(raw.trim()) {
            Some(hint) => hint.clone(),
            None => self.why.say(raw),
        }
    }
}

/// `code` 那种语言的一份（没有的照英文）：第一次要时读，读一次。编进程序的读不懂的（测试守着）是空的，原话照原样。
pub fn table(code: &str) -> &'static Table {
    static ZH: OnceLock<Table> = OnceLock::new();
    static EN: OnceLock<Table> = OnceLock::new();
    let (cell, code) = match code {
        "zh" => (&ZH, "zh"),
        _ => (&EN, "en"),
    };
    cell.get_or_init(|| crate::config::why_table(code).unwrap_or_else(|_| empty()))
}

/// 读不懂时的一份：什么都不认。
fn empty() -> Table {
    Table {
        why: Why {
            exact: HashMap::new(),
            patterns: Vec::new(),
            os: HashMap::new(),
            os_at: "{what}: {why}".to_string(),
        },
        hints: HashMap::new(),
    }
}

#[cfg(test)]
mod tests;
