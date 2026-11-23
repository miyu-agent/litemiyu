//! 装进 shell、移除、看装没装（从 Miyu 终端的 `shellhook/install.rs` 搬来）：fish 是 `conf.d` 下自己的一个文件，bash、zsh 是
//! 启动文件里带记号的一段；装进去的只有调 `shellhook script` 的一行，升级以后新开的 shell 自动用新的（坑 A1）。不是自己写的
//! 文件不动（A3）；再装一次是替换原来那一段（A4）；启动文件是符号链接的顺着链接写，不把链接换成普通文件（A5）；记的程序是
//! `PATH` 上指着同一个文件的那个路径（A6）；别家的集成也在的不装（A7）。

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use super::shells::{Other, Shell};

/// fish 那个文件的第一行、bash 和 zsh 那一段的开头：认它是不是自己写的。
const MARK: &str = "# >>> litemiyu shellhook >>>";
/// bash、zsh 那一段的结尾。
const END: &str = "# <<< litemiyu shellhook <<<";
/// Miyu 的 shell 集成那一段的开头、结尾（它的 `shellhook/install.rs`）。
const MIYU_MARK: &str = "# >>> miyu shellhook >>>";
const MIYU_END: &str = "# <<< miyu shellhook <<<";
/// 旧版 Miyu 0.7.x 在 bash、zsh 的启动文件里写的那一段的开头、结尾（它的 `shell/bash.rs`、`shell/zsh.rs`）。
const OLD_MARKS: [(&str, &str); 2] = [
    ("# >>> miyu bash hook >>>", "# <<< miyu bash hook <<<"),
    ("# >>> miyu zsh hook >>>", "# <<< miyu zsh hook <<<"),
];
/// 停用别家 fish 的那个文件时改成的名字后面加的这一截：`conf.d` 只读 `.fish` 结尾的，改了名就不起作用，东西还在。
const RETIRED: &str = ".replaced-by-litemiyu";

/// 装好了的一份是什么样。
#[derive(Debug, PartialEq, Eq)]
pub enum Found {
    /// 没装。
    Missing,
    /// 自己写的，调的是这个程序（读不出来的是 `None`）。
    Ours(Option<PathBuf>),
    /// fish 的那个文件在，却不是自己写的。
    Foreign,
}

/// 装完是哪一种。
#[derive(Debug, PartialEq, Eq)]
pub enum Done {
    /// 新装的。
    Installed,
    /// 原来就有，换成了这一份。
    Updated,
    /// fish 的文件不是自己写的，没动。
    Foreign,
}

/// 别家的集成在哪：文件、第几行（整个文件都是它的没有行号）、哪一种。
#[derive(Debug, PartialEq, Eq)]
pub struct Elsewhere {
    pub path: PathBuf,
    pub line: Option<usize>,
    pub kind: Other,
}

/// 默认调哪个程序：这个程序自己。`PATH` 上有指着同一个文件的，记 `PATH` 上那个（[`on_path`]）。
///
/// # Errors
///
/// 认不出自己在哪。
pub fn default_program() -> io::Result<PathBuf> {
    let me = std::env::current_exe()?;
    Ok(on_path(&me, std::env::var_os("PATH").as_deref()).unwrap_or(me))
}

/// `PATH`（`path`）上同名、指着同一个文件的另一个路径。认自己在哪会解开链接（Linux 读 `/proc/self/exe`）：Homebrew 把程序
/// 放在带版本号的目录里（`Cellar/litemiyu/<版本>/bin/litemiyu`）、在 `bin` 里放链接，记真路径的升级、清掉旧版以后装进去的
/// 那一行就不起作用了，链接还在。只认同一个文件。它自己就在 `PATH` 上的照旧（Arch 的 `/bin` 是 `/usr/bin` 的链接，不换成
/// `/bin/litemiyu`）。
pub fn on_path(program: &Path, path: Option<&std::ffi::OsStr>) -> Option<PathBuf> {
    let real = fs::canonicalize(program).ok()?;
    let name = program.file_name()?;
    let found: Vec<PathBuf> = std::env::split_paths(path?)
        .filter(|dir| dir.is_absolute())
        .map(|dir| dir.join(name))
        .collect();
    if found.iter().any(|found| found == program) {
        return None;
    }
    found
        .into_iter()
        .find(|found| fs::canonicalize(found).is_ok_and(|found| found == real))
}

/// 装进去的那一段。
pub fn block(shell: Shell, program: &Path) -> String {
    let p = shell.quote(program);
    let note = "# 由 `litemiyu shellhook install` 写入，用 `litemiyu shellhook uninstall` 移除";
    let name = shell.name();
    match shell {
        Shell::Fish => format!(
            "{MARK}\n{note}\nif status is-interactive; and test -x {p}\n    {p} shellhook script --shell {name} --program {p} | source\nend\n"
        ),
        Shell::Bash => format!(
            "{MARK}\n{note}\nif [[ $- == *i* ]] && [ -x {p} ]; then eval \"$({p} shellhook script --shell {name} --program {p})\"; fi\n{END}\n"
        ),
        Shell::Zsh => format!(
            "{MARK}\n{note}\nif [[ -o interactive && -x {p} ]]; then eval \"$({p} shellhook script --shell {name} --program {p})\"; fi\n{END}\n"
        ),
    }
}

/// 装进 `path`。
///
/// # Errors
///
/// 读写失败。
pub fn install_at(shell: Shell, path: &Path, program: &Path) -> io::Result<Done> {
    let block = block(shell, program);
    let (text, done) = match (shell, read(path)?) {
        (_, None) => (block, Done::Installed),
        (Shell::Fish, Some(old)) if old.starts_with(MARK) => (block, Done::Updated),
        (Shell::Fish, Some(_)) => return Ok(Done::Foreign),
        (_, Some(old)) => match cut(&old) {
            Some((before, after)) => (format!("{before}{block}{after}"), Done::Updated),
            None => {
                let gap = if old.is_empty() || old.ends_with('\n') {
                    ""
                } else {
                    "\n"
                };
                (format!("{old}{gap}{block}"), Done::Installed)
            }
        },
    };
    write(path, &text)?;
    Ok(done)
}

/// 从 `path` 移除：移除了的回 `true`，本来就没有的回 `false`。fish 不是自己写的文件也算没有。
///
/// # Errors
///
/// 读写失败。
pub fn uninstall_at(shell: Shell, path: &Path) -> io::Result<bool> {
    let Some(old) = read(path)? else {
        return Ok(false);
    };
    match shell {
        Shell::Fish if old.starts_with(MARK) => {
            fs::remove_file(path)?;
            Ok(true)
        }
        Shell::Fish => Ok(false),
        Shell::Bash | Shell::Zsh => match cut(&old) {
            Some((before, after)) => {
                write(path, &format!("{before}{after}"))?;
                Ok(true)
            }
            None => Ok(false),
        },
    }
}

/// `path` 里装的是什么样。
///
/// # Errors
///
/// 读失败（不在的不算）。
pub fn found(shell: Shell, path: &Path) -> io::Result<Found> {
    let Some(text) = read(path)? else {
        return Ok(Found::Missing);
    };
    let ours = match shell {
        Shell::Fish if text.starts_with(MARK) => Some(text.as_str()),
        Shell::Fish => return Ok(Found::Foreign),
        Shell::Bash | Shell::Zsh => cut(&text).map(|(before, _)| &text[before.len()..]),
    };
    Ok(match ours {
        Some(ours) => Found::Ours(program_in(ours)),
        None => Found::Missing,
    })
}

/// 别家的集成（坑 A7）：fish 看 `conf.d` 下那两个文件在不在；bash、zsh 看启动文件 `path` 里有没有 Miyu 那一段的记号，另把
/// 自己那一段以外、看起来像在装 Miyu 集成的行（有 `miyu`、又有 `shell`，注释不算）记成「认不准」的。
///
/// # Errors
///
/// 读失败（不在的不算）。
pub fn elsewhere(
    shell: Shell,
    path: &Path,
    env: &impl Fn(&str) -> Option<String>,
) -> io::Result<Vec<Elsewhere>> {
    let mut all: Vec<Elsewhere> = shell
        .other_files(env)
        .into_iter()
        .filter(|(path, _)| path.exists())
        .map(|(path, kind)| Elsewhere {
            path,
            line: None,
            kind,
        })
        .collect();
    if shell == Shell::Fish {
        return Ok(all);
    }
    let Some(text) = read(path)? else {
        return Ok(all);
    };
    // 自己那一段、Miyu 那一段里面的行不再一行行看。
    let mine = cut(&text).map(|(before, after)| (before.len(), text.len() - after.len()));
    let mut at = 0;
    let mut inside_miyu = false;
    for (index, line) in text.split_inclusive('\n').enumerate() {
        let start = at;
        at += line.len();
        if mine.is_some_and(|(from, to)| start >= from && start < to) {
            continue;
        }
        let trimmed = line.trim();
        let kind = if trimmed == MIYU_MARK {
            inside_miyu = true;
            Some(Other::Miyu)
        } else if OLD_MARKS.iter().any(|(begin, _)| trimmed == *begin) {
            inside_miyu = true;
            Some(Other::OldMiyu)
        } else if trimmed == MIYU_END || OLD_MARKS.iter().any(|(_, end)| trimmed == *end) {
            inside_miyu = false;
            None
        } else if !inside_miyu && looks_like_miyu(trimmed) {
            Some(Other::Maybe)
        } else {
            None
        };
        if let Some(kind) = kind {
            all.push(Elsewhere {
                path: path.to_path_buf(),
                line: Some(index + 1),
                kind,
            });
        }
    }
    Ok(all)
}

/// 停用了别家的一份集成以后是哪一种。
#[derive(Debug, PartialEq, Eq)]
pub enum Retired {
    /// fish 的那个文件改了名（新的名字）：不再起作用，东西还在。
    Moved(PathBuf),
    /// bash、zsh 启动文件里的那一段删掉了。
    Cut,
}

/// 停用别家的一份集成（`shellhook install --replace`，或者它调的 `miyu` 已经不在了、留着只会让终端出错）。fish 的是整个
/// 文件：改名（加 [`RETIRED`]），不删。bash、zsh 的是启动文件里带记号的一段：从开头的记号删到结尾的记号。认不准的
/// （[`Other::Maybe`]）不归这里管。
///
/// 同一个启动文件里有几段的，调的一方从后往前停（前面的行号才不变）。
///
/// # Errors
///
/// 读写失败；那一段找不到结尾的记号（不照猜的删）。
pub fn retire(other: &Elsewhere) -> io::Result<Retired> {
    let Some(line) = other.line else {
        let mut name = other.path.clone().into_os_string();
        name.push(RETIRED);
        let moved = PathBuf::from(name);
        fs::rename(&other.path, &moved)?;
        return Ok(Retired::Moved(moved));
    };
    let unmarked = || io::Error::new(io::ErrorKind::InvalidData, "no end marker");
    let text = read(&other.path)?.ok_or_else(unmarked)?;
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let begin = line.checked_sub(1).ok_or_else(unmarked)?;
    let ends =
        |trimmed: &str| trimmed == MIYU_END || OLD_MARKS.iter().any(|(_, end)| trimmed == *end);
    let end = lines
        .iter()
        .enumerate()
        .skip(begin + 1)
        .find(|(_, line)| ends(line.trim()))
        .map(|(index, _)| index)
        .ok_or_else(unmarked)?;
    let kept: String = lines[..begin]
        .iter()
        .chain(&lines[end + 1..])
        .copied()
        .collect();
    write(&other.path, &kept)?;
    Ok(Retired::Cut)
}

/// `PATH`（`path`）上有没有叫 `name` 的程序：别家的集成调的 `miyu` 还在不在。
pub fn has_program(name: &str, path: Option<&std::ffi::OsStr>) -> bool {
    path.is_some_and(|path| {
        std::env::split_paths(path)
            .filter(|dir| dir.is_absolute())
            .any(|dir| dir.join(name).is_file())
    })
}

/// 这一行看起来像在装 Miyu 的集成：不是注释，去掉 `litemiyu` 以后还有 `miyu`，又提到 `shell`。
fn looks_like_miyu(line: &str) -> bool {
    !line.starts_with('#')
        && line.replace("litemiyu", "").contains("miyu")
        && line.contains("shell")
}

/// 装进去的那一段里记的程序：`-x` 后面那个单引号包着的词。
fn program_in(block: &str) -> Option<PathBuf> {
    // fish 的单引号里 \' \\ 是转义；POSIX 的引号里写不了引号，写成 '\''（[`Shell::quote`]）。
    let fish = block.contains("status is-interactive");
    let lead = "-x '";
    let quoted = &block[block.find(lead)? + lead.len()..];
    let mut out = String::new();
    let mut at = 0;
    while let Some(rest) = quoted.get(at..) {
        if fish && rest.starts_with('\\') {
            let c = rest[1..].chars().next()?;
            out.push(c);
            at += 1 + c.len_utf8();
        } else if !fish && rest.starts_with("'\\''") {
            out.push('\'');
            at += "'\\''".len();
        } else if rest.starts_with('\'') {
            return Some(PathBuf::from(out));
        } else {
            let c = rest.chars().next()?;
            out.push(c);
            at += c.len_utf8();
        }
    }
    None
}

/// 启动文件里自己那一段切出来：前面的、后面的（不含那一段）。没有的是 `None`。
fn cut(text: &str) -> Option<(&str, &str)> {
    let start = text
        .match_indices(MARK)
        .map(|(at, _)| at)
        .find(|&at| at == 0 || text[..at].ends_with('\n'))?;
    let end_at = text[start..].find(END)? + start + END.len();
    let end = text[end_at..]
        .strip_prefix('\n')
        .map_or(end_at, |_| end_at + 1);
    Some((&text[..start], &text[end..]))
}

/// 读一个文件；不在的是 `None`。
fn read(path: &Path) -> io::Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

/// 写一个文件：顺着符号链接写到它指的那个，先写在旁边再换过去，权限照原来的。
fn write(path: &Path, text: &str) -> io::Result<()> {
    let target = real(path)?;
    if let Some(dir) = target.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut name = target.file_name().unwrap_or_default().to_os_string();
    name.push(".litemiyu-shellhook.tmp");
    let temp = target.with_file_name(name);
    fs::write(&temp, text)?;
    if let Ok(meta) = fs::metadata(&target) {
        fs::set_permissions(&temp, meta.permissions())?;
    }
    fs::rename(&temp, &target)
}

/// 符号链接指的那个文件（一层层跟到底，指着不在的文件也照它）；不是链接的照原样。
fn real(path: &Path) -> io::Result<PathBuf> {
    let mut path = path.to_path_buf();
    for _ in 0..40 {
        match fs::symlink_metadata(&path) {
            Ok(meta) if meta.file_type().is_symlink() => {
                let link = fs::read_link(&path)?;
                path = match path.parent() {
                    Some(dir) if link.is_relative() => dir.join(link),
                    _ => link,
                };
            }
            _ => return Ok(path),
        }
    }
    Err(io::Error::other("too many levels of symbolic links"))
}
