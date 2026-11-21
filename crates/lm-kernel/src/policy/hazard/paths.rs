//! 删的目标照字面换成绝对路径再判（施工 5-17；施工 AS-9 从 `rules.rs` 挪出来，加上通配和 `cd`）：`~`、`$HOME` 接家目录，
//! 相对的接这时的当前目录（同一条命令里 `cd` 过的照 `cd` 到的，说不清的交回空的），`.`、`..` 照字面去掉。通配的照它能删到
//! 的最大的那一个算：`dir/*`、`dir/?*`、`dir/{*,.*}` 照删 `dir`，`~/.cach*` 照删装着缓存那一处的 `~/.cache`。

use super::Kind;
use super::Places;
use super::lex::Word;

/// 通配的字。
const GLOB: [char; 4] = ['*', '?', '[', '{'];

/// 删一个目标：照字面换成绝对路径（通配的照它删得到的最大的那一个），再照 [`Places`] 判。`cwd` 是这时的当前目录，`cd` 到
/// 说不清的地方以后是空的（相对的目标照说不清算）。
pub(super) fn target(
    word: &Word,
    places: &Places<'_>,
    cwd: Option<&str>,
    recursive: bool,
) -> Option<Kind> {
    let Some(path) = absolute(word, places, cwd) else {
        return recursive.then_some(Kind::DeleteUnresolved);
    };
    let path = widest(&path, places);
    if path == "/" {
        return Some(Kind::DeleteRoot);
    }
    if let Some(home) = places.home
        && inside(&path, home)
    {
        return Some(Kind::DeleteHome);
    }
    // 自己的几处之一、装着它的，或者几处里面不让工具碰的（LM-37：原来连记忆的文件也拒，配置、缓存、状态三处一处都不认）。
    if holds_own(&path, places)
        || (places.own.iter().any(|own| inside(own, &path))
            && !places.open.iter().any(|open| inside(open, &path)))
    {
        return Some(Kind::DeleteDataRoot);
    }
    if path.rfind('/') == Some(0) {
        return Some(Kind::DeleteSystem);
    }
    if inside(&path, places.cwd) {
        return Some(Kind::DeleteWorkingDir);
    }
    None
}

/// 挪走、扔进回收站一个来源（LM-37，同 Miyu RO-4补「只管数据根」）：照字面换成绝对路径（通配的照它挪得到的最大的那一个），
/// 是自己的一处或者装着它的报删数据根，别的（系统目录、工作目录、几处里面的单个文件）不报。说不清的不报。
pub(super) fn moved_away(word: &Word, places: &Places<'_>, cwd: Option<&str>) -> Option<Kind> {
    let path = absolute(word, places, cwd)?;
    holds_own(&widest(&path, places), places).then_some(Kind::DeleteDataRoot)
}

/// 规整过的绝对路径 `path` 是自己的一处或者装着它的。
fn holds_own(path: &str, places: &Places<'_>) -> bool {
    places.own.iter().any(|own| inside(path, own))
}

/// `of` 是 `dir` 自己或者在它里面。
fn inside(dir: &str, of: &str) -> bool {
    of == dir || of.starts_with(&format!("{}/", dir.trim_end_matches('/')))
}

/// 通配的路径删得到的最大的那一个（施工 AS-9）：通配只在最后一段才看。整段都是通配（`*`、`.*`、`?*`、`{*,.*}`）的照删它
/// 所在的目录；`前缀*`（后面只有 `*`）的照删同一层里要保护的那几个（家目录、数据根、工作目录，或者通往它们的那一级）里名字
/// 合前缀的。别的（`*.o`、通配在中间那一段）照原样：根下面的照原样也是根下一级（系统目录），别处的认不出什么。
fn widest(path: &str, places: &Places<'_>) -> String {
    let Some(glob) = path.find(GLOB) else {
        return path.to_string();
    };
    let slash = path[..glob].rfind('/').unwrap_or(0);
    let (dir, last) = (&path[..slash], &path[slash + 1..]);
    if last.contains('/') {
        return path.to_string();
    }
    let dir = if dir.is_empty() { "/" } else { dir };
    if last.contains('*')
        && last
            .chars()
            .all(|c| matches!(c, '*' | '?' | '.' | '{' | '}' | ','))
    {
        return dir.to_string();
    }
    let at = last.find(GLOB).unwrap_or(last.len());
    let (prefix, rest) = last.split_at(at);
    // 前缀后面只有 `*` 的才是「名字以它开头的都算」；`*.log` 这类有尾巴的删不到要保护的那几个。
    if prefix.is_empty() || !rest.chars().all(|c| c == '*') {
        return path.to_string();
    }
    let below = |place: &str| -> Option<String> {
        let rest = place
            .strip_prefix(dir.trim_end_matches('/'))?
            .strip_prefix('/')?;
        let name = rest.split('/').next()?;
        name.starts_with(prefix)
            .then(|| format!("{}/{name}", dir.trim_end_matches('/')))
    };
    // 根下面的不用另算：照原样就是根下一级，照系统目录判。
    places
        .home
        .into_iter()
        .chain(places.own.iter().copied())
        .chain([places.cwd])
        .find_map(below)
        .unwrap_or_else(|| path.to_string())
}

/// 照字面换成绝对路径，去掉 `.`、`..`、多余的 `/`：`~`、`$HOME`、`${HOME}` 开头的接家目录，相对的接 `cwd`（空的交回空的）。
/// 别的变量、命令替换说不清，交回空的。
pub(super) fn absolute(word: &Word, places: &Places<'_>, cwd: Option<&str>) -> Option<String> {
    let text = word.text.as_str();
    let (base, rest) = if let Some(rest) = text
        .strip_prefix("${HOME}")
        .or_else(|| text.strip_prefix("$HOME"))
    {
        (places.home?, rest)
    } else if word.unresolved {
        return None;
    } else if text == "~" || text.starts_with("~/") {
        (places.home?, &text[1..])
    } else if text.starts_with('~') {
        // `~名字`（LM-37）：那个用户的家目录，不知道在哪，说不清。原来当成工作目录下一个叫 `~名字` 的文件，`rm -rf ~me`
        // 删的是自己的家目录却判成安全。
        return None;
    } else if text.starts_with('/') {
        ("/", text)
    } else {
        (cwd?, text)
    };
    if rest.contains('$') {
        return None;
    }
    let mut parts: Vec<&str> = Vec::new();
    for part in base.split('/').chain(rest.split('/')) {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            part => parts.push(part),
        }
    }
    Some(format!("/{}", parts.join("/")))
}
