//! 本机是什么系统（施工 2-7 再补，`26-提示词.md` 第四节第 3 块）：写进 system 核心的几行，她照它挑包管理器、写命令，不用先跑
//! 一遍 `cat /etc/os-release`。核心头一次要时认一次，之后照它（LM-5 起管 Linux、macOS、BSD）：
//!
//! - Linux、FreeBSD（13 起有）：`os-release`；
//! - macOS：`/System/Library/CoreServices/SystemVersion.plist` 的 `ProductVersion`，写成 `macOS 15` 这样（只要大版本）；
//! - 都认不出的：照编译时的系统写名字（`Linux`、`FreeBSD`、`OpenBSD`、`NetBSD`……）。
//!
//! 只写系统叫什么、大版本：小版本每个月都变，变了新开的会话 system 就和昨天的不一样，跨会话的缓存接不上。

use std::sync::OnceLock;

#[cfg(test)]
mod tests;

/// 系统的名字最长几个字：`/etc/os-release` 是发行版写的，太长的截掉，进 system 的只有一小截。
const MAX_CHARS: usize = 64;

/// 本机系统的名字：照 `/etc/os-release` 的 `NAME` 加 `VERSION_ID`（`Arch Linux`、`Ubuntu 24.04`）；macOS 是 `macOS 15` 这样。
pub fn os_name() -> &'static str {
    static NAME: OnceLock<String> = OnceLock::new();
    NAME.get_or_init(detect)
}

/// 认一次。
fn detect() -> String {
    if cfg!(target_os = "macos") {
        return std::fs::read_to_string("/System/Library/CoreServices/SystemVersion.plist")
            .ok()
            .and_then(|text| macos(&text))
            .unwrap_or_else(|| "macOS".to_string());
    }
    ["/etc/os-release", "/usr/lib/os-release"]
        .into_iter()
        .find_map(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| linux(&text))
        .unwrap_or_else(|| fallback(std::env::consts::OS))
}

/// 认不出的：照编译时的系统（`std::env::consts::OS`）写个名字。
fn fallback(os: &str) -> String {
    match os {
        "linux" => "Linux",
        "macos" => "macOS",
        "freebsd" => "FreeBSD",
        "openbsd" => "OpenBSD",
        "netbsd" => "NetBSD",
        "dragonfly" => "DragonFly BSD",
        other => other,
    }
    .to_string()
}

/// macOS 的 `SystemVersion.plist`：`<key>ProductVersion</key>` 后面那个 `<string>` 的大版本（`15.1` 写成 `macOS 15`）。
fn macos(plist: &str) -> Option<String> {
    let after = plist.split_once("<key>ProductVersion</key>")?.1;
    let value = after.split_once("<string>")?.1.split_once("</string>")?.0;
    let major = value.trim().split('.').next()?;
    (!major.is_empty() && major.chars().all(|c| c.is_ascii_digit()))
        .then(|| format!("macOS {major}"))
}

fn linux(os_release: &str) -> Option<String> {
    let field = |key: &str| {
        os_release.lines().find_map(|line| {
            let value = line.trim().strip_prefix(key)?.strip_prefix('=')?;
            let value = unquote(value.trim());
            (!value.is_empty()).then_some(value)
        })
    };
    let name = field("NAME")?;
    Some(clean(&match field("VERSION_ID") {
        Some(version) => format!("{name} {version}"),
        None => name,
    }))
}

/// 去掉 `os-release` 值两头的引号，引号里的 `\"` 这类转义照字面还原。
fn unquote(value: &str) -> String {
    let quoted = ['"', '\''].into_iter().find_map(|quote| {
        value
            .strip_prefix(quote)
            .and_then(|rest| rest.strip_suffix(quote))
    });
    match quoted {
        Some(inner) => inner.replace("\\\"", "\"").replace("\\\\", "\\"),
        None => value.to_string(),
    }
}

/// 进 system 的只留一行：控制字符换成空格，连着的空白并成一个，太长的截掉。
fn clean(name: &str) -> String {
    let spaced: String = name
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    spaced
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(MAX_CHARS)
        .collect()
}
